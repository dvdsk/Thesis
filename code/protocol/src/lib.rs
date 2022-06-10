use std::net::SocketAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod connection;
pub type Idx = u64;

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
pub enum Request {
    CreateFile(PathBuf),
    /// check if change is committed to disk, should be awnserd by Done
    /// if it is or by No if not
    IsCommitted {path: PathBuf, idx: Idx },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// wrong subtree redirect client to correct clerk/minister
    Redirect { subtree: PathBuf, addr: SocketAddr },
    /// change not yet done, starting comit with index
    Ticket { idx: Idx },
    /// affirming awnser to `Request::IsCommitted`
    Committed,
    /// negative awnser to `Request::IsCommitted`
    NotCommitted,
    /// change committed to disk
    Done,
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn serialized_request_size() {
//         use Request::*;

//         let test_cases = vec![
//             (20, OpenReadOnly("test".into(), Existence::Needed)),
//             (25, OpenReadOnly("test/test".into(), Existence::Allowed)),
//         ];

//         for (size, obj) in test_cases {
//             let v: Vec = bincode::serialize(&obj).unwrap();
//             assert_eq!(size, v.len())
//         }
//     }
// }
