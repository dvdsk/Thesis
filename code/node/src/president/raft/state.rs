use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::Id;

use super::Order;
type Term = u32;
type LogIdx = u32;

#[derive(Debug, Clone)]
pub struct State {
    tx: mpsc::Sender<Order>,
    db: sled::Tree,
}

impl State {
    pub fn new(tx: mpsc::Sender<Order>, db: sled::Tree) -> Self {
        Self { tx, db }
    }
    pub fn vote_req(&self, req: RequestVote) -> Option<VoteReply> {
        if req.term > self.term() {
            self.give_up_election()
        }

        if req.term < self.term() {
            return None;
        }

        if let Some(candidate) = self.voted_for() {
            if candidate != r
        }
        if self.voted_for.is_none() || self.voted_for == Some(req.candidate_id) {
            if req.log_up_to_date(&self) {
            }
        }
    }
    pub fn append_req(&self, req: AppendEntries) {}
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

pub struct AppendReply {
    term: Term,
    succes: bool,
}
