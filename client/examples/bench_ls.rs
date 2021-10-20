use std::time::{Duration, Instant};

use client::{ls, mkdir, Conn, ReadServer, ServerList, WriteServer};
use tokio::time::sleep;

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

async fn make_list(wconn: &mut WriteServer, prefix: &str) {
    for numb in 0..20 {
        let path = format!("{}/{}", prefix, numb);
        mkdir(wconn, &path).await;
    }
}

async fn bench_run(concurrent_conns: usize, numb_calls: usize, list: &ServerList, prefix: &String) {
    let start = Instant::now();
    let mut read_servers = list
        .fallback
        .iter()
        .filter(|a| **a != list.write_serv.expect("ws has to be known for this bench"))
        .cloned()
        .cycle();
    let tasks = (0..concurrent_conns).into_iter().map(|_| {
        let mut list = list.clone();
        let prefix = prefix.clone();
        list.read_serv = read_servers.next();
        tokio::spawn(async move {
            let mut rconn = ReadServer::from_serverlist(list).await.unwrap();
            for _ in 0..numb_calls {
                let _ = ls(&mut rconn, &prefix).await;
            }
        })
    });
    futures::future::join_all(tasks).await;
    println!(
        "conns: {}, elapsed: {:?}",
        concurrent_conns,
        start.elapsed()
    );
}

#[tokio::main]
async fn main() {
    println!("bench ls started");
    setup_tracing();
    let list = serverlist_from_args();

    let mut wconn = WriteServer::from_serverlist(list).await.unwrap();

    use rand::{distributions::Alphanumeric, Rng};
    let prefix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();
    make_list(&mut wconn, &prefix).await;
    tracing::info!("made list");

    let WriteServer { list, .. } = wconn;
    const MAXCONNS: f32 = 4096.;
    for i in 1..=(MAXCONNS.log2() as u32) {
        bench_run(2usize.pow(i), 100, &list, &prefix).await;
        sleep(Duration::from_secs(2)).await;
    }
    tracing::info!("benchmarks done");
}
