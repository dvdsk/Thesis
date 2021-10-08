use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

pub mod connection;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerList {
    /// client port
    pub port: u16,
    /// write server a client should contact
    pub write_serv: Option<IpAddr>,
    /// read server a client should contact
    pub read_serv: Option<IpAddr>,
    /// fallback adresses in case both write and read server are down
    pub fallback: Vec<IpAddr>,
}

impl ServerList {
    pub fn random_server(&self) -> SocketAddr {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        let ip = self.fallback.choose(&mut rng).expect("no fallback servers");
        SocketAddr::from((*ip, self.port))
    }
    pub fn write_serv(&self) -> Option<SocketAddr> {
        self.write_serv.map(|ip| SocketAddr::from((ip, self.port)))
    }
    pub fn read_serv(&self) -> Option<SocketAddr> {
        self.read_serv.map(|ip| SocketAddr::from((ip, self.port)))
    }
}

pub trait Message<'de>: Serialize + Deserialize<'de> {
    fn from_buf(buf: &'de [u8]) -> Self {
        bincode::deserialize(buf).unwrap()
    }
    fn serialize_into(&self, buf: &mut [u8]) -> usize {
        bincode::serialize_into(buf, self).expect("could not serialize");
        todo!()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Existence {
    Needed,
    Allowed,
    Forbidden,
}

pub type PathString = String; // easier to serialize then Path obj
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsEntry {
    Dir(PathString),
    File(PathString),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    GetAssignedServers(ServerList),
    OpenReadOnly(PathString, Existence),
    OpenReadWrite(PathString, Existence),
    /// no writes allowed, reads can go on, read server needs no update
    OpenAppend(PathString, Existence),
    Close(PathString),
    AddDir(PathString),
    RmDir(PathString),
    Remove(PathString),
    Ls(PathString),

    Test,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Ok,
    FileExists,
    NotWriteServ(ServerList),
    NotReadServ,
    NoSuchDir,
    Ls(Vec<FsEntry>),
    Test,
    Todo(Request),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_request_size() {
        use Request::*;

        let test_cases = vec![
            (16, Truncate("test".into())),
            (24, Truncate("test/test/hi".into())),
            (20, OpenReadOnly("test".into(), Existence::Needed)),
            (25, OpenReadOnly("test/test".into(), Existence::Allowed)),
        ];

        for (size, obj) in test_cases {
            let v: Vec<u8> = bincode::serialize(&obj).unwrap();
            assert_eq!(size, v.len())
        }
    }
}
