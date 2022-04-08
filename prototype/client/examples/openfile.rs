use client::Existence;
use client::{Conn, ReadServer, ServerList, WriteServer, WriteableFile};

fn serverlist_from_args() -> ServerList {
    let mut args = std::env::args();
    let port = args.nth(1).unwrap().parse().unwrap();
    ServerList {
        port,
        write_serv: None,
        read_serv: None,
        fallback: args.map(|a| a.parse().unwrap()).collect(),
    }
}

fn setup_tracing() {
    use tracing_subscriber::FmtSubscriber;
    let _subscriber = FmtSubscriber::builder().try_init().unwrap();
}

#[tokio::main]
async fn main() {
    setup_tracing();
    let list = serverlist_from_args();

    let wconn = WriteServer::from_serverlist(list.clone()).await.unwrap();
    WriteableFile::open(wconn, "does_not_exists/home", Existence::Needed).await.unwrap();
}
