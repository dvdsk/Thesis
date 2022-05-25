use std::net::IpAddr;
use std::num::NonZeroU16;
use std::path::PathBuf;

use clap::Parser;
use color_eyre::eyre::Result;
pub use color_eyre::eyre::WrapErr;
use instance_chart::{discovery, ChartBuilder};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

pub mod util;

mod clerk;
mod idle;
mod minister;
mod president;

pub type Id = u64;
pub type Term = u32; // raft term
pub type Idx = u32;  // raft idx

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Role {
    Idle,
    Clerk,
    Minister,
    President { term: Term },
}

/// Simple program to greet a person
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Config {
    /// Id of this node
    #[clap(short, long)]
    pub id: Id,
    /// Instrumentation endpoint
    #[clap(short, long, default_value = "127.0.0.1")]
    pub endpoint: IpAddr,
    /// Run
    #[clap(short('u'), long)]
    pub run: u16,
    /// Enable running multiple instances a the same host
    #[clap(short, long)]
    pub local_instances: bool,

    /// Optional, port on which to listen for presidential orders
    /// by default pick a free port
    #[clap(short, long)]
    pub pres_port: Option<NonZeroU16>,
    /// Optional, port on which to listen for internal communication
    /// by default pick a free port
    #[clap(short, long)]
    pub node_port: Option<NonZeroU16>,
    /// Optional, port on which to listen for client request
    /// by default pick a free port
    #[clap(short, long)]
    pub req_port: Option<NonZeroU16>,

    /// number of nodes in the cluster, must be fixed
    /// by default pick a free port
    #[clap(short, long)]
    pub cluster_size: u16,

    /// database path, change when running multiple instances on
    /// the same machine
    #[clap(short, long, default_value = "database")]
    pub database: PathBuf,
}

#[instrument(level = "info")]
pub async fn run(conf: Config) -> Result<()> {
    let (pres_listener, pres_port) = util::open_socket(conf.pres_port).await?;
    let (mut node_listener, node_port) = util::open_socket(conf.node_port).await?;
    let (mut req_listener, req_port) = util::open_socket(conf.req_port).await?;
    let mut chart = ChartBuilder::new()
        .with_id(conf.id)
        .with_service_ports([pres_port, node_port, req_port])
        .local_discovery(conf.local_instances)
        .finish()?;
    tokio::task::Builder::new().name("maintain discovery").spawn(discovery::maintain(chart.clone()));
    discovery::found_majority(&chart, conf.cluster_size).await;

    info!("opening on disk db at: {:?}", conf.database);
    let db = sled::open(conf.database).unwrap();
    let mut pres_orders =
        president::Log::open(chart.clone(), conf.cluster_size, db, pres_listener)?;

    let mut role = Role::Idle;
    loop {
        role = match role {
            Role::Idle => idle::work(&mut pres_orders).await?,
            Role::Clerk => clerk::work(&mut pres_orders, &mut node_listener),
            Role::Minister => minister::work(&mut pres_orders, &mut node_listener),
            Role::President { term } => {
                president::work(&mut pres_orders, &mut chart, &mut req_listener, term).await
            }
        }
    }
}
