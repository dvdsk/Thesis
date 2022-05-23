use super::super::state::append::Request;
use crate::president::raft::state::LogMeta;
use crate::president::raft::State;
use crate::president::Chart;
use crate::Term;

#[derive(Debug, Clone)]
pub struct RequestGen {
    state: State,
    pub base: Request,
    pub next_idx: u32,
}

impl RequestGen {
    pub fn heartbeat(&self) -> Request {
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
        todo!("fix prev log term");
    }

    pub fn append(&mut self) -> Request {
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

    pub fn new(state: State, term: Term, chart: &Chart) -> Self {
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

// TODO make it so "raft client" also runs on president possibly by calling append_req with an
// append req in the instruct funct, kinda instructing ourself. Might fit in LogWriter too 
