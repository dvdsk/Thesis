use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::{SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use protocol::{Request, Response};
use tokio::net::TcpStream;
use tokio::time::sleep;
use tracing::{warn, info, instrument};

use super::Ministry;
use crate::random_node::RandomNode;
use crate::{RequestError, Ticket};

pub struct Connection {
    pub stream: MsgStream<Response, Request>,
    pub peer: SocketAddr,
}

impl Connection {
    async fn connect(addr: SocketAddr) -> Result<Self, io::Error> {
        let stream = TcpStream::connect(addr).await?;
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
        addr = nodes.random_node().await;
    }
}

impl<T: RandomNode> super::Client<T> {
    #[instrument(skip(self))]
    async fn send_request(
        &mut self,
        path: &Path,
        req: &Request,
        needs_minister: bool,
    ) {
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
                (addr, _) => { // hint addr is not bound, covers None case
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

    #[instrument(skip(self))]
    pub async fn follow_up_on_ticket(&mut self, path: &Path) -> Result<(), RequestError> {
        let msg = self.conn.as_mut().unwrap().stream.try_next().await;
        match msg {
            Ok(Some(Response::Done)) => return Ok(()),
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

    #[instrument(skip(self))]
    pub(super) async fn request<R: FromResponse>(
        &mut self,
        path: &Path,
        req: Request,
        needs_minister: bool,
    ) -> Result<R, RequestError> {
        loop {
            self.send_request(path, &req, needs_minister).await;
            let response = self.conn.as_mut().unwrap().stream.try_next().await;
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
                Ok(Some(Response::Redirect { staff, subtree })) => {
                    info!("updateing staff: {staff:?}");
                    self.map.insert(Ministry {
                        staff,
                        subtree,
                    });
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
            ()
        } else {
            panic!("invalid response, expected Done got: {resp:?}");
        }
    }
}

from_response!([List], Vec<PathBuf>);
from_response!([WriteLease, ReadLease], protocol::Lease);
