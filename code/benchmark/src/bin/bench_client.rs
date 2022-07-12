use std::time::Duration;

use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::time::sleep;
use tokio::net::TcpStream;
use clap::Parser;

use benchmark::bench::{self, Bench};
use benchmark::sync;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    sync_server: String,
    #[clap(subcommand)]
    command: bench::Command,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let bench = Bench::from(&args.command);

    let nodes = client::ChartNodes::<3, 2>::new(8080);
    let mut client = client::Client::new(nodes);
    let mut buf = vec![0u8; 500_000_000]; // eat/reserve 500 mB of ram

    sleep(Duration::from_millis(500)).await; // let the client discover the nodes

    let mut stream = TcpStream::connect((args.sync_server, sync::PORT)).await.unwrap();
    stream.write_u8(0).await.unwrap(); // signal we are rdy
    let start_sync = stream.read_u8().await.unwrap(); // await sync start signal
    assert_eq!(start_sync, 42);
    
    bench.perform(&mut client, &mut buf).await;
}
