use std::path::Path;
use protocol::{ServerList, Request};
use std::net::TcpStream;

pub struct Conn {
    servers: ServerList,
    read_stream: Option<TcpStream>,
    write_stream: Option<TcpStream>,
}

impl Conn {
    pub fn from_serverlist(list: ServerList) -> Self {
        Self {
            servers: list,
            read_stream: None,
            write_stream: None,
        }
    }

    pub fn send_readonly(&mut self, req: Request) {
    }

    pub fn send_writable(&mut self, req: Request) {
    }
}
