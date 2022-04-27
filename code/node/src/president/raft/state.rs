use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify};

use crate::util::TypedSled;
use crate::Id;

use super::Order;
type Term = u32;
type LogIdx = u32;

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
    pub const TERM: [u8; 1] = [1u8];
    pub const VOTED_FOR: [u8; 1] = [2u8];
}

#[derive(Debug, Clone)]
pub struct State {
    tx: mpsc::Sender<Order>,
    db: sled::Tree,
    vars: Arc<Vars>,
}

impl State {
    pub fn new(tx: mpsc::Sender<Order>, db: sled::Tree) -> Self {
        Self {
            tx,
            db,
            vars: Default::default(),
        }
    }
    pub fn vote_req(&self, req: RequestVote) -> Option<VoteReply> {
        if req.term > self.term() {
            self.set_term(req.term);
        }

        if req.term < self.term() {
            return None;
        }

        if let Some(id) = self.voted_for() {
            if id != req.candidate_id {
                return None;
            }
        }

        if req.log_up_to_date(&self) {
            Some(VoteReply {
                term: self.term(),
                vote_granted: (),
            })
        } else {
            None
        }
    }
    /// handle append request, this can be called in parallel.
    pub fn append_req(&self, req: AppendEntries) -> AppendReply {
        if req.term > self.term() {
            self.set_term(req.term);
        }

        if req.term < self.term() {
            return AppendReply::InconsistentLog(self.term());
        }

        if !self.log_contains(req.prev_log_idx, req.prev_log_term) {
            return AppendReply::InconsistentLog(self.term());
        }

        // This must execute in parallel without side effects
        // see called functions for motivation
        for (i, entry) in req.entries.iter().enumerate() {
            let index = req.prev_log_idx + i as u32 + 1;
            self.prepare_log(index, req.term);
            self.insert_into_log(index, entry)
        }

        if req.leader_commit > self.commit_index() {
            let last_new_idx = req.prev_log_idx + req.entries.len() as u32;
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
        let mut sub = self.db.watch_prefix(db::TERM);
        while let Some(event) = (&mut sub).await {
            match event {
                sled::Event::Insert{key, value} if &key == &db::TERM => {
                    let term = bincode::deserialize(&value).unwrap();
                    return term;
                }
                _ => panic!("term key should never be removed"),
            }
        }
        unreachable!("db subscriber should never be dropped")
    }
}

impl State {
    pub(super) async fn order(&self, ord: Order) {
        self.tx.send(ord).await.unwrap();
    }

    pub(super) fn log_contains(&self, prev_log_idx: u32, prev_log_term: u32) -> bool {
        todo!()
    }

    // check side effects if called interleaved
    fn prepare_log(&self, index: u32, term: u32) {
        todo!()
    }

    fn insert_into_log(&self, index: u32, entry: &Order) {
        todo!()
    }

    pub(super) fn apply_log(&self, idx: u32) {
        todo!();
        //self.order()
    }
}

impl State {
    /// Sets a new higher commit index
    fn set_commit_index(&self, new: u32) {
        self.vars.commit_index.fetch_max(new, Ordering::SeqCst);
    }

    fn commit_index(&self) -> u32 {
        self.vars.commit_index.load(Ordering::SeqCst)
    }

    fn increment_last_applied(&self) -> u32 {
        self.vars.last_applied.fetch_add(1, Ordering::SeqCst)
    }

    fn term(&self) -> Term {
        self.db.get_val(db::TERM).unwrap_or(0)
    }

    fn set_term(&self, term: u32) {
        self.db.set_val(db::TERM, term);
    }

    /// increase term by one and return the new term
    pub(crate) fn increment_term(&self) -> u32 {
        self.db.increment(db::TERM)
    }

    fn voted_for(&self) -> Option<Id> {
        self.db.get_val(db::VOTED_FOR)
    }

    pub(crate) fn last_applied(&self) -> u32 {
        self.vars.last_applied.load(Ordering::SeqCst)
    }

    pub fn heartbeat(&self) -> &Notify {
        &self.vars.heartbeat
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVote {
    term: Term,
    candidate_id: Id,
    last_log_idx: u32,
    last_log_term: Term,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteReply {
    term: Term,
    // implied by sending a reply at all
    vote_granted: (), // here to match Raft paper
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntries {
    term: Term,
    leader_id: Id,
    prev_log_idx: LogIdx,
    prev_log_term: Term,
    entries: Vec<Order>,
    leader_commit: LogIdx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppendReply {
    Ok,
    InconsistentLog(Term),
}
impl RequestVote {
    fn log_up_to_date(&self, arg: &&State) -> bool {
        todo!()
    }
}
