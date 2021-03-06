use std::net::IpAddr;
use std::num::NonZeroU16;
use std::path::PathBuf;

use clap::Parser;
pub use color_eyre::eyre::WrapErr;
use instance_chart::{discovery, ChartBuilder};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::instrument;

pub mod util;

mod directory;
mod idle;
mod raft;
mod redirectory;

mod clerk;
mod minister;
mod president;

pub type Id = u64;
pub type Term = u32; // raft term
pub type Idx = u32; // raft idx

use instance_chart::Chart as mChart;

pub use self::president::load_balancing::{self, Partition};
use self::redirectory::{Node, ReDirectory};
use self::util::open_socket;
type Chart = mChart<3, u16>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    Idle,
    Clerk {
        subtree: PathBuf,
    },
    Minister {
        subtree: PathBuf,
        clerks: Vec<Node>,
        term: Term,
    },
    President {
        term: Term,
    },
}

impl Role {
    fn subtree(&self) -> Option<PathBuf> {
        match self {
            Role::Idle => None,
            Role::Clerk { subtree, .. } => Some(subtree.clone()),
            Role::Minister { subtree, .. } => Some(subtree.clone()),
            Role::President { .. } => None,
        }
    }
}

/// Simple program to greet a person
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Config {
    /// Id of this node
    #[clap(short, long)]
    pub id: Id,
    /// Instrumentation endpoint, when running locally 127.0.0.1;
    #[clap(short, long)]
    pub endpoint: Option<IpAddr>,
    /// Run number (passed to opentelemetry backend)
    #[clap(short('u'), long)]
    pub run: u16,
    /// Enable running multiple instances a the same host
    #[clap(short, long)]
    pub local_instances: bool,

    /// Optional, port on which to listen for presidential orders
    /// by default pick a free port
    #[clap(long)]
    pub pres_port: Option<NonZeroU16>,
    /// Optional, port on which to listen for internal communication
    /// by default pick a free port
    #[clap(long)]
    pub minister_port: Option<NonZeroU16>,
    /// Optional, port on which to listen for client request
    /// by default pick a free port
    #[clap(long)]
    pub client_port: Option<NonZeroU16>,

    /// number of nodes in the cluster, must be fixed
    /// by default pick a free port
    /// Minimum is 4 (1 president, 1 minister, 2 clerks)
    #[clap(short, long)]
    pub cluster_size: u16,

    /// use static subtree partitions. One of the partitions must be 
    /// the root partition
    #[clap(short, long = "partition")]
    pub partitions: Vec<Partition>,

    /// database path, change when running multiple instances on
    /// the same machine
    #[clap(short, long, default_value = "database")]
    pub database: PathBuf,
}

struct State {
    pres_orders: raft::Log<president::Order>,
    min_orders: raft::ObserverLog<minister::Order>,
    client_listener: TcpListener,
    redirectory: ReDirectory,
    partitions: Vec<Partition>,
    id: Id,
    db: sled::Db,
}

#[instrument(level = "info")]
pub async fn run(conf: Config) {
    assert!(conf.cluster_size > 3, "minimum cluster size is 4");
    assert!(
        load_balancing::has_root(&conf.partitions),
        "one of the partitions has to be root"
    );

    let (pres_listener, pres_port) = open_socket(conf.pres_port).await.unwrap();
    let (minister_listener, minister_port) = open_socket(conf.minister_port).await.unwrap();
    let (client_listener, client_port) = open_socket(conf.client_port).await.unwrap();

    let mut chart = ChartBuilder::new()
        .with_id(conf.id)
        .with_service_ports([pres_port, minister_port, client_port])
        .local_discovery(conf.local_instances)
        .finish()
        .unwrap();
    tokio::task::Builder::new()
        .name("maintain discovery")
        .spawn(discovery::maintain(chart.clone()));
    discovery::found_majority(&chart, conf.cluster_size).await;

    let db = util::open_db(&conf.database).unwrap();
    let tree = db.open_tree("president log").unwrap();
    let pres_orders =
        raft::Log::open(chart.clone(), conf.cluster_size, tree, pres_listener).unwrap();
    let redirectory = ReDirectory::from_committed(&pres_orders.state);

    let tree = db.open_tree("minister log").unwrap();
    let min_orders = raft::ObserverLog::open(tree, minister_listener).unwrap();

    let mut state = State {
        pres_orders,
        min_orders,
        client_listener,
        redirectory,
        partitions: conf.partitions,
        id: conf.id,
        db,
    };

    let mut role = Role::Idle;
    let mut prev_subtree = None;
    loop {
        match (&prev_subtree, role.subtree()) {
            (Some(prev), Some(new)) if new == *prev => (),
            (Some(_), Some(new)) => {
                prev_subtree = Some(new);
                state.min_orders.reset();
            }
            _ => (),
        }

        role = match role {
            Role::Idle => idle::work(&mut state).await.unwrap(),
            Role::Clerk { subtree, .. } => clerk::work(&mut state, subtree).await.unwrap(),
            Role::Minister {
                subtree,
                clerks,
                term,
            } => minister::work(&mut state, subtree, clerks, term)
                .await
                .unwrap(),
            Role::President { term } => president::work(&mut state, &mut chart, term).await,
        }
    }
}
