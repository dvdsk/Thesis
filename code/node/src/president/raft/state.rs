use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use tokio::sync::{mpsc, Notify};

use super::Order;
pub type Term = u32;
type LogIdx = u32;

pub mod append;
pub mod vote;

#[derive(Debug)]
struct Vars {
    last_applied: AtomicU32,
    commit_index: AtomicU32,
    heartbeat: Notify,
}

impl Default for Vars {
    fn default() -> Self {
        Self {
            last_applied: AtomicU32::new(0),
            commit_index: AtomicU32::new(0),
            heartbeat: Default::default(),
        }
    }
}

mod db {
    #[repr(u8)]
    pub enum Prefix {
        /// reserved to allow unwrap_or_default to reduce no key found Option
        /// to an unused key_prefix.
        #[allow(dead_code)]
        Invalid = 0,
        /// contains last seen term and voted_for
        ElectionData = 1,
        /// keys are 5 bytes: [this prefix, u32 as_ne_bytes],
        /// entries are 4 bytes term as_ne_bytes + byte serialized order,
        Log = 2,
    }

    impl From<&u8> for Prefix {
        fn from(prefix: &u8) -> Self {
            use Prefix::*;
            match *prefix {
                x if x == ElectionData as u8 => ElectionData,
                x if x == Log as u8 => Log,
                _ => Invalid,
            }
        }
    }
    pub const ELECTION_DATA: [u8; 1] = [Prefix::ElectionData as u8];
    #[allow(dead_code)]
    pub const LOG: [u8; 1] = [Prefix::Log as u8];
    pub fn log_key(idx: u32) -> [u8; 5] {
        let mut key = [Prefix::Log as u8, 0, 0, 0, 0];
        key[1..5].clone_from_slice(&idx.to_ne_bytes());
        key
    }
}

#[derive(Debug, Clone)]
pub struct State {
    pub id: instance_chart::Id,
    tx: mpsc::Sender<Order>,
    db: sled::Tree,
    vars: Arc<Vars>,
}

impl State {
    pub fn new(tx: mpsc::Sender<Order>, db: sled::Tree, id: instance_chart::Id) -> Self {
        let state = Self {
            id,
            tx,
            db,
            vars: Default::default(),
        };
        state.init_election_data();
        state.insert_into_log(0, &append::LogEntry::default());
        state
    }

    pub(crate) async fn watch_term(&self) -> Term {
        let mut sub = self.db.watch_prefix(db::ELECTION_DATA);
        while let Some(event) = (&mut sub).await {
            match event {
                sled::Event::Insert { key, value } if key == db::ELECTION_DATA => {
                    let data: vote::ElectionData = bincode::deserialize(&value).unwrap();
                    return data.term;
                }
                _ => panic!("term key should never be removed"),
            }
        }
        unreachable!("db subscriber should never be dropped")
    }

    pub fn heartbeat(&self) -> &Notify {
        &self.vars.heartbeat
    }
}

#[derive(Debug, Default)]
pub struct LogMeta {
    pub term: u32,
    pub idx: u32,
}

impl State {
    /// for an empty log return (0,0)
    pub(crate) fn last_log_meta(&self) -> LogMeta {
        use db::Prefix;

        let max_key = [u8::MAX];
        let (key, value) = self
            .db
            .get_lt(max_key)
            .expect("internal db issue")
            .unwrap_or_default();

        match key.get(0).map(Prefix::from).unwrap_or(Prefix::Invalid) {
            Prefix::Log => LogMeta {
                idx: u32::from_ne_bytes(key[1..5].try_into().unwrap()),
                term: u32::from_ne_bytes(value[0..4].try_into().unwrap()),
            },
            _ => Default::default(),
        }
    }

    pub(super) async fn order(&self, ord: Order) {
        self.tx.send(ord).await.unwrap();
    }
}
