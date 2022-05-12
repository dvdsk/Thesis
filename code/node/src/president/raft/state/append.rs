use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tracing::instrument;

use super::{db, LogIdx, Order, State};
use crate::util::TypedSled;
use crate::{Id, Term};

impl State {
    #[instrument(skip(self), fields(id = self.id), ret)]
    pub async fn append_req(&self, req: Request) -> Reply {
        {
            // lock scope of election_office
            let mut election_office = self.election_office.lock().unwrap();
            let election_data = election_office.data();
            let mut term = election_data.term();

            if req.term > *term {
                election_office.set_term(req.term); // needs lock
                term = &req.term;
            }

            if req.term < *term {
                return Reply::ExPresident(*term);
            }

            // at this point the request can only be from 
            // (as seen from this node) a valid leader
            self.vars.heartbeat.notify_one();

            // From here on we need to be locked , are locked by the election_office
            if !self.log_contains(req.prev_log_idx, req.prev_log_term) {
                return Reply::InconsistentLog;
            }

            let n_entries = req.entries.len() as u32;
            for (i, order) in req.entries.into_iter().enumerate() {
                let index = req.prev_log_idx + i as u32 + 1;
                self.prepare_log(index, req.term);
                let entry = LogEntry { term: *term, order };
                self.insert_into_log(index, &entry)
            }

            if req.leader_commit > self.commit_index() {
                let last_new_idx = req.prev_log_idx + n_entries;
                let new = u32::min(req.leader_commit, last_new_idx);
                self.set_commit_index(new);
            }
        } // end lock scope

        // safely runs concurrently; TODO proof
        let last_applied = self.last_applied();
        if self.commit_index() > last_applied {
            let to_apply = self.increment_last_applied();
            self.apply_log(to_apply).await;
        }

        Reply::Ok
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
    /// return true if the log contains prev_log_idx and prev_log_texm
    pub(super) fn log_contains(&self, prev_log_idx: u32, prev_log_term: u32) -> bool {
        match self.db.get_val(db::log_key(prev_log_idx)) {
            Some(LogEntry { term, .. }) if term == prev_log_term => true,
            _ => false,
        }
    }

    // TODO check side effects if called interleaved
    // If an existing entry conflicts with a new one (same index
    // but different terms), delete the existing entry and all that follow it
    fn prepare_log(&self, index: u32, term: u32) {
        let existing_entry = self.db.get_val(db::log_key(index));
        let existing_term = match existing_entry {
            None => return,
            Some(LogEntry { term, .. }) => term,
        };

        if existing_term != term {
            for key in self
                .db
                .range(db::log_key(index)..)
                .keys()
                .map(Result::unwrap)
            {
                self.db.remove(key).expect("key should still be present");
            }
        }
    }

    #[instrument(skip(self))]
    pub(super) fn insert_into_log(&self, idx: u32, entry: &LogEntry) {
        self.db.set_val(db::log_key(idx), entry);
    }

    #[instrument(skip(self))]
    pub(super) async fn apply_log(&self, idx: u32) {
        let LogEntry{order, ..} = self
            .db
            .get_val(db::log_key(idx))
            .expect("there should be an item in the log");
        self.tx.send(order).await.unwrap(); // TODO add backpressure?
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
        self.vars.last_applied.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub(crate) fn last_applied(&self) -> u32 {
        self.vars.last_applied.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub term: Term,
    pub leader_id: Id,
    pub prev_log_idx: LogIdx,
    pub prev_log_term: Term,
    pub entries: Vec<Order>,
    pub leader_commit: LogIdx,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Reply {
    Ok,
    ExPresident(Term),
    InconsistentLog,
}
