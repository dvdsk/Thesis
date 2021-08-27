use futures::{SinkExt, TryStreamExt};
use client_protocol::{connection, Request, Response, PathString};
use std::net::{IpAddr, Ipv4Addr};
use structopt::StructOpt;
use tokio::net::TcpListener;

mod db;
use db::{Directory, DbError};

mod server_conn;
use server_conn::read::ReadServers;

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

async fn mkdir(mut directory: Directory, path: PathString) -> Response {
    match directory.mkdir(path).await {
        Ok(_) => Response::Ok,
        Err(DbError::FileExists) => Response::FileExists,
    }
}

async fn handle_client(mut stream: ClientStream, directory: Directory) {
    if let Some(msg) = stream.try_next().await.unwrap() {
        let response = match msg {
            Request::Test => Response::Test,
            Request::AddDir(path) => mkdir(directory, path).await,
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
async fn write_server(port: u16, directory: Directory) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let dir = directory.clone();
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let stream: ClientStream = connection::wrap(socket);
            handle_client(stream, dir).await;
        });
    }
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    let servers = ReadServers::new();
    let db = Directory::new(servers.clone());

    let maintain_conns = ReadServers::maintain(servers.conns.clone(), opt.control_port);
    let handle_req = write_server(opt.client_port, db);

    futures::join!(maintain_conns, handle_req);
}
