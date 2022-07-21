use std::iter::repeat;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use std::io;

use futures::{SinkExt, TryStreamExt};
use itertools::chain;
use protocol::connection::{self, MsgStream};
use protocol::{Request, Response, DirList};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tracing::{debug, info, instrument, trace, warn};

use super::Ministry;
use crate::random_node::RandomNode;
use crate::{RequestError, Ticket};

pub struct Connection {
    pub stream: MsgStream<Response, Request>,
    pub peer: SocketAddr,
}

impl Connection {
    async fn connect(addr: SocketAddr) -> Result<Self, io::Error> {
        let connect = TcpStream::connect(addr);
        let stream = timeout(Duration::from_millis(100), connect)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "timed out trying to connect"))??;

        let peer = stream.peer_addr()?;
        Ok(Self {
            stream: connection::wrap(stream),
            peer,
        })
    }
}

#[instrument(skip(nodes))]
async fn connect(initial_addr: Option<SocketAddr>, nodes: &impl RandomNode) -> Connection {
    let mut addr = match initial_addr {
        Some(addr) => addr,
        None => nodes.random_node().await,
    };

    loop {
        match Connection::connect(addr).await {
            Err(e) => warn!("failed to connect to {addr:?}, error: {e:?}"),
            Ok(conn) => return conn,
        }
        sleep(Duration::from_millis(100)).await;
        addr = nodes.random_node().await;
    }
}

impl<T: RandomNode> super::Client<T> {
    #[instrument(skip(self))]
    async fn send_request(&mut self, path: &Path, req: &Request, needs_minister: bool) {
        let Self {
            map, conn, nodes, ..
        } = self;
        loop {
            let node = match needs_minister {
                true => map.minister_for(path),
                false => map.clerk_for(path),
            };

            match (node, conn.as_mut()) {
                (None, Some(_conn)) => (),
                (Some(addr), Some(conn)) if conn.peer == addr => (),
                (addr, _) => {
                    // addr is not bound, therefore this covers None case
                    *conn = Some(connect(addr, nodes).await);
                }
            };

            let res = conn.as_mut().unwrap().stream.send(req.clone()).await;
            match res {
                Ok(_) => return,
                Err(e) => {
                    warn!("Could not send request, error: {e:?}");
                    let broken = conn.take().unwrap();
                    map.invalidate(path, broken.peer);
                    sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }
        }
    }

    #[instrument(skip(self), err(Debug))]
    pub async fn follow_up_on_ticket(&mut self, path: &Path) -> Result<(), RequestError> {
        let msg = self.conn.as_mut().unwrap().stream.try_next().await; // cant timeout here
        match msg {
            Ok(Some(Response::Done)) => return Ok(()),
            Ok(None) => warn!("Connection closed while following up on ticket"),
            Ok(msg) => panic!("unexpected msg while following up on ticket: {msg:?}"),
            Err(e) => warn!("Connection lost while following up on ticket: {e:?}"),
        }

        let ticket = self.ticket.take().ok_or(RequestError::NoTicket)?;
        let req = Request::IsCommitted {
            path: ticket.path,
            idx: ticket.idx,
        };
        loop {
            self.send_request(path, &req, ticket.needs_minister).await;
            let response = self.conn.as_mut().unwrap().stream.try_next().await;
            match response {
                Ok(Some(Response::NotCommitted)) => return Err(RequestError::Uncommitted),
                Ok(Some(Response::Committed)) => return Ok(()),
                Ok(None) => {
                    warn!("Connection was closed ");
                    let broken = self.conn.take().unwrap();
                    self.map.invalidate(path, broken.peer);
                }
                Err(e) => {
                    warn!("Error connecting: {e:?}");
                    let broken = self.conn.take().unwrap();
                    self.map.invalidate(path, broken.peer);
                }
                _ => unreachable!(),
            }
        }
    }

    /// recieve the response from the fs, if it is leasedropped eat that
    /// response and wait for another
    #[instrument(skip(self), ret)]
    async fn recieve(&mut self) -> Result<Option<Response>, io::Error> {
        loop {
            let msg = self.conn.as_mut().unwrap().stream.try_next();
            let msg = timeout(Duration::from_millis(500), msg)
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "timed out trying to connect"))?;
            if let Ok(Some(Response::LeaseDropped)) = msg {
                trace!("ignoring lease dropped; only valid while holding lease");
                continue;
            }
            return msg;
        }
    }

    #[instrument(skip(self))]
    pub(super) async fn request<R: FromResponse>(
        &mut self,
        path: &Path,
        req: Request,
        needs_minister: bool,
    ) -> Result<R, RequestError> {
        let mut no_capacity = chain!([100, 200], repeat(400)).map(Duration::from_millis);
        let mut redirect = chain!([10, 20, 40, 100], repeat(400)).map(Duration::from_millis);
        loop {
            self.send_request(path, &req, needs_minister).await;
            let response = self.recieve().await;
            debug!("response: {response:?}");
            match response {
                Ok(Some(Response::NotCommitted)) => (), // will trigger a retry
                Ok(Some(Response::Ticket { idx })) => {
                    self.ticket = Some(Ticket {
                        idx,
                        needs_minister,
                        path: path.to_owned(),
                    });
                    match self.follow_up_on_ticket(path).await {
                        Ok(_) => return Ok(R::from_response(Response::Done)),
                        Err(_) => continue, // send the request again
                    }
                }
                Ok(Some(Response::CouldNotRedirect)) => {
                    self.map.invalidate(path, self.conn.as_ref().unwrap().peer);
                    sleep(redirect.next().unwrap()).await;
                    continue;
                }
                Ok(Some(Response::Redirect { staff, subtree })) => {
                    info!("updating staff: {staff:?}");
                    self.map.insert(Ministry { staff, subtree });
                    sleep(redirect.next().unwrap()).await;
                    continue;
                }
                Ok(Some(Response::ConflictingWriteLease)) => {
                    sleep(Duration::from_millis(50)).await;
                    continue;
                }
                Ok(Some(Response::NoCapacity)) => {
                    sleep(no_capacity.next().unwrap()).await;
                    continue;
                }
                Ok(Some(response)) => return Ok(R::from_response(response)),
                Ok(None) => {
                    warn!("Connection was closed ");
                    let broken = self.conn.take().unwrap();
                    self.map.invalidate(path, broken.peer);
                }
                Err(e) => {
                    warn!("Error connecting: {e:?}");
                    let broken = self.conn.take().unwrap();
                    self.map.invalidate(path, broken.peer);
                }
            }
        }
    }
}

pub(super) trait FromResponse {
    fn from_response(resp: Response) -> Self;
}

macro_rules! from_response {
    ([$( $re:ident ),+], $ret:ty) => {
        impl FromResponse for $ret {
            fn from_response(resp: Response) -> Self {
                match resp {
                    $(Response::$re(inner) => inner,)+
                    _ => panic!("invalid response, expected Done got: {resp:?}"),
                }
            }
        }
    };
}

impl FromResponse for () {
    fn from_response(resp: Response) -> Self {
        if let Response::Done = resp {
        } else {
            panic!("invalid response, expected Done got: {resp:?}");
        }
    }
}

// impl FromResponse for Option<protocol::Lease> {
//     fn from_response(resp: Response) -> Self {
//         if let Response::Done = resp {
//             ()
//         } else {
//             panic!("invalid response, expected Done got: {resp:?}");
//         }
//     }
// }

from_response!([List], DirList);
from_response!([WriteLease, ReadLease], protocol::Lease);
