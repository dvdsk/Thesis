use std::time::Instant;

pub mod readserv;
pub mod writeserv;
pub mod db;

pub use db::DbError;

// use crate::consensus::HB_TIMEOUT;

type ChunkId = u64;
pub struct File {
    _lease: Option<Instant>,
    _chunks: Vec<ChunkId>,
}
