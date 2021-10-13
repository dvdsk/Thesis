use async_recursion::async_recursion;
use client::FsEntry;
use client::{ls, mkdir, rmdir, Conn, ReadServer, ServerList, WriteServer};

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

const WIDTH: usize = 10;
const DEPTH: usize = 5;

#[async_recursion]
async fn make_tree(wconn: &mut WriteServer, prefix: &str, depth: usize) {
    for numb in 0..WIDTH {
        let path = format!("{}/{}", prefix, numb);
        mkdir(wconn, &path).await;
        if depth < DEPTH {
            make_tree(wconn, &path, depth+1).await;
        }
    }
}

async fn make_list(wconn: &mut WriteServer, prefix: &str) {
    for numb in 0..300 {
        let path = format!("{}/{}", prefix, numb);
        mkdir(wconn, &path).await;
    }
}

#[tokio::main]
async fn main() {
    setup_tracing();
    let list = serverlist_from_args();

    let mut wconn = WriteServer::from_serverlist(list.clone()).await.unwrap();

    let prefix = "hi";
    // make_tree(&mut wconn, prefix, 0).await;
    make_list(&mut wconn, prefix).await;

    let mut rconn = ReadServer::from_serverlist(list).await.unwrap();
    let res = ls(&mut rconn, "").await;
    dbg!(res);
}
