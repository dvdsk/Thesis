use futures::prelude::*;
use tokio::net::TcpListener;
use tokio_serde::formats::*;
use tokio_util::codec::{FramedRead, LengthDelimitedCodec};
use protocol::Request;
use structopt::StructOpt;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, StructOpt)]
enum MetaRole {
    ReadServer,
    WriteServer,
}

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(short, long)]
    port: u16,

    #[structopt(subcommand)]
    role: MetaRole,
}

async fn write_server(port: u16) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let length_delimited = FramedRead::new(socket, LengthDelimitedCodec::new());

        let mut deserialized_stream = tokio_serde::SymmetricallyFramed::new(
            length_delimited,
            SymmetricalBincode::<Request>::default(),
        );

        deserialized_stream = ();
        while let Some(msg) = deserialized_stream.try_next().await.unwrap() {
            dbg!(msg);
        }
    }
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    write_server(opt.port).await;
}
