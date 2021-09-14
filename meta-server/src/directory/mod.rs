use std::time::Instant;

pub mod readserv;
pub mod writeserv;
pub mod db;

pub use db::DbError;

type ChunkId = u64;
pub struct File {
    lease: Option<Instant>,
    chunks: Vec<ChunkId>,
}
