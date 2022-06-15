use super::super::state::append::Request;
use super::Source;
use crate::raft::state::LogMeta;
use crate::raft::{State, Order};
use crate::Term;

#[derive(Debug, Clone)]
pub struct RequestGen<O> {
    state: State<O>,
    pub base: Request<O>,
    pub next_idx: u32,
}

impl<O: Order> RequestGen<O> {
    pub fn heartbeat(&self) -> Request<O> {
        Request {
            leader_commit: self.state.commit_index(),
            entries: Vec::new(),
            ..self.base
        }
    }

    pub fn increment_idx(&mut self) {
        self.base.prev_log_term = self.base.term;
        self.base.prev_log_idx = self.next_idx;
        self.next_idx += 1;
    }

    pub fn decrement_idx(&mut self) {
        self.next_idx -= 1;
        let prev_entry = self.state.entry_at(self.next_idx - 1).unwrap();
        self.base.prev_log_term = prev_entry.term;
    }

    pub fn append(&mut self) -> Request<O> {
        let entry = self.state.entry_at(self.next_idx).unwrap();
        let req = Request {
            leader_commit: self.state.commit_index(),
            prev_log_idx: self.next_idx - 1,
            entries: vec![entry.order],
            ..self.base
        };
        self.base.prev_log_term = entry.term;
        req
    }

    pub(crate) fn misses_logs(&self) -> bool {
        self.state.last_log_meta().idx >= self.next_idx
    }

    pub fn new(state: State<O>, term: Term, chart: &impl Source) -> Self {
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

        let next_idx = state.last_applied() + 1;
        Self {
            state,
            next_idx,
            base,
        }
    }
}
