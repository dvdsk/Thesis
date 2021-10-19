use std::collections::HashSet;
use std::iter::FromIterator;
use std::time::Instant;

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

fn path_list(prefix: &str) -> Vec<String> {
    (0..300)
        .into_iter()
        .map(|n| format!("{}/{}", prefix, n))
        .collect()
}

async fn make_dirs(wconn: &mut WriteServer, list: Vec<String>) {
    for path in list {
        mkdir(wconn, &path).await;
    }
}

#[tokio::main]
async fn main() {
    setup_tracing();
    let list = serverlist_from_args();

    let mut wconn = WriteServer::from_serverlist(list.clone()).await.unwrap();

    let prefix = "hi";
    let start = Instant::now();
    make_dirs(&mut wconn, path_list(prefix)).await;
    println!("make dirs took: {:?}", start.elapsed());

    let mut rconn = ReadServer::from_serverlist(list).await.unwrap();

    let start = Instant::now();
    let res = ls(&mut rconn, "").await;
    println!("ls took: {:?}", start.elapsed());

    let on_server: HashSet<String> = res
        .into_iter()
        .map(|e| {
            if let FsEntry::Dir(path) = e {
                path
            } else {
                panic!("should only contain dirs")
            }
        })
        .collect();
    for correct_path in path_list(prefix).iter() {
        assert!(
            on_server.contains(correct_path),
            "path {:?} not in cluster",
            correct_path
        );
    }
}
