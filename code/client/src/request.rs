use std::io;
use std::net::SocketAddr;
use std::path::Path;

use futures::{SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use protocol::{Request, Response};
use tokio::net::TcpStream;
use tracing::warn;

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
    async fn send_request(
        &mut self,
        path: &Path,
        req: &Request,
        needs_minister: bool,
    ) -> Option<Ministry> {
        let Self {
            map, conn, nodes, ..
        } = self;
        loop {
            let ministry = map.ministry_for(path);
            let node = match needs_minister {
                true => ministry.map(|m| m.minister).flatten(),
                false => ministry.map(|m| m.clerk).flatten(),
            };

            let conn = match (node, conn.as_mut()) {
                (None, Some(conn)) => conn,
                (Some(addr), Some(conn)) if conn.peer == addr => conn,
                (addr, _) => {
                    *conn = Some(connect(addr, nodes).await);
                    conn.as_mut().unwrap()
                }
            };

            let res = conn.stream.send(req.clone()).await;
            match res {
                Ok(_) => return ministry.cloned(),
                Err(e) => {
                    warn!("Could not send request, error: {e:?}");
                    continue;
                }
            }
        }
    }

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
            let ministry = self.send_request(path, &req, ticket.needs_minister).await;
            let response = self.conn.as_mut().unwrap().stream.try_next().await;
            match response {
                Ok(Some(Response::NotCommitted)) => return Err(RequestError::Uncommitted),
                Ok(Some(Response::Committed)) => return Ok(()),
                Ok(None) => {
                    warn!("Connection was closed ");
                    let mut updated = ministry.unwrap();
                    updated.minister = None;
                    self.map.insert(updated);
                }
                Err(e) => {
                    warn!("Error connecting: {e:?}");
                    let mut updated = ministry.unwrap();
                    updated.minister = None;
                    self.map.insert(updated);
                }
                _ => unreachable!(),
            }
        }
    }

    pub(super) async fn request(
        &mut self,
        path: &Path,
        req: Request,
        needs_minister: bool,
    ) -> Result<(), RequestError> {
        loop {
            let ministry = self.send_request(path, &req, needs_minister).await;
            let response = self.conn.as_mut().unwrap().stream.try_next().await;
            match response {
                Ok(Some(Response::Committed)) => return Ok(()),
                Ok(Some(Response::NotCommitted)) => (), // will trigger a retry
                Ok(Some(Response::Done)) => return Ok(()),
                Ok(Some(Response::Ticket { idx })) => {
                    self.ticket = Some(Ticket {
                        idx,
                        needs_minister,
                        path: path.to_owned(),
                    });
                    match self.follow_up_on_ticket(path).await {
                        Ok(_) => return Ok(()),
                        Err(_) => continue, // send the request again
                    }
                }
                Ok(Some(Response::Redirect { addr, subtree })) => {
                    self.map.insert(Ministry {
                        minister: Some(addr),
                        clerk: None,
                        subtree,
                    });
                    continue;
                }
                Ok(None) => {
                    warn!("Connection was closed ");
                    let mut updated = ministry.unwrap();
                    updated.minister = None;
                    self.map.insert(updated);
                }
                Err(e) => {
                    warn!("Error connecting: {e:?}");
                    let mut updated = ministry.unwrap();
                    updated.minister = None;
                    self.map.insert(updated);
                }
            }
        }
    }
}
