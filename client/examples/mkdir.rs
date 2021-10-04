use std::net::{IpAddr, SocketAddr};
use client::{Conn, ReadServer, ServerList, WriteServer, ls, mkdir};

fn from_arg(port: u16, arg: String) -> SocketAddr {
    let ip: IpAddr = arg.parse().unwrap();
    SocketAddr::from((ip, port))
}

fn serverlist_from_args() -> ServerList {
    let mut args = std::env::args();
    let port = args.nth(1).unwrap().parse().unwrap();
    ServerList {
        write_serv: Some(from_arg(port, args.next().unwrap())),
        read_serv: from_arg(port, args.next().unwrap()),
        fallback: args.map(|a| from_arg(port, a)).collect(),
    }
}

fn setup_tracing() {
    use tracing_subscriber::FmtSubscriber;
    let subscriber = FmtSubscriber::builder().try_init().unwrap();
}

#[tokio::main]
async fn main() {
    setup_tracing();
    let list = serverlist_from_args();

    let wconn = WriteServer::from_serverlist(list.clone()).await.unwrap();
    let rconn = ReadServer::from_serverlist(list).await.unwrap();

    let res = mkdir(wconn, "test_dir").await;
    dbg!("info call done");

    let res = ls(rconn, "").await;
    dbg!(res);
}
