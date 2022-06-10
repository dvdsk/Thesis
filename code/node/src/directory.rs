#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{PathBuf, Path};
use std::sync::Arc;

use derivative::Derivative;
use instance_chart::Id;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::president:: Order;
use crate::raft::State;
use crate::Term;

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
}

#[derive(Derivative, Debug, Clone, Serialize, Deserialize)]
#[derivative(Hash, Eq, PartialEq)]
pub struct Node {
    pub id: Id,
    #[derivative(Hash = "ignore", PartialEq = "ignore")]
    pub addr: SocketAddr,
}

impl Node {
    pub fn local(id: Id) -> Self {
        Self {
            id,
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
        }
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

// TODO iedereen moet een redirectory
impl ReDirectory {
    pub fn from_committed(state: &State) -> Self {
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

    pub async fn to_staff(&self, path: &Path) -> (Staff, PathBuf) {
        let tree = self.trees.read().await;
        for (subtree, staff) in tree.iter().rev() {
            if path.starts_with(subtree) {
                return (staff.clone(), subtree.clone())
            }
        }
        panic!("no root directory found or path without root")
    }

    pub(crate) fn set_tree(&mut self, tree: Option<PathBuf>) {
        self.our_tree = tree;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashSet;
    use std::net::SocketAddr;
    use std::str::FromStr;

    fn test_cleck() {
        let set: HashSet<_> = [Node {
            id: 3,
            addr: SocketAddr::from_str("127.0.0.1:42").unwrap(),
        }]
        .into_iter()
        .collect();

        assert!(set.contains(&Node {
            id: 3,
            addr: SocketAddr::from_str("192.168.1.4:22").unwrap()
        }))
    }
}
