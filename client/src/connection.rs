use protocol::{Request, Response, ServerList};
use std::convert::TryInto;
use std::io;
use std::io::{Read, Write};
use std::net::TcpStream;

fn msg_len(stream: &mut TcpStream, buf: &mut [u8]) -> Result<usize, io::Error> {
    let buf: &mut [u8; 2] = buf.try_into().unwrap();
    stream.read_exact(buf)?;
    let len = u16::from_ne_bytes(*buf) as usize;
    Ok(len)
}

fn send_recieve(stream: &mut TcpStream) -> Result<Response, io::Error> {
    let mut buf = [0u8; 512];
    stream.write_all(&[0]).map_err(|e| e.kind())?;
    let len = msg_len(stream, &mut buf[0..2])?;
    stream.read_exact(&mut buf[0..len])?;
    Ok(Response::from(&buf[0..len]))
}

#[derive(thiserror::Error, Debug)]
pub enum ConnError {
    #[error("Could not contact metadata server")]
    IoError(#[from] std::io::Error),
}

pub trait Conn {
    fn from_serverlist(list: ServerList) -> Self;
    fn assure_connection(&mut self) -> Result<&mut TcpStream, ConnError>;
    fn set_disconnected(&mut self);
    fn send(&mut self, req: Request) -> Result<Response, ConnError> {
        loop {
            use io::ErrorKind::*;
            let stream = self.assure_connection()?;
            let res = send_recieve(stream);
            match res {
                Ok(resp) => return Ok(resp),
                Err(e) => match e.kind() {
                    ConnectionReset | ConnectionAborted | ConnectionRefused => continue,
                    _ => return Err(e.into()),
                }
            }
        }
    }
}

pub struct WriteServer {
    list: ServerList,
    stream: Option<TcpStream>,
}

impl Conn for WriteServer {
    fn from_serverlist(list: ServerList) -> Self {
        Self { list, stream: None }
    }

    fn assure_connection(&mut self) -> Result<&mut TcpStream, ConnError> {
        if self.stream.is_none() {
            self.stream = Some(TcpStream::connect(self.list.write_serv).unwrap());
        } // TODO handle unconnectable
        todo!()
    }

    fn set_disconnected(&mut self) {
        self.stream = None
    }
}

pub struct ReadServer {
    list: ServerList,
    stream: Option<TcpStream>,
}

impl Conn for ReadServer {
    fn from_serverlist(list: ServerList) -> Self {
        Self { list, stream: None }
    }

    fn assure_connection(&mut self) -> Result<&mut TcpStream, ConnError> {
        if self.stream.is_none() {
            self.stream = Some(TcpStream::connect(self.list.read_serv).unwrap());
        } // TODO handle unconnectable
        todo!()
    }
    fn set_disconnected(&mut self) {
        self.stream = None
    }
}
