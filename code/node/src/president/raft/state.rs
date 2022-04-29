use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sled::CompareAndSwapError;
use tokio::sync::{mpsc, Notify};
use tracing::{debug, instrument, trace, warn};

use crate::util::TypedSled;
use crate::Id;

use self::db::LOG;

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
}

#[derive(Debug, Clone)]
pub struct State {
    tx: mpsc::Sender<Order>,
    db: sled::Tree,
    vars: Arc<Vars>,
}

impl State {
    pub fn new(tx: mpsc::Sender<Order>, db: sled::Tree) -> Self {
        let data = bincode::serialize(&ElectionData::default()).unwrap();
        let _ig_existing_key_val_err = db.compare_and_swap(
            db::ELECTION_DATA,
            None as Option<&[u8]>,
            Some(data),
        )
        .unwrap();

        Self {
            tx,
            db,
            vars: Default::default(),
        }
    }
    #[instrument(ret, skip(self))]
    pub fn vote_req(&self, req: RequestVote) -> Option<VoteReply> {
        let ElectionData {
            mut term,
            voted_for,
        } = self.election_data();
        if req.term > term {
            term = req.term;
            self.set_term(req.term);
        }

        if req.term < term {
            trace!("term to low");
            return None;
        }

        if let Some(id) = voted_for {
            if id != req.candidate_id {
                trace!("already voted");
                return None;
            }
        }

        if req.log_up_to_date(&self) {
            if self.set_voted_for(term, req.candidate_id) {
                return Some(VoteReply {
                    term,
                    vote_granted: (),
                });
            }
        }

        None
    }
    /// handle append request, this can be called in parallel.
    pub fn append_req(&self, req: AppendEntries) -> AppendReply {
        let ElectionData { term, .. } = self.election_data();
        if req.term > term {
            self.set_term(req.term);
        }

        if req.term < term {
            return AppendReply::InconsistentLog(term);
        }

        if !self.log_contains(req.prev_log_idx, req.prev_log_term) {
            return AppendReply::InconsistentLog(term);
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
        let mut sub = self.db.watch_prefix(db::ELECTION_DATA);
        while let Some(event) = (&mut sub).await {
            match event {
                sled::Event::Insert { key, value } if &key == &db::ELECTION_DATA => {
                    let data: ElectionData = bincode::deserialize(&value).unwrap();
                    return data.term;
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
                idx: u32::from_ne_bytes(key[1..=5].try_into().unwrap()),
                term: u32::from_ne_bytes(value[0..=4].try_into().unwrap()),
            },
            _ => Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElectionData {
    /// the current term
    term: Term,
    voted_for: Option<Id>,
}

impl State {
    fn election_data(&self) -> ElectionData {
        self.db.get_val(db::ELECTION_DATA).unwrap_or_default()
    }

    /// updates term and resets voted_for
    fn set_term(&self, term: u32) {
        let data = ElectionData {
            term,
            voted_for: None,
        };
        self.db.set_val(db::ELECTION_DATA, data);
    }

    /// sets voted_for if the term did not change and it was not
    #[instrument(ret, skip(self))]
    fn set_voted_for(&self, term: u32, candidate_id: u64) -> bool {
        let old = ElectionData {
            term,
            voted_for: None,
        };
        let new = ElectionData {
            term,
            voted_for: Some(candidate_id),
        };
        let res = self
            .db
            .compare_and_swap(
                db::ELECTION_DATA,
                Some(bincode::serialize(&old).unwrap()),
                Some(bincode::serialize(&new).unwrap()),
            )
            .expect("database encounterd error");
        // .is_ok()
        match res {
            Ok(..) => true,
            Err(e) => {
                warn!("not setting vote: {e:?}");
                false
            }
        }
    }

    /// increase term by one and return the new term
    pub(crate) fn increment_term(&self) -> u32 {
        self.db
            .update(db::ELECTION_DATA, |ElectionData { term, .. }| {
                ElectionData {
                    term: term + 1,
                    voted_for: None,
                }
            })
            .map(|data| data.term)
            .unwrap_or_default()
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

    pub(crate) fn last_applied(&self) -> u32 {
        self.vars.last_applied.load(Ordering::SeqCst)
    }

    pub fn heartbeat(&self) -> &Notify {
        &self.vars.heartbeat
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVote {
    pub term: Term,
    pub candidate_id: Id,
    pub last_log_idx: u32,
    pub last_log_term: Term,
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
    /// check if the log of the requester is at least as
    /// up to date as ours
    #[instrument(ret, skip(arg, self))]
    fn log_up_to_date(&self, arg: &State) -> bool {
        self.last_log_idx >= arg.last_log_meta().idx
    }
}
