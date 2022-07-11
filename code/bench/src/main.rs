use clap::Parser;

mod bench;
mod deploy;
mod sync;

use bench::Bench;
use color_eyre::Result;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    command: bench::Command,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let bench = Bench::from(&args.command);
    let nodes = deploy::reserve(bench.needed_nodes())?;
    deploy::start_cluster(args.command, &nodes[0..bench.fs_nodes()])?;

    let server = sync::start_server(bench.client_nodes());
    deploy::start_clients(&nodes[bench.fs_nodes()..])?;
    server.block_till_synced();

    Ok(())
}
