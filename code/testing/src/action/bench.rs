use std::{path::PathBuf, str::FromStr, time::Duration};

use tokio::time::sleep;
use tracing::instrument;

use super::Client;

#[instrument(skip(client))]
async fn prep_dir(client: &mut Client, readers: usize, writers: usize, spread: usize) {
    let n_files = readers.max(writers);

    for id in 0..n_files {
        let dir = id % spread;
        let path = PathBuf::from_str(&format!("/{dir}/{id}")).unwrap();
        client.create_file(path.clone()).await;
    }
}

async fn do_reads((id, spread, mut client): (usize, usize, Client)) {
    let mut buf = vec![0u8; 10_000];
    let dir = id % spread;
    let path = PathBuf::from_str(&format!("/{dir}/{id}")).unwrap();

    for _ in 0..2 {
        let mut file = client.open_readable(path.clone()).await;
        file.read(&mut buf).await;
    }
}

async fn do_writes((id, spread, mut client): (usize, usize, Client)) {
    let mut buf = vec![0u8; 10_000];
    let dir = id % spread;
    let path = PathBuf::from_str(&format!("/{dir}/{id}")).unwrap();

    for _ in 0..2 {
        let mut file = client.open_writeable(path.clone()).await;
        file.write(&mut buf).await;
    }
}

/// bench reading and/or writing, spreads load across `spread` directories
#[instrument(skip(client))]
pub async fn leases(client: &mut Client, readers: usize, writers: usize, spread: usize) {
    use futures::stream::{FuturesUnordered, StreamExt};

    let make_client = |id: usize| {
        let nodes = client::ChartNodes::<3, 2>::new(8080);
        (id, Client::new(nodes))
    };

    let prep = prep_dir(client, readers, writers, spread);
    let readers: Vec<_> = (0..readers).map(make_client).collect();
    let writers: Vec<_> = (0..writers).map(make_client).collect();
    let sleep = sleep(Duration::from_millis(500)); // give the clients time to map the cluster
    tokio::join!(prep, sleep);

    let readers: FuturesUnordered<_> = readers
        .into_iter()
        .map(|(id, client)| (id, spread, client))
        .map(do_reads)
        .collect();

    let writers: FuturesUnordered<_> = writers
        .into_iter()
        .map(|(id, client)| (id, spread, client))
        .map(do_writes)
        .collect();

    let read = readers.collect::<Vec<()>>();
    let write = writers.collect::<Vec<()>>();
    let (_read_res, _write_res) = tokio::join!(read, write);
}

async fn do_create((id, spread, mut client): (usize, usize, Client)) {
    let dir = id % spread;

    for i in 0..2 {
        let path = PathBuf::from_str(&format!("/{dir}/{id}_{i}")).unwrap();
        client.create_file(path.clone()).await;
    }
}

async fn do_list((id, spread, mut client): (usize, usize, Client)) {
    let dir = id % spread;
    let path = PathBuf::from_str(&format!("/{dir}")).unwrap();

    for _ in 0..2 {
        client.list(path.clone()).await;
    }
}

#[instrument]
pub async fn meta(creators: usize, listers: usize, spread: usize) {
    use futures::stream::{FuturesUnordered, StreamExt};

    let make_client = |id: usize| {
        let nodes = client::ChartNodes::<3, 2>::new(8080);
        (id, Client::new(nodes))
    };

    let creators: Vec<_> = (0..creators).map(make_client).collect();
    let listers: Vec<_> = (0..listers).map(make_client).collect();
    sleep(Duration::from_millis(500)).await; // give the clients time to map the cluster

    let creators: FuturesUnordered<_> = creators
        .into_iter()
        .map(|(id, client)| (id, spread, client))
        .map(do_create)
        .collect();

    let listers: FuturesUnordered<_> = listers
        .into_iter()
        .map(|(id, client)| (id, spread, client))
        .map(do_list)
        .collect();

    let create = creators.collect::<Vec<()>>();
    let list = listers.collect::<Vec<()>>();
    let (_create_res, _list_res) = tokio::join!(create, list);
}
