use serde::Serialize;
use std::path::PathBuf;
use std::net::SocketAddr;


pub struct ServerList {
    /// write server a client should contact
    write_serv: SocketAddr,
    /// read server a client should contact
    read_serv: SocketAddr,
    /// fallback adresses in case both write and read server are down
    fallback: Vec<SocketAddr>,
}

pub enum Existence {
    Needed,
    Allowed,
    Forbidden,
}

// pub type Existing = bool;
pub enum Request {
    GetAssignedServers(ServerList),
    OpenReadOnly(PathBuf, Existence),
    OpenReadWrite(PathBuf, Existence),
    Truncate(PathBuf),
}

pub enum Response {
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
