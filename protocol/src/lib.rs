use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

pub mod connection;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerList {
    /// write server a client should contact
    pub write_serv: SocketAddr,
    /// read server a client should contact
    pub read_serv: SocketAddr,
    /// fallback adresses in case both write and read server are down
    pub fallback: Vec<SocketAddr>,
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
    Remove(PathString),
    Ls(PathString),

    Test,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Ok,
    FileExists,
    NotWriteServ(ServerList),
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
