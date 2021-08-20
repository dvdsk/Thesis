use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use std::net::SocketAddr;

#[derive(Serialize, Deserialize)]
pub struct ServerList {
    /// write server a client should contact
    pub write_serv: SocketAddr,
    /// read server a client should contact
    pub read_serv: SocketAddr,
    /// fallback adresses in case both write and read server are down
    pub fallback: Vec<SocketAddr>,
}

#[derive(Serialize, Deserialize)]
pub enum Existence {
    Needed,
    Allowed,
    Forbidden,
}

#[derive(Serialize, Deserialize)]
pub enum Request {
    GetAssignedServers(ServerList),
    OpenReadOnly(PathBuf, Existence),
    OpenReadWrite(PathBuf, Existence),
    Truncate(PathBuf),
}

macro_rules! from_bytes {
    ($Enum:ident) => {
        impl From<&[u8]> for $Enum {
            fn from(buf: &[u8]) -> Self {
                bincode::deserialize(buf).unwrap()
            }
        }
    };
}

from_bytes!(Request);
from_bytes!(Response);

#[derive(Serialize, Deserialize)]
pub enum Response {
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
