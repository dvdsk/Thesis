use clap::Parser;
use futures::{
    stream::{FuturesUnordered, StreamExt},
    Future,
};

use bench::Bench;
use benchmark::{bench, deploy, sync};

use color_eyre::{eyre::eyre, eyre::WrapErr, Help, Report, Result, SectionExt};
use tracing::{debug, info};

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    command: bench::Command,
}

async fn watch_nodes(
    cluster: &mut FuturesUnordered<impl Future<Output = Result<String, Report>>>,
    clients: &mut FuturesUnordered<impl Future<Output = Result<String, Report>>>,
) -> Result<Vec<String>> {
    let mut output = Vec::new();

    loop {
        tokio::select! {
            err = cluster.next() => {
                return match err.expect("should always be more then one node") {
                    Ok(output) => Err(eyre!("node exits early"))
                        .with_section(move || output.header("Output:")),
                    Err(err) => Err(err).wrap_err("node failed"),
                };
            }
            res = clients.next() => {
                match res {
                    Some(res) => {
                        let out = res.wrap_err("client failed")?;
                        output.push(out);
                    }
                    None => return Ok(output), // all clients are done
                }
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
    let mut cluster = deploy::start_cluster(&bench, &nodes[0..bench.fs_nodes()])?;

    // do any prep work for the benchmark (make files etc)
    /* FIX: Can not find any nodes as the clusters head node is not on the
     * same subnet/network as the servers. Should implementa node source 
     * (see client RandomNode trait) that uses the list generated above
     * to do so ports will need to be made fixed/static too <12-07-22> */
    let find_nodes = client::ChartNodes::<3, 2>::new(8080);
    let mut client = client::Client::new(find_nodes);
    tokio::select! {
        err = cluster.next() => {
            return match err.expect("should always be more then one node") {
                Ok(output) => Err(eyre!("node exits early"))
                    .with_section(move || output.header("Output:")),
                Err(err) => Err(err).wrap_err("node failed"),
            };
        }
        _ = bench.prep(&mut client) => (),
    }

    let server = sync::server(bench.client_nodes());
    let mut clients = deploy::start_clients(args.command, &nodes[bench.fs_nodes()..])?;

    tokio::select! {
        res = server.block_till_synced() => {
            info!("benchmark clients now synced");
            res?;
        }
        res = watch_nodes(&mut cluster, &mut clients) => {
            return Err(res.expect_err("benchmark can not be done before clients are synced"));
        }
    }

    let output = watch_nodes(&mut cluster, &mut clients).await?;
    info!("benchmark completed!");
    debug!("node output: {output:?}");
    Ok(())
}

fn setup_tracing() {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{filter, fmt};

    let filter = filter::EnvFilter::builder()
        .parse("info,instance_chart=trace,client=debug,benchmark=debug")
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
