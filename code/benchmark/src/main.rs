use clap::Parser;
use futures::stream::StreamExt;

use bench::Bench;
use benchmark::{bench, deploy, sync};
use color_eyre::Result;
use tracing::error;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    command: bench::Command,
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();
    color_eyre::install().unwrap();

    let args = Args::parse();
    let bench = Bench::from(&args.command);
    let nodes = deploy::reserve(bench.needed_nodes())?;
    let mut cluster = deploy::start_cluster(&bench, &nodes[0..bench.fs_nodes()])?;

    // do any prep work for the benchmark (make files etc)
    let find_nodes = client::ChartNodes::<3, 2>::new(8080);
    let mut client = client::Client::new(find_nodes);
    bench.prep(&mut client).await;

    let server = sync::start_server(bench.client_nodes());
    let clients = deploy::start_clients(args.command, &nodes[bench.fs_nodes()..])?;
    server.block_till_synced();

    use futures::future;
    let mut client_failures = clients.filter_map(|res| future::ready(res.err()));
    tokio::select! {
        err = cluster.next() => {
            error!("cluster node failed: {err:?}")
        }
        res = client_failures.next() => {
            if let Some(err) = res {
                error!("client failed: {err:?}")
            }
        }
    }

    Ok(())
}

fn setup_tracing() {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{filter, fmt};

    let filter = filter::EnvFilter::builder()
        .parse("info,instance_chart=warn,client=warn")
        .unwrap();

    let uptime = fmt::time::uptime();
    let fmt_layer = fmt::layer()
        .pretty()
        .with_line_number(true)
        .with_timer(uptime);

    let _ignore_err = tracing_subscriber::registry()
        .with(ErrorLayer::default())
        .with(filter)
        .with(fmt_layer)
        .try_init();
}
