use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify};
use tracing::instrument;

use crate::util::TypedSled;
use crate::Id;

use super::Order;
pub type Term = u32;
type LogIdx = u32;

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
        let mut key = [Prefix::Log as u8, 0,0,0,0];
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
        state.insert_into_log(0, &LogEntry::default());
        state
    }
    /// handle append request, this can be called in parallel.
    #[instrument(skip(self), fields(id = self.id), ret)]
    pub fn append_req(&self, req: AppendEntries) -> AppendReply {
        let vote::ElectionData { term, .. } = self.election_data();
        if req.term > term {
            self.set_term(req.term);
        }

        if req.term < term {
            return AppendReply::ExPresident(term);
        }

        // got rpc from current leader
        self.vars.heartbeat.notify_one();

        if !self.log_contains(req.prev_log_idx, req.prev_log_term) {
            return AppendReply::InconsistentLog;
        }

        // This must execute in parallel without side effects
        // see called functions for motivation
        let n_entries = req.entries.len() as u32;
        for (i, order) in req.entries.into_iter().enumerate() {
            let index = req.prev_log_idx + i as u32 + 1;
            self.prepare_log(index, req.term);
            let entry = LogEntry {term, order};
            self.insert_into_log(index, &entry)
        }

        if req.leader_commit > self.commit_index() {
            let last_new_idx = req.prev_log_idx + n_entries;
            let new = u32::min(req.leader_commit, last_new_idx);
            self.set_commit_index(new);
        }

        // This must execute in parallel without side effects
        // see called functions for motivation
        let last_applied = self.last_applied();
        if self.commit_index() > last_applied {
            let to_apply = self.increment_last_applied();
            self.apply_log(to_apply);
        }

        AppendReply::Ok
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    term: Term,
    order: Order,
}

impl Default for LogEntry {
    fn default() -> Self {
        Self {
            term: 0,
            order: Order::None,
        }
    }
}

impl State {
    pub(super) async fn order(&self, ord: Order) {
        self.tx.send(ord).await.unwrap();
    }

    pub(super) fn log_contains(&self, prev_log_idx: u32, prev_log_term: u32) -> bool {
        match self.db.get_val(db::log_key(prev_log_idx)) {
            Some(LogEntry{ term, ..}) if term == prev_log_term => true,
            _ => false,
        }
    }

    // check side effects if called interleaved
    fn prepare_log(&self, _index: u32, _term: u32) {
        todo!()
    }

    fn insert_into_log(&self, index: u32, entry: &LogEntry) {
        let mut key = [db::Prefix::Log as u8, 0,0,0,0];
        key[1..5].clone_from_slice(&index.to_ne_bytes());
        self.db.set_val(key, entry);
    }

    pub(super) fn apply_log(&self, _idx: u32) {
        todo!();
        //self.order()
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
}

impl State {
    /// Sets a new higher commit index
    fn set_commit_index(&self, new: u32) {
        self.vars.commit_index.fetch_max(new, Ordering::SeqCst);
    }

    pub fn commit_index(&self) -> u32 {
        self.vars.commit_index.load(Ordering::SeqCst)
    }

    fn increment_last_applied(&self) -> u32 {
        self.vars.last_applied.fetch_add(1, Ordering::SeqCst)
    }

    pub(crate) fn last_applied(&self) -> u32 {
        self.vars.last_applied.load(Ordering::SeqCst)
    }

    pub fn heartbeat(&self) -> &Notify {
        &self.vars.heartbeat
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntries {
    pub term: Term,
    pub leader_id: Id,
    pub prev_log_idx: LogIdx,
    pub prev_log_term: Term,
    pub entries: Vec<Order>,
    pub leader_commit: LogIdx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppendReply {
    Ok,
    ExPresident(Term),
    InconsistentLog,
}

