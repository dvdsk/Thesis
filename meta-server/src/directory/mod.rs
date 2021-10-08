use std::time::{Duration, Instant};

pub mod readserv;
pub mod writeserv;
pub mod db;

pub use db::DbError;

use crate::consensus::HB_TIMEOUT;

const LEASE_DUR: Duration = HB_TIMEOUT;

type ChunkId = u64;
pub struct File {
    lease: Option<Instant>,
    chunks: Vec<ChunkId>,
}
