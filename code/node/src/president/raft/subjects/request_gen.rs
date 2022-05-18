use super::super::state::append::Request;
use super::commited::Commited;
use crate::president::raft::state::LogMeta;
use crate::president::raft::State;
use crate::president::Chart;
use crate::Term;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RequestGen {
    commit_idx: Arc<AtomicU32>,
    pub base: Request,
}

impl RequestGen {
    pub fn heartbeat(&self) -> Request {
        Request {
            leader_commit: self.commit_idx.load(Ordering::Relaxed),
            entries: Vec::new(),
            ..self.base
        }
    }

    pub fn append(&mut self, state: &State, next_idx: u32) -> Request {
        let entry = state.entry_at(next_idx);
        let req = Request {
            leader_commit: self.commit_idx.load(Ordering::Relaxed),
            prev_log_idx: next_idx - 1,
            entries: vec![entry.order],
            ..self.base
        };
        self.base.prev_log_term = self.base.term;
        req
    }

    pub fn new(state: &State, commit_idx: &Commited, term: Term, chart: &Chart) -> Self {
        let LogMeta {
            idx: prev_idx,
            term: prev_term,
        } = state.last_log_meta();

        let base = Request {
            term,
            leader_id: chart.our_id(),
            prev_log_idx: prev_idx,
            prev_log_term: prev_term,
            entries: Vec::new(),
            leader_commit: state.commit_index(),
        };

        Self {
            commit_idx: commit_idx.commit_idx.clone(),
            base,
        }
    }
}
