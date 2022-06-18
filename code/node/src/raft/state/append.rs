use color_eyre::eyre::eyre;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tokio::sync::mpsc::error::TrySendError;
use tracing::{instrument, warn};

use super::{db, LogIdx, Order, State};
use crate::util::TypedSled;
use crate::{Id, Idx, Term};

impl<O: Order> State<O> {
    #[instrument(skip(self), ret)]
    pub async fn append_req(&self, req: Request<O>) -> Result<Reply> {
        let nothing_to_append;
        {
            // lock scope of election_office
            let mut election_office = self.election_office.lock().await;
            let election_data = election_office.data();
            let mut term = election_data.term();

            if req.term > *term {
                election_office.set_term(req.term); // needs lock
                term = &req.term;
            }

            if req.term < *term {
                return Ok(Reply::ExPresident(*term));
            }

            // at this point the request can only be from
            // (as seen from this node) a valid leader
            self.vars.heartbeat.notify_one();

            // From here on we need to be locked, are locked by the election_office
            if !self.log_contains(req.prev_log_idx, req.prev_log_term) {
                return Ok(Reply::InconsistentLog);
            }

            let n_entries = req.entries.len() as u32;
            nothing_to_append = req.entries.is_empty();
            for (i, order) in req.entries.into_iter().enumerate() {
                let index = req.prev_log_idx + i as u32 + 1;
                self.prepare_log(index, req.prev_log_term);
                let entry = LogEntry { term: *term, order };
                self.insert_into_log(index, &entry)
            }

            if req.leader_commit > self.commit_index() {
                let last_new_idx = req.prev_log_idx + n_entries;
                let new = Idx::min(req.leader_commit, last_new_idx);
                self.set_commit_index(new);
            }
        } // end lock scope

        self.apply_comitted()?;

        Ok(match nothing_to_append {
            true => Reply::HeartBeatOk,
            false => Reply::AppendOk,
        })
    }

    pub fn apply_comitted(&self) -> Result<()> {
        let last_applied = self.last_applied();
        if self.commit_index() > last_applied {
            let to_apply = self.increment_last_applied();
            self.apply_log(to_apply)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry<O> {
    pub term: Term,
    pub order: O,
}

impl<O: Order> Default for LogEntry<O> {
    fn default() -> Self {
        Self {
            term: 0,
            order: O::none(),
        }
    }
}

impl<O: Order> State<O> {
    /// return true if the log contains prev_log_idx and prev_log_texm
    pub(super) fn log_contains(&self, prev_log_idx: Idx, prev_log_term: Term) -> bool {
        let entry: Option<LogEntry<O>> = self.db.get_val(db::log_key(prev_log_idx));
        match entry {
            Some(LogEntry { term, .. }) if term == prev_log_term => true,
            Some(LogEntry { term, .. }) => {
                warn!("log contains entry with term: {term}, which is not the required: {prev_log_term}");
                false
            }
            None => {
                warn!("log does not contain an entry for index: {prev_log_idx}");
                false
            }
        }
    }

    // TODO check side effects if called interleaved
    // If an existing entry conflicts with a new one (same index
    // but different terms), delete the existing entry and all that follow it
    fn prepare_log(&self, index: Idx, term: Term) {
        let existing_entry: Option<LogEntry<O>> = self.db.get_val(db::log_key(index));
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
    pub(super) fn insert_into_log(&self, idx: Term, entry: &LogEntry<O>) {
        self.db.set_val(db::log_key(idx), entry);
    }

    #[instrument(skip(self))]
    pub(super) fn apply_log(&self, idx: Idx) -> Result<()> {
        let LogEntry { order, .. } = self
            .db
            .get_val(db::log_key(idx))
            .expect("there should be an item in the log");
        tracing::trace!("applied log: {idx}");
        match self.tx.try_send(order) {
            Ok(..) => Ok(()),
            Err(TrySendError::Full(..)) => Err(eyre!("Orders are not consumed (fast) enough")),
            Err(TrySendError::Closed(..)) => unreachable!("Log closed the recieving mpsc"),
        }
    }
}

impl<O: Order> State<O> {
    /// Sets a new higher commit index
    pub(crate) fn set_commit_index(&self, new: Idx) {
        self.vars.commit_index.fetch_max(new, Ordering::SeqCst);
    }

    pub fn commit_index(&self) -> Idx {
        self.vars.commit_index.load(Ordering::SeqCst)
    }

    pub(super) fn increment_last_applied(&self) -> Idx {
        self.vars.last_applied.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub(crate) fn last_applied(&self) -> Idx {
        self.vars.last_applied.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request<O> {
    pub term: Term,
    pub leader_id: Id,
    pub prev_log_idx: LogIdx,
    pub prev_log_term: Term,
    pub entries: Vec<O>,
    pub leader_commit: LogIdx,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Reply {
    HeartBeatOk,
    AppendOk,
    ExPresident(Term),
    InconsistentLog,
}
