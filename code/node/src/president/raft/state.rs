use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::util::TypedSled;
use crate::Id;

use super::Order;
type Term = u32;
type LogIdx = u32;

#[derive(Debug)]
struct Vars {
    last_applied: AtomicU32,
    commit_index: AtomicU32,
}

impl Default for Vars {
    fn default() -> Self {
        Self {
            last_applied: AtomicU32::new(0),
            commit_index: AtomicU32::new(0),
        }
    }
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
            self.give_up_election();
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
            self.give_up_election();
            self.set_term(req.term);
        }

        if req.term < self.term() {
            return AppendReply { term: self.term(), succes: false };
        }

        if !self.log_contains(req.prev_log_idx, req.prev_log_term) {
            return AppendReply { term: self.term(), succes: false };
        }

        // This must execute in parallel without side effects
        // see called functions for motivation
        for (i, entry) in req.entries.iter().enumerate() {
            let index = req.prev_log_idx + i as u32 + 1;
            self.prepare_log(index, req.term);
            self.insert(index, entry)
        }

        if req.leader_commit > self.commit_index() {
            let last_new_idx = req.prev_log_idx + req.entries.len() as u32;
            let new = u32::min(req.leader_commit, last_new_idx);
            self.set_commit_index(new);
        }

        // This must execute in parallel without side effects
        // see called functions for motivation
        if self.commit_index() > self.last_applied() {
            self.increment_last_applied();
            self.apply_log();
        }

        AppendReply {
            term: self.term(),
            succes: true,
        }
    }

    pub(crate) fn log_contains(&self, prev_log_idx: u32, prev_log_term: u32) -> bool {
        todo!()
    }

    // check side effects if called interleaved
    fn prepare_log(&self, index: u32, term: u32) {
        todo!()
    }

    fn insert(&self, index: u32, entry: &Order) {
        todo!()
    }

    fn set_commit_index(&self, new: u32) {
        todo!()
    }

    fn commit_index(&self) -> u32 {
        todo!()
    }

    fn increment_last_applied(&self) {
        todo!()
    }
}

impl State {
    fn term(&self) -> Term {
        self.db.get_val("term").unwrap_or(0)
    }

    fn voted_for(&self) -> Option<Id> {
        self.db.get_val("voted_for")
    }

    fn give_up_election(&self) {
        todo!()
    }

    fn set_term(&self, term: u32) {
        todo!()
    }

    pub(crate) fn last_applied(&self) -> u32 {
        todo!()
    }

    // increments last_applied and applies log msg
    pub(crate) fn apply_log(&self) {
        todo!()
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
pub struct AppendReply {
    term: Term,
    // false if log inconsistancy
    succes: bool,
}
impl RequestVote {
    fn log_up_to_date(&self, arg: &&State) -> bool {
        todo!()
    }
}
