use std::path::PathBuf;

use client::{Client, ChartNodes};
use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// Path where to write to
    path: PathBuf,

    /// Port on which to discover the server
    #[clap(short, long, default_value = "8080")]
    discovery_port: u16,

    /// Bytes of data to write
    bytes: usize
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    let nodes = ChartNodes::<3,2>::new(args.discovery_port);
    let mut client = Client::new(nodes);

    client.create_file(args.path.clone()).await;
    let mut file = client.open_readable(args.path).await;
    let mut buf = vec![0u8; args.bytes];
    file.read(&mut buf).await
}
