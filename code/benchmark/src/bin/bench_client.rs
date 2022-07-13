use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use color_eyre::{Result, eyre::WrapErr, Help};
use itertools::Itertools;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio::time::sleep;

use benchmark::bench::{self, Bench};
use benchmark::sync;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    sync_server: String,
    #[clap(subcommand)]
    command: bench::Command,
}

pub async fn bench_task(notify: Arc<Notify>, bench: Bench, id: usize) {
    let nodes = client::ChartNodes::<3, 2>::new(8080);
    let mut client = client::Client::new(nodes);
    let mut buf = vec![0u8; 500_000_000]; // eat/reserve 500 mB of ram
    notify.notified().await;

    let results = bench.perform(&mut client, &mut buf).await;
    println!("id: {id}, minmax: {:?}", results.iter().minmax());
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install().unwrap();
    let args = Args::parse();
    let bench = Bench::from(&args.command);

    let mut benchmarks = JoinSet::new();
    let notify = Arc::new(Notify::new());
    for id in 0..bench.clients_per_node {
        let task = bench_task(notify.clone(), bench.clone(), id);
        benchmarks.spawn(task);
    }
    sleep(Duration::from_millis(500)).await; // let the client discover the nodes

    let mut stream = TcpStream::connect((args.sync_server.clone(), sync::PORT))
        .await
        .wrap_err("Could not connect to sync server")
        .with_note(|| format!("sync server adress: {}", args.sync_server))?;
    stream.write_u8(0).await.unwrap(); // signal we are rdy
    let start_sync = stream.read_u8().await.unwrap(); // await sync start signal
    assert_eq!(start_sync, 42);
    notify.notify_waiters();

    while let Some(res) = benchmarks.join_one().await {
        res.wrap_err("benchmark ran into error")?;
    }

    Ok(())
}
