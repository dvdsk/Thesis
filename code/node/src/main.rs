use clap::Parser;
use color_eyre::eyre::Result;
use multicast_discovery::{discovery, ChartBuilder};

mod util;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[clap(short, long)]
    id: u64,
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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    util::setup_tracing(args.id.to_string(), &args.endpoint, args.run);
    util::setup_errors();

    let (socket, port) = util::open_socket(args.port)?;
    let chart = ChartBuilder::new().with_id(args.id).with_service_port(port).build()?;
    tokio::spawn(discovery::maintain(chart.clone()));
    discovery::found_majority(chart, args.cluster_size).await;

    Ok(())
}
