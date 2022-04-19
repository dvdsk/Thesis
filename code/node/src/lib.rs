use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser;
use color_eyre::eyre::Result;
use multicast_discovery::{discovery, ChartBuilder};
pub use color_eyre::eyre::WrapErr;

pub mod util;

mod clerk;
mod idle;
mod minister;
mod president;

pub type Id = u64;
pub enum Role {
    Idle,
    Clerk,
    Minister,
    President,
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
    #[clap(short, long)]
    pub run: u16,
    /// Optional, if not specified the node picks a random free port
    #[clap(short, long, default_value = "0")]
    pub port: u16,
    /// number of nodes in the cluster, must be fixed
    #[clap(short, long)]
    pub cluster_size: u16,
    /// database path, change when running multiple instances on
    /// the same machine
    #[clap(short, long, default_value = "database")]
    pub database: PathBuf
}

pub async fn run(conf: Config) -> Result<()> {
    let (pres_socket, pres_port) = util::open_socket(conf.port)?;
    let (mut node_socket, node_port) = util::open_socket(conf.port)?;

    let mut chart = ChartBuilder::new()
        .with_id(conf.id)
        .with_service_ports([pres_port, node_port])
        .finish()?;
    tokio::spawn(discovery::maintain(chart.clone()));
    discovery::found_majority(&chart, conf.cluster_size).await;

    let db = sled::open(conf.database).unwrap();
    let mut pres_orders = president::Log::open(db, pres_socket)?;
    let mut role = Role::Idle;
    loop {
        role = match role {
            Role::Idle => idle::work(&mut pres_orders, &mut node_socket).await,
            Role::Clerk => clerk::work(&mut pres_orders, &mut node_socket),
            Role::Minister => minister::work(&mut pres_orders, &mut node_socket),
            Role::President => president::work(&mut chart),
        }
    }
}
