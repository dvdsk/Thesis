use std::net::SocketAddr;
use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use time::OffsetDateTime;
use serde::{Deserialize, Serialize};

pub mod connection;
pub type Idx = u32;

pub trait Message<'de>: Serialize + Deserialize<'de> {
    fn from_buf(buf: &'de [u8]) -> Self {
        bincode::deserialize(buf).unwrap()
    }
    fn serialize_into(&self, buf: &mut [u8]) -> usize {
        bincode::serialize_into(buf, self).expect("could not serialize");
        todo!()
    }
}


slotmap::new_key_type! { pub struct AccessKey; }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    /// list the content of path
    List(PathBuf),
    Create(PathBuf),
    /// refresh the lease hold by the current connection
    RefreshLease,
    /// get a write lease to the file at this path
    Write{path: PathBuf, range: Range<u64>},
    /// get a write lease to the file at this path
    Read{path: PathBuf, range: Range<u64>},
    /// check if change is committed to disk, should be awnserd by Done
    /// if it is or by No if not
    IsCommitted {path: PathBuf, idx: Idx },
    /// send to a clerk by a minister needing write permissions over a file (range)
    /// the clerk will stop any ongoing reading
    Lock {path: PathBuf, range: Range<u64>, key: AccessKey },
    /// tells the clerk it can start reading again
    Unlock { path: PathBuf, key: AccessKey },
    /// unlock all file leases, send by new minister to ensure locks held under
    /// the old administration are released (since they are no longer used)
    UnlockAll,
    /// get the highest committed commit idx (used by the load balancer to find a
    /// replacement for a minister that went down)
    HighestCommited,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Lease {
    pub expires: OffsetDateTime,
    pub area: Range<u64>,
}

impl Lease {
    pub fn expires_in(&self) -> Duration {
        let dur = self.expires - OffsetDateTime::now_utc();
        if dur.is_negative() {
            Duration::from_secs(0)
        } else {
            dur.unsigned_abs()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    List(Vec<PathBuf>),
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
    /// a write lease till 
    WriteLease(Lease),
    /// a read lease till 
    ReadLease(Lease),
    /// Something went wrong
    Error(String),
    /// lease timed out or we canceld it by sending another request
    LeaseDropped,
    /// Highest commit index
    HighestCommited(Idx),
}
