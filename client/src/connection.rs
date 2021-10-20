use async_trait::async_trait;
use futures::prelude::*;
use std::time::Duration;
use std::{io, thread};
use tokio::net::TcpStream;
use tracing::{info, warn};

use protocol::connection::{self, MsgStream};
use protocol::{Request, Response, ServerList};

type ClientStream = MsgStream<Response, Request>;
async fn send_recieve(req: Request, stream: &mut ClientStream) -> Result<Response, ConnError> {
    use ConnError::*;
    stream.send(req).await.map_err(RequestIo)?;

    let response = stream
    .try_next()
    .await
    .map_err(ResponseIo)?
    .ok_or(NoResponse)?;
    Ok(response)
}

#[derive(thiserror::Error, Debug)]
pub enum ConnError {
    #[error("Could not connect to any read server")]
    NoReadServers(std::io::Error),
    #[error("Could not connect to write server")]
    NoWriteServer(std::io::Error),
    #[error("IO-error while sending resuest to metadata server")]
    RequestIo(std::io::Error),
    #[error("IO-error while listening for metadata server response")]
    ResponseIo(std::io::Error),
    #[error("There was no response to the request")]
    NoResponse,
}

#[async_trait]
pub trait Conn: Sized {
    async fn from_serverlist(list: ServerList) -> Result<Self, ConnError>;
    async fn re_connect(&mut self) -> Result<(), ConnError>;
    fn get_stream_mut(&mut self) -> &mut ClientStream;
    async fn request(&mut self, req: Request) -> Result<Response, ConnError>;

    async fn basic_request(&mut self, req: Request) -> Result<Response, ConnError> {
        loop {
            use io::ErrorKind::*;
            let stream = self.get_stream_mut();
            let res = send_recieve(req.clone(), stream).await;
            match res {
                Ok(Response::Todo(req)) => {
                    panic!("server reports it can not yet handle: {:?}", req)
                }
                Ok(resp) => return Ok(resp),
                Err(ConnError::RequestIo(e)) => match e.kind() {
                    ConnectionReset | ConnectionAborted | ConnectionRefused => (),
                    _ => return Err(ConnError::RequestIo(e)),
                },
                Err(ConnError::ResponseIo(e)) => match e.kind() {
                    ConnectionReset | ConnectionAborted | ConnectionRefused => (),
                    _ => return Err(ConnError::ResponseIo(e)),
                },
                Err(e) => return Err(e),
            }
            self.re_connect().await?;
        }
    }
}

pub struct WriteServer {
    pub list: ServerList,
    stream: ClientStream,
}

impl WriteServer {
    async fn connect(list: &mut ServerList) -> Result<ClientStream, ConnError> {
        let mut addr = list
            .write_serv()
            .unwrap_or_else(|| list.random_server());
        if let Ok(stream) = TcpStream::connect(addr).await {
            list.write_serv = Some(addr.ip());
            return Ok(connection::wrap(stream));
        }

        loop {
            warn!(
                "could not connect to server on: {:?}, retrying random adress in 500ms",
                addr
            );
            addr = list.random_server();
            thread::sleep(Duration::from_millis(500));
            if let Ok(stream) = TcpStream::connect(addr).await {
                return Ok(connection::wrap(stream));
            }
        }
    }
}

#[async_trait]
impl Conn for WriteServer {
    async fn from_serverlist(mut list: ServerList) -> Result<Self, ConnError> {
        let stream = Self::connect(&mut list).await?;
        Ok(Self { list, stream })
    }

    async fn re_connect(&mut self) -> Result<(), ConnError> {
        self.stream = Self::connect(&mut self.list).await?;
        Ok(())
    }

    fn get_stream_mut(&mut self) -> &mut ClientStream {
        &mut self.stream
    }
    async fn request(&mut self, req: Request) -> Result<Response, ConnError> {
        loop {
            match self.basic_request(req.clone()).await {
                Ok(Response::NotWriteServ(new_list)) => {
                    info!("updating write server with new: {:?}", new_list);
                    self.list = new_list;
                    self.re_connect().await.unwrap();
                }
                Err(ConnError::NoWriteServer(_)) => {
                    info!("current write server unreachable");
                    self.list.write_serv = None;
                }
                _other => return _other,
            }
        }
    }
}

pub struct ReadServer {
    list: ServerList,
    stream: ClientStream,
}

impl ReadServer {
    async fn connect(list: &ServerList) -> Result<ClientStream, ConnError> {
        let mut addr = list.read_serv().unwrap_or_else(|| list.random_server());
        if let Ok(stream) = TcpStream::connect(addr).await {
            return Ok(connection::wrap(stream));
        }

        loop {
            warn!(
                "could not connect to server on: {:?}, retrying random adress in 500ms",
                addr
            );
            addr = list.random_server();
            thread::sleep(Duration::from_millis(500));
            if let Ok(stream) = TcpStream::connect(addr).await {
                return Ok(connection::wrap(stream));
            }
        }
    }
}

#[async_trait]
impl Conn for ReadServer {
    async fn from_serverlist(list: ServerList) -> Result<Self, ConnError> {
        let stream = Self::connect(&list).await?;
        Ok(Self { list, stream })
    }

    async fn re_connect(&mut self) -> Result<(), ConnError> {
        self.stream = Self::connect(&self.list).await?;
        Ok(())
    }

    fn get_stream_mut(&mut self) -> &mut ClientStream {
        &mut self.stream
    }

    async fn request(&mut self, req: Request) -> Result<Response, ConnError> {
        loop {
            match self.basic_request(req.clone()).await {
                Ok(Response::NotReadServ) => {
                    info!("not read server moving to diff adress");
                    self.list.read_serv = None;
                    self.re_connect().await.unwrap();
                }
                _other => return _other,
            }
        }
    }
}
