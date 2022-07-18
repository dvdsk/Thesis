use std::{net::{SocketAddr, ToSocketAddrs}, time::Duration};

use bench::Bench;
use benchmark::{bench, deploy, sync};
use clap::Parser;
use client::RandomNode;
use color_eyre::{eyre::eyre, eyre::WrapErr, Help, Report, Result, SectionExt};
use futures::{
    stream::{FuturesUnordered, StreamExt},
    Future,
};
use rand::prelude::*;
use tokio::time::sleep;
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

struct NodesList {
    list: Vec<SocketAddr>,
}

impl NodesList {
    fn from_hostnames(names: &[String], port: u16) -> Result<Self> {
        let mut list: Vec<SocketAddr> = Vec::new();
        for host_name in names {
            let addr: SocketAddr = format!("{host_name}:{port}")
                .to_socket_addrs()
                .wrap_err("Could not resolve host name")
                .with_section(|| host_name.to_owned().header("hostname:"))?
                .next()
                .ok_or_else(|| {
                    eyre!("Did not resolve to any adress")
                        .with_section(|| host_name.to_owned().header("hostname:"))
                })?;
            list.push(addr);
        }
        Ok(Self { list })
    }
}

#[async_trait::async_trait]
impl RandomNode for NodesList {
    async fn random_node(&self) -> SocketAddr {
        let mut rng = rand::thread_rng();
        self.list.iter().choose(&mut rng).unwrap().clone()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();
    color_eyre::install().unwrap();
    let (pres_port, min_port, client_port) = (34784, 3987, 3978);

    let args = Args::parse();
    let bench = Bench::from(&args.command);
    let nodes = deploy::reserve(bench.needed_nodes())?;
    let mut cluster = deploy::start_cluster(
        &bench,
        &nodes[0..bench.fs_nodes()],
        pres_port,
        min_port,
        client_port,
    )?;

    let find_nodes = NodesList::from_hostnames(&nodes, client_port)?;
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
    // workaround to ensure create is done by all clerks before 
    // we start the benchmark
    sleep(Duration::from_millis(200)).await;

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
        .parse("info,instance_chart=warn,client=error,benchmark=debug")
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
