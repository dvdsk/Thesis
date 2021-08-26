use futures::{SinkExt, TryStreamExt};
use protocol::{connection, Request, Response};
use std::net::{IpAddr, Ipv4Addr};
use structopt::StructOpt;
use tokio::net::TcpListener;

mod db;
mod read_servers;

#[derive(Debug, StructOpt)]
enum MetaRole {
    ReadServer,
    WriteServer,
}

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(short, long)]
    client_port: u16,

    #[structopt(short, long)]
    control_port: u16,

    #[structopt(subcommand)]
    role: MetaRole,
}

async fn handle_client(mut stream: ClientStream) {
    while let Some(msg) = stream.try_next().await.unwrap() {
        let response = match msg {
            Request::Test => Response::Test,
            // Request::AddDir(path) => 
            // Request::OpenReadWrite(path, policy) => open_rw(path, policy).await,
            _e => {
                println!("TODO: responding to {:?}", _e);
                Response::Todo(_e)
            }
        };
        if let Err(_) = stream.send(response).await {
            return;
        }
    }
}

type ClientStream = connection::MsgStream<Request, Response>;
async fn write_server(port: u16) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let stream: ClientStream = connection::wrap(socket);
            handle_client(stream).await;
        });
    }
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    let db = db::Directory::new();

    write_server(opt.client_port).await;
}
