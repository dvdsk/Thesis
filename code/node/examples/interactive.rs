use std::net::{Ipv4Addr, IpAddr};
use std::path::PathBuf;
use std::collections::HashMap;

use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use color_eyre::eyre::Result;
use mktemp::Temp;
use node::Config;
use node::util::runtime_dir;
use tokio::task;

use node::util;
use tracing::log::error;
use tracing::warn;

type Task = task::JoinHandle<()>;

fn setup_node(id: u64) -> Result<Task> {
    let config = Config {
        id: 0,
        endpoint: IpAddr::V4(Ipv4Addr::LOCALHOST),
        run: util::run_number(&runtime_dir()),
        local_instances: true,
        pres_port: None,
        node_port: None,
        req_port: None,
        cluster_size: 3,
        database: PathBuf::from("changed in loop"),
    };

    let temp_dir = Temp::new_dir().unwrap();
    let config =  Config {
        id,
        database: temp_dir.join(format!("{id}.db")),
        ..config.clone()
    };
    let fut = node::run(config);
    let task = task::Builder::new().name("node").spawn(fut);
    Ok(task)
}

async fn local_cluster() -> Result<HashMap<u64, Task>> {
    let nodes = (0..4)
        .map(setup_node)
        .map(Result::unwrap)
        .fold(HashMap::new(), |mut set, task| {
            let id = set.len() as u64;
            set.insert(id, task);
            set
        });
    Ok(nodes)
}

async fn manage_cluster() {
    start_jeager::start_if_not_running(runtime_dir()).await;
    // console_subscriber::init();
    util::setup_errors();

    let mut cluster = local_cluster().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:4242").await.unwrap();
    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        loop {
            let mut buf = [0u8;2];
            socket.read_exact(&mut buf).await.unwrap();
            let [command, id] = buf;
            match command as char {
                'k' => {
                    let task = cluster.remove(&(id as u64)).unwrap();
                    if task.is_finished() {
                        let panic = task.await.unwrap_err().into_panic();
                        error!("task paniced before it could be aborted [by remote request]\npanic: {panic:?}");
                    } else {
                        task.abort();
                        task.await.unwrap_err().is_cancelled();
                        warn!("killed node {id} [by remote request]");
                    }
                }
                'r' => { // `ressurrect` a node
                    let node = setup_node(id as u64).unwrap();
                    cluster.insert(id as u64, node);
                    warn!("resurrected node {id} [by remote request]");
                }
                'a' => { // add new node
                    let id = cluster.len() as u64;
                    cluster.insert(id, setup_node(id).unwrap());
                    warn!("added node {id} [by remote request]");
                }
                _ => panic!("recieved incorrect command"),
            }
        }
    }
}

async fn control_cluster() -> Result<()> {
    let mut conn = TcpStream::connect("127.0.0.1:4242").await.unwrap();
    loop {
        println!("send a command to the cluster manager, <k,r,a><an id>");
        let mut buffer = String::new();
        let stdin = std::io::stdin(); // We get `Stdin` here.
        stdin.read_line(&mut buffer)?;
        let command = buffer.chars().next().unwrap();
        let id: u8 = buffer.trim()[1..].parse().unwrap();
        let msg = [command as u8, id];
        conn.write_all(&msg).await.unwrap();
    }

}

#[tokio::main]
async fn main() {
    let arg = std::env::args().nth(1);
    match arg.as_ref().map(String::as_str) {
        Some("c") => manage_cluster().await,
        Some("r") => control_cluster().await.unwrap(),
        _ => println!("pass one argument, c (cluster) or r (remote)"),
    }

}
