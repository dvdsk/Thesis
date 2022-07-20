use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use color_eyre::{eyre::WrapErr, Help, Result};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
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

pub async fn bench_task(notify: Arc<Notify>, bench: Bench, id: usize) -> Vec<(Instant, Instant)> {
    let nodes = client::ChartNodes::<3, 2>::new(8080);
    let mut client = client::Client::new(nodes);
    let mut buf = vec![0u8; 500_000_000]; // eat/reserve 500 mB of ram
    notify.notified().await;

    let results = bench.perform(&mut client, &mut buf).await;
    println!("id: {id}, minmax: {:?}", results.iter().minmax());
    results
}

#[derive(Serialize, Deserialize)]
struct Row {
    client: usize,
    // seconds since synchronized benchmark begin
    time_start: Vec<f32>,
    time_end: Vec<f32>,
}

impl Row {
    fn from(client: usize, data: Vec<(Instant, Instant)>, start_time: Instant) -> Self {
        let time_start = data
            .iter()
            .map(|(op_started, _)| op_started.checked_duration_since(start_time))
            .map(Option::unwrap)
            .map(|dur| dur.as_secs_f32())
            .collect();
        let time_end = data
            .iter()
            .map(|(_, op_ended)| op_ended.checked_duration_since(start_time))
            .map(Option::unwrap)
            .map(|dur| dur.as_secs_f32())
            .collect();

        Self {
            client,
            time_start,
            time_end,
        }
    }
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
    let start_time = Instant::now();

    let mut results = Vec::new();
    while let Some(res) = benchmarks.join_one().await {
        let res = res.wrap_err("benchmark ran into error")?;
        results.push(res);
    }

    let path = args.command.results_file();
    std::fs::create_dir_all(&path).wrap_err("Could not create directory for results")?;
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false)
        .from_path(path)
        .wrap_err("Could not create/overwrite results file")?;

    for (client, data) in results.into_iter().enumerate() {
        wtr.serialize(Row::from(client, data, start_time))?;
    }

    Ok(())
}
