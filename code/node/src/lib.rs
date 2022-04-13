use color_eyre::eyre::Result;
use multicast_discovery::{discovery, ChartBuilder};
use clap::Parser;

pub mod util;

mod idle;
mod clerk;
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
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Config {
    /// Name of the person to greet
    #[clap(short, long)]
    id: Id,
    /// Number of times to greet
    #[clap(short, long, default_value = "127.0.0.1")]
    endpoint: String,
    /// Run
    #[clap(short, long)]
    run: u16,
    /// Optional, if not specified the node picks a random free port
    #[clap(short, long, default_value = "0")]
    port: u16,
    /// number of nodes in the cluster, must be fixed
    #[clap(short, long)]
    cluster_size: u16,
}

pub async fn run(conf: Config) -> Result<()> {
    util::setup_tracing(conf.id.to_string(), &conf.endpoint, conf.run);
    util::setup_errors();

    let (mut socket, port) = util::open_socket(conf.port)?;
    let mut chart = ChartBuilder::new().with_id(conf.id).with_service_port(port).build()?;
    tokio::spawn(discovery::maintain(chart.clone()));
    discovery::found_majority(&chart, conf.cluster_size).await;

    let db = sled::open("database").unwrap();
    let mut pres_orders = president::Log::open(db)?;
    let mut role = Role::Idle;
    loop {
        role = match role {
            Role::Idle => idle::work(&mut pres_orders, &mut socket),
            Role::Clerk => clerk::work(&mut pres_orders, &mut socket),
            Role::Minister => minister::work(&mut pres_orders, &mut socket),
            Role::President => president::work(&mut chart),
        }
    }
}
