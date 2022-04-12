//! Simple lightweight local service discovery for testing
//!
//! This crate provides a lightweight alternative to mDNS. It discovers other instances on the
//! same machine or network. You provide an Id and Port you wish to be contacted on. Multicast-discovery
//! then gives you a live updating chart of all the discovered Ids, Ports pairs and their adress.
//!
//! ## Usage
//!
//! Add a dependency on `multicast-discovery` in `Cargo.toml`:
//!
//! ```toml
//! multicast-discovery = "0.1"
//! ```
//!
//! Now add the following snippet somewhere in your codebase. Discovery will stop when you drop the
//! maintain future.
//!
//! ```rust
//!use multicast_discovery::{discovery, ChartBuilder};
//!
//!#[tokio::main]
//!async fn main() {
//!   let chart = ChartBuilder::new()
//!       .with_id(1)
//!       .with_service_port(8042)
//!       .build()
//!       .unwrap();
//!   let maintain = discovery::maintain(chart.clone());
//!   let _ = tokio::spawn(maintain); // maintain task will run forever
//! }
//! ```
//!
//!

mod chart;
pub mod discovery;
use std::io;

pub use chart::{Chart, ChartBuilder};
type Id = u64;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Error setting up bare socket")]
    Construct(io::Error),
    #[error("Error not set Reuse flag on the socket")]
    SetReuse(io::Error),
    #[error("Error not set Broadcast flag on the socket")]
    SetBroadcast(io::Error),
    #[error("Error not set Multicast flag on the socket")]
    SetMulticast(io::Error),
    #[error("Error not set NonBlocking flag on the socket")]
    SetNonBlocking(io::Error),
    #[error("Error binding to socket")]
    Bind(io::Error),
    #[error("Error joining multicast network")]
    JoinMulticast(io::Error),
    #[error("Error transforming to async socket")]
    ToTokio(io::Error),
}

