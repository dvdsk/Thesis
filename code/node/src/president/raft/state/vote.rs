use std::ops::Deref;

use serde::{Deserialize, Serialize};
use tracing::{instrument, trace};

use super::{db, State};
use crate::util::TypedSled;
use crate::{Id, Term};

impl State {
    // this does not need to be lockless as its called a lot
    // once a leader goes down. The function never blocks so
    // locks will not block long.
    #[instrument(level= "debug", skip(self), fields(id = self.id), ret)]
    pub async fn vote_req(&self, req: RequestVote) -> Option<VoteReply> {
        // lock term and voted_for,
        let mut election_office = self.election_office.lock().await;
        let ElectionData {
            mut term,
            mut voted_for,
        } = *election_office.data();

        if req.term < term {
            trace!("term to low");
            return None; // TODO: drops lock
        }

        if req.term > term {
            term = req.term;
            voted_for = None;
            election_office.set_term(req.term); // TODO use db update to check term
        }

        if let Some(id) = voted_for {
            if id != req.candidate_id {
                trace!("already voted");
                return None;
            }
        }

        if req.log_up_to_date(self) && election_office.set_voted_for(term, req.candidate_id) {
            return Some(VoteReply {
                term,
                vote_granted: (),
            });
        }

        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElectionData {
    /// the current term
    term: Term,
    voted_for: Option<Id>,
}

impl ElectionData {
    pub fn term(&self) -> &Term {
        &self.term
    }
}

/// exposes methods to get election data. Thanks to the borrow
/// members type this struct does not auto implement Sync. Therefore
/// it must be wrapped in some concurrent access control. This is
/// enough to preventing race conditions in its member functions
#[derive(Debug)]
pub struct ElectionOffice {
    // I use this to enforce lifetimes upon
    // returned electiondata, this makes sure
    // the data can not be used when the election
    // office goes out of scope
    db: sled::Tree,
}

pub struct ElectionGuard<'a> {
    data: ElectionData,
    _office: &'a ElectionOffice,
}

impl<'a> Deref for ElectionGuard<'a> {
    type Target = ElectionData;
    fn deref(self: &ElectionGuard<'a>) -> &Self::Target {
        &self.data
    }
}

impl ElectionOffice {
    pub(crate) fn from_tree(db: &sled::Tree) -> Self {
        Self { db: db.clone() }
    }
    pub(super) fn init_election_data(&self) {
        let data = bincode::serialize(&ElectionData::default()).unwrap();
        let _ig_existing_key_val_err = self
            .db
            .compare_and_swap(db::ELECTION_DATA, None as Option<&[u8]>, Some(data))
            .unwrap();
    }
    pub fn data<'a>(&'a self) -> ElectionGuard<'a> {
        let data = self.db.get_val(db::ELECTION_DATA).unwrap_or_default();
        ElectionGuard {
            data,
            _office: self,
        }
    }

    /// updates term and resets voted_for
    /// is &mut to ensure old data (only availible via borrow)
    /// is no longer in scope
    pub(super) fn set_term(&mut self, term: u32) {
        let data = ElectionData {
            term,
            voted_for: None,
        };
        self.db.set_val(db::ELECTION_DATA, data);
    }

    /// sets voted_for if the term did not change and it was not
    #[instrument(ret, skip(self), level = "trace")]
    fn set_voted_for(&self, term: u32, candidate_id: u64) -> bool {
        let old = ElectionData {
            term,
            voted_for: None,
        };
        let new = ElectionData {
            term,
            voted_for: Some(candidate_id),
        };
        self.db
            .compare_and_swap(
                db::ELECTION_DATA,
                Some(bincode::serialize(&old).unwrap()),
                Some(bincode::serialize(&new).unwrap()),
            )
            .expect("database encounterd error")
            .is_ok()
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVote {
    pub term: Term,
    pub candidate_id: Id,
    pub last_log_idx: u32,
    pub last_log_term: Term,
}

impl RequestVote {
    /// check if the log of the requester is at least as
    /// up to date as ours
    #[instrument(ret, skip(arg, self), level = "trace")]
    fn log_up_to_date(&self, arg: &State) -> bool {
        self.last_log_idx >= arg.last_log_meta().idx
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteReply {
    term: Term,
    // implied by sending a reply at all
    vote_granted: (), // here to match Raft paper
}
