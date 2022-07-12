use clap::Parser;
use futures::{
    stream::{FuturesUnordered, StreamExt},
    Future,
};

use benchmark::{bench, deploy, sync};
use bench::Bench;

use color_eyre::{eyre::eyre, eyre::WrapErr, Help, Report, Result, SectionExt};

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    command: bench::Command,
}

async fn watch_nodes(
    mut cluster: FuturesUnordered<impl Future<Output = Result<String, Report>>>,
    clients: FuturesUnordered<impl Future<Output = Result<String, Report>>>,
) -> Result<()> {
    use futures::future;
    let mut client_failures = clients.filter_map(|res| future::ready(res.err()));

    tokio::select! {
        err = cluster.next() => {
            match err.expect("should always be more then one node") {
                Ok(output) => Err(eyre!("node exits early"))
                    .with_section(move || output.header("Output:")),
                Err(e) => Err(e).wrap_err("node error"),
            }
        }
        res = client_failures.next() => {
            match res {
                Some(err) => Err(err.wrap_err("client failed")),
                None => Ok(()),
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();
    color_eyre::install().unwrap();

    let args = Args::parse();
    let bench = Bench::from(&args.command);
    let nodes = deploy::reserve(bench.needed_nodes())?;
    let cluster = deploy::start_cluster(&bench, &nodes[0..bench.fs_nodes()])?;

    // do any prep work for the benchmark (make files etc)
    let find_nodes = client::ChartNodes::<3, 2>::new(8080);
    let mut client = client::Client::new(find_nodes);
    bench.prep(&mut client).await;

    let server = sync::server(bench.client_nodes());
    let clients = deploy::start_clients(args.command, &nodes[bench.fs_nodes()..])?;

    tokio::select! {
        res = server.block_till_synced() => {
            res?;
        }
        res = watch_nodes(cluster, clients) => {
            res?;
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
