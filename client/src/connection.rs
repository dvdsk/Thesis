use protocol::{Request, Response, Message, ServerList};
use protocol::connection;
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

fn send_recieve(req: &Request, stream: &mut TcpStream) -> Result<Response, io::Error> {
    let mut buf = [0u8; 512];
    let len = req.serialize_into(&mut buf[2..]);
    buf[0..2].copy_from_slice(&len.to_ne_bytes());
    stream.write_all(&[0])?;

    let len = msg_len(stream, &mut buf[0..2])?;
    stream.read_exact(&mut buf[0..len])?;
    Ok(Response::from_buf(&buf[0..len]))
}

#[derive(thiserror::Error, Debug)]
pub enum ConnError {
    #[error("Could not contact metadata server")]
    IoError(#[from] std::io::Error),
}

pub trait Conn: Sized {
    fn from_serverlist(list: ServerList) -> Result<Self, ConnError>;
    fn re_connect(&mut self) -> Result<(), ConnError>;
    fn get_stream_mut(&mut self) -> &mut TcpStream;

    fn send(&mut self, req: Request) -> Result<Response, ConnError> {
        loop {
            use io::ErrorKind::*;
            let stream = self.get_stream_mut();
            let res = send_recieve(&req, stream);
            match res {
                Ok(resp) => return Ok(resp),
                Err(e) => match e.kind() {
                    ConnectionReset | ConnectionAborted | ConnectionRefused => (),
                    _ => return Err(e.into()),
                }
            }
            self.re_connect()?;
        }
    }
}

pub struct WriteServer {
    list: ServerList,
    stream: TcpStream,
}

impl WriteServer {
fn connect(list: &ServerList) -> Result<TcpStream, ConnError> {
        let stream = TcpStream::connect(list.write_serv)?; 
        let msg_stream = connection::wrap(stream);
        Ok(stream)
    }
}

impl Conn for WriteServer {
    fn from_serverlist(list: ServerList) -> Result<Self, ConnError> {
        let stream = Self::connect(&list)?;
        Ok(Self { list, stream })
    }

    fn re_connect(&mut self) -> Result<(), ConnError> {
        self.stream = Self::connect(&self.list)?;
        Ok(())
    }

    fn get_stream_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }
}

pub struct ReadServer {
    list: ServerList,
    stream: TcpStream,
}

impl ReadServer {
fn connect(list: &ServerList) -> Result<TcpStream, ConnError> {
        Ok(TcpStream::connect(list.read_serv)?)
    }
}

impl Conn for ReadServer {
    fn from_serverlist(list: ServerList) -> Result<Self, ConnError> {
        let stream = Self::connect(&list)?;
        Ok(Self { list, stream })
    }

    fn re_connect(&mut self) -> Result<(), ConnError> {
        self.stream = Self::connect(&self.list)?;
        Ok(())
    }

    fn get_stream_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }
}
