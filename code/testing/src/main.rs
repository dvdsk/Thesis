use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use color_eyre::eyre::Result;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use mktemp::Temp;
use node::util::runtime_dir;
use node::{Config, Id};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{self, JoinHandle};

use node::Partition;
use node::util;
use tokio::time::sleep;
use tracing::log::error;
use tracing::warn;

mod action;

type Task = task::JoinHandle<()>;

/// run number 4242 indicates resurrected/added node
fn setup_node(id: u64, run_number: u16, parts: Vec<Partition>) -> Result<Task> {
    let config = Config {
        id: 0,
        endpoint: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        run: run_number,
        local_instances: true,
        pres_port: None,
        minister_port: None,
        client_port: None,
        cluster_size: 4,
        database: PathBuf::from("changed in loop"),
        partitions: parts,
    };

    let temp_dir = Temp::new_dir().unwrap();
    let config = Config {
        id,
        database: temp_dir.join(format!("{id}.db")),
        ..config
    };
    let fut = node::run(config);
    let task = task::Builder::new().name("node").spawn(fut);
    Ok(task)
}

async fn local_cluster(parts: &[Partition]) -> Result<HashMap<u64, Task>> {
    let run_number = util::run_number(&runtime_dir());
    let n_subjects = parts.iter().map(|p| 1 + p.clerks).sum::<usize>().max(4) as u64;
    let nodes = (0..(n_subjects + 1))
        .map(|id| setup_node(id, run_number, parts.to_vec()))
        .map(Result::unwrap)
        .fold(HashMap::new(), |mut set, task| {
            let id = set.len() as u64;
            set.insert(id, task);
            set
        });
    Ok(nodes)
}

async fn perform_command(
    id: Id,
    command: char,
    cluster: &mut HashMap<u64, JoinHandle<()>>,
    parts: &Vec<Partition>,
) {
    match command as char {
        'k' => {
            let task = cluster.remove(&id).unwrap();
            if task.is_finished() {
                let panic = task.await.unwrap_err().into_panic();
                error!(
                    "task paniced before it could be aborted [by remote request]\npanic: {panic:?}"
                );
            } else {
                task.abort();
                task.await.unwrap_err().is_cancelled();
                warn!("killed node {id} [by remote request]");
            }
        }
        'r' => {
            // `ressurrect` a node
            let node = setup_node(id, 4242, parts.clone()).unwrap();
            cluster.insert(id as u64, node);
            warn!("resurrected node {id} [by remote request]");
        }
        'a' => {
            // add new node
            let id = cluster.len() as u64;
            cluster.insert(id, setup_node(id, 4242, parts.clone()).unwrap());
            warn!("added node {id} [by remote request]");
        }
        _ => panic!("recieved incorrect command"),
    }
}

async fn manage_cluster(parts: Vec<Partition>) {
    let _guard = setup_flame_tracing();
    let mut cluster = local_cluster(&parts).await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:4242").await.unwrap();
    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        loop {
            let mut buf = [0u8; 2];
            {
                let mut nodes: FuturesUnordered<_> = cluster.iter_mut().map(|(_, h)| h).collect();
                tokio::select! {
                    down = nodes.next() => panic!("node went down: {down:?}"),
                    res = socket.read_exact(&mut buf) => {res.unwrap();}
                }
            }
            socket.read_exact(&mut buf).await.unwrap();
            let [command, id] = buf;
            if command as char == 's' {
                return; // do shutdown
            }
            perform_command(id as u64, command as char, &mut cluster, &parts).await;
        }
    }
}

async fn cluster_action(conn: &mut TcpStream, buffer: String) {
    let command = buffer.chars().next().unwrap();
    let id: u8 = buffer.trim()[1..].parse().unwrap();
    let msg = [command as u8, id];
    conn.write_all(&msg).await.unwrap();
}

async fn control_cluster() -> Result<()> {
    setup_tracing();
    let nodes = client::ChartNodes::<3, 2>::new(8080);
    let mut client = client::Client::new(nodes);

    println!("waiting for connectin to cluster");
    let mut conn = loop {
        if let Ok(conn) = TcpStream::connect("127.0.0.1:4242").await {
            break conn;
        }
        sleep(Duration::from_millis(100)).await;
    };

    loop {
        println!(
            "\nsend a command to the cluster manager
    s0:    shutdown cluster
    k<id>: kill node with the given id
    r<id>: ressurect the node with id (must be killed first!)
    a<id>: add a new node with id (make sure its a unique id!)
    
    interact with the cluster [all form the same client]
    ls <path>: list files in this dir and any subdir
    touch <path>: makes a new file on the cluster
    read <path> <bytes>: (simulate) reading, use `_` for readability
    write <path> <bytes>: (simulate) writing, use `_` for readability

    test the cluster using many clients
    bench_leases <#readers> <#writers>: sim. read and write from many clients
    bench_meta <#creaters> <#listers>: sim. creating and listing from many clients
    colliding_writers <#writers> <#row_len>: sim. many writers on the same file using range API
        "
        );

        let mut buffer = String::new();
        let stdin = std::io::stdin(); // We get `Stdin` here.
        stdin.read_line(&mut buffer)?;
        if buffer.contains('_') {
            action::bench(&mut client, buffer).await?;
        } else if buffer.contains(' ') {
            action::client(&mut client, buffer).await?;
        } else {
            cluster_action(&mut conn, buffer).await;
        }
    }
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Commands {
    Remote,
    Cluster {
        /// one or more partitions, space separated.
        /// format: <path>:<number of clerks>
        partitions: Vec<Partition>,
    },
}

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install().unwrap();
    let args = Args::parse();

    println!("setting up log analyzer/collector (jeager)");
    start_jeager::start_if_not_running(runtime_dir()).await;
    println!("logging setup completed");

    match args.command {
        Commands::Cluster { partitions } => manage_cluster(partitions).await,
        Commands::Remote => control_cluster().await.unwrap(),
    }
    Ok(())
}

fn setup_tracing() {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{filter, fmt};

    let additional_filter = "";
    let base_filter = "info,instance_chart=warn";
    let filter = filter::EnvFilter::builder()
        .parse(format!("{base_filter},{additional_filter}"))
        .unwrap();

    // console_subscriber::init();
    let fmt = fmt::layer()
        .pretty()
        .with_line_number(true)
        .with_test_writer();

    let _ignore_err = tracing_subscriber::registry()
        .with(ErrorLayer::default())
        .with(filter)
        .with(fmt)
        .try_init();
}

fn setup_flame_tracing() -> impl Drop {
    use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
    // use tracing_subscriber::fmt;
    use tracing_flame::FlameLayer;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{filter, fmt};

    // let fmt_layer = fmt::Layer::default();
    let (flame_layer, _guard) = FlameLayer::with_file("./tracing.folded").unwrap();

    let filter = filter::EnvFilter::builder()
        .parse(format!("error,node=trace,instance_chart=trace"))
        .unwrap();

    let _ignore_err = tracing_subscriber::registry()
        // .with(fmt_layer)
        .with(filter)
        .with(flame_layer)
        .init();

    return _guard
}
