use std::hash;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use derivative::Derivative;
use instance_chart::Id;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::instrument;

use crate::president::Order;
use crate::raft::State;
use crate::{Chart, Term};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Staff {
    pub minister: Node,
    pub clerks: Vec<Node>,
    pub term: Term,
}

impl Staff {
    pub fn len(&self) -> usize {
        self.clerks.len() + 1
    }

    pub(crate) fn for_client(self) -> protocol::Staff {
        protocol::Staff {
            minister: Some(self.minister.client_addr().untyped()),
            clerks: self
                .clerks
                .into_iter()
                .map(|n| n.client_addr().untyped())
                .collect(),
        }
    }
}

#[derive(Derivative, Debug, Clone, Serialize, Deserialize)]
#[derivative(Hash, Eq, PartialEq)]
pub struct Node {
    pub id: Id,
    #[derivative(Hash = "ignore", PartialEq = "ignore")]
    ip: IpAddr,
    #[derivative(Hash = "ignore", PartialEq = "ignore")]
    client_port: u16,
    #[derivative(Hash = "ignore", PartialEq = "ignore")]
    minister_port: u16,
    #[derivative(Hash = "ignore", PartialEq = "ignore")]
    president_port: u16,
}

pub trait Addr: Into<SocketAddr> + hash::Hash + Clone + PartialEq + PartialOrd + Eq {}

macro_rules! addr_type {
    ($name:ident, $port:ident) => {
        #[derive(Debug, Clone, Hash, Eq, PartialEq, Copy)]
        pub struct $name(pub SocketAddr);

        impl $name {
            fn new(ip: IpAddr, port: u16) -> Self {
                $name(SocketAddr::new(ip, port))
            }
            pub fn untyped(self) -> SocketAddr {
                self.0
            }
            pub fn as_untyped(&self) -> &SocketAddr {
                &self.0
            }
        }

        impl From<Node> for $name {
            fn from(node: Node) -> Self {
                $name::new(node.ip, node.$port)
            }
        }
    };
}

addr_type!(ClientAddr, client_port);
addr_type!(MinisterAddr, minister_port);
addr_type!(PresidentAddr, president_port);

impl Node {
    pub fn from_chart(id: Id, chart: &Chart) -> Self {
        let addresses = chart.get_addr_list(id).expect("id not in chart");
        let ip = addresses[0].ip();
        let [president_port, minister_port, client_port] = addresses.map(|addr| addr.port());
        Self {
            id,
            ip,
            client_port,
            minister_port,
            president_port,
        }
    }
    pub fn client_addr(&self) -> ClientAddr {
        ClientAddr::new(self.ip, self.client_port)
    }
    pub fn minister_addr(&self) -> MinisterAddr {
        MinisterAddr::new(self.ip, self.minister_port)
    }
    pub fn president_addr(&self) -> PresidentAddr {
        PresidentAddr::new(self.ip, self.president_port)
    }
}

/// keep an up to date map of the ministies to redirect clients
/// to there right address given their subtree
#[derive(Default, Clone)]
pub struct ReDirectory {
    our_tree: Option<PathBuf>,
    trees: Arc<RwLock<Vec<(PathBuf, Staff)>>>,
}

fn insert_sorted(trees: &mut Vec<(PathBuf, Staff)>, path: PathBuf, staff: Staff) {
    let res = trees.binary_search_by_key(&path, |(subtree, _)| subtree.clone());
    let idx = match res {
        Ok(idx) => idx,
        Err(idx) => idx,
    };
    trees.insert(idx, (path, staff));
}

impl ReDirectory {
    pub fn from_committed(state: &State<Order>) -> Self {
        let mut trees = Vec::new();
        for order in state.committed() {
            if let Order::AssignMinistry { subtree, staff } = order {
                insert_sorted(&mut trees, subtree, staff);
            }
        }

        Self {
            our_tree: None,
            trees: Arc::new(RwLock::new(trees)),
        }
    }

    pub async fn update(&mut self, order: &Order) {
        if let Order::AssignMinistry { subtree, staff } = order {
            let mut trees = self.trees.write().await;
            insert_sorted(&mut trees, subtree.clone(), staff.clone());
        }
    }

    #[instrument(level = "debug", skip(self), ret)]
    pub async fn to_staff(&self, path: &Path) -> (Staff, PathBuf) {
        let tree = self.trees.read().await;
        for (subtree, staff) in tree.iter().rev() {
            if path.starts_with(subtree) {
                return (staff.clone(), subtree.clone());
            }
        }
        if !path.starts_with("/") {
            panic!("path without root: {path:?}");
        } else {
            panic!("no root directory: {:?}", *tree);
        }
    }

    pub(crate) fn set_tree(&mut self, tree: Option<PathBuf>) {
        self.our_tree = tree;
    }

    pub(crate) async fn subtrees(&self, path: &PathBuf) -> Vec<PathBuf> {
        let tree = self.trees.read().await;
        tree.iter()
            .map(|(path, _)| path)
            .filter(|tree| tree.starts_with(path))
            .filter(|tree| *tree != path)
            .cloned()
            .collect()
    }
}
