use core::fmt;

use tracing::instrument;

use super::super::state::append::Request;
use crate::{raft, Id, Idx, Term};
use raft::state::{append::LogEntry, LogMeta};
use raft::Order;

pub trait StateInfo: Clone {
    type Order: Clone + fmt::Debug;
    fn entry(&self, idx: u32) -> Option<LogEntry<Self::Order>>;
    fn commit_idx(&self) -> Idx;
    fn last_meta(&self) -> LogMeta;
    fn last_appl(&self) -> Idx;
}

impl<O: Clone + Order> StateInfo for super::State<O> {
    type Order = O;
    fn entry(&self, idx: u32) -> Option<LogEntry<O>> {
        self.entry_at(idx)
    }
    fn commit_idx(&self) -> Idx {
        self.commit_index()
    }
    fn last_meta(&self) -> LogMeta {
        self.last_log_meta()
    }
    fn last_appl(&self) -> Idx {
        self.last_applied()
    }
}

pub trait GetTerm: Clone + Send {
    fn curr(&mut self) -> Term;
    /// returns the value of the previous call to curr
    fn prev(&mut self) -> Term;
}

#[derive(Debug, Clone)]
pub struct RequestGen<S: StateInfo, T: GetTerm> {
    state: S,
    leader_id: Id,
    prev_log_idx: Idx,
    prev_log_term: Term,
    pub next_idx: u32,
    pub term: T,
}

impl<S, T> RequestGen<S, T>
where
    S: StateInfo,
    T: GetTerm,
{
    #[instrument(skip_all, level = "trace")]
    pub fn heartbeat(&mut self) -> Request<<S as StateInfo>::Order> {
        Request {
            leader_commit: self.state.commit_idx(),
            entries: Vec::new(),
            term: self.term.curr(),
            leader_id: self.leader_id,
            prev_log_idx: self.prev_log_idx,
            prev_log_term: self.prev_log_term,
        }
    }

    #[instrument(skip_all, level = "trace")]
    pub fn increment_idx(&mut self) {
        self.prev_log_term = self.term.prev(); // TODO won work if term can change
        self.prev_log_idx = self.next_idx;
        self.next_idx += 1;
    }

    #[instrument(skip_all, level = "trace")]
    pub fn decrement_idx(&mut self) {
        self.next_idx -= 1;
        let prev_entry = self.state.entry(self.next_idx - 1).unwrap();
        self.prev_log_term = prev_entry.term;
    }

    #[instrument(skip_all, level = "trace")]
    pub fn append(&mut self) -> Request<<S as StateInfo>::Order> {
        let entry = self.state.entry(self.next_idx).unwrap();
        let req = Request {
            leader_commit: self.state.commit_idx(),
            prev_log_idx: self.next_idx - 1,
            entries: vec![entry.order],
            term: self.term.curr(),
            leader_id: self.leader_id,
            prev_log_term: self.prev_log_term,
        };
        self.prev_log_term = entry.term;
        req
    }

    #[instrument(skip_all, level = "trace")]
    pub(crate) fn misses_logs(&self) -> bool {
        self.state.last_meta().idx >= self.next_idx
    }

    pub fn new(state: S, term: T, leader_id: Id) -> Self {
        let LogMeta {
            idx: prev_idx,
            term: prev_term,
        } = state.last_meta();

        let next_idx = state.last_appl() + 1;
        Self {
            state,
            next_idx,
            leader_id,
            prev_log_idx: prev_idx,
            prev_log_term: prev_term,
            term,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::president::FixedTerm;

    #[derive(Clone)]
    struct MockState;
    impl StateInfo for MockState {
        type Order = ();
        fn entry(&self, _idx: u32) -> Option<LogEntry<()>> {
            None
        }

        fn commit_idx(&self) -> Idx {
            0
        }

        fn last_meta(&self) -> LogMeta {
            LogMeta { term: 0, idx: 0 }
        }

        fn last_appl(&self) -> Idx {
            0
        }
    }

    #[test]
    fn increment_idx() {
        let state = MockState;
        let mut gen = RequestGen::new(state, FixedTerm(0), 0);
        for i in 2..1024 {
            gen.increment_idx();
            assert_eq!(gen.next_idx, i);
        }
    }
}
