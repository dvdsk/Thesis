use serde::{Deserialize, Serialize};
use tracing::{instrument, trace};

use crate::util::TypedSled;
use crate::{Id, Term};
use super::{db, State};

impl State {
    #[instrument(skip(self), fields(id = self.id), ret)]
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

        if req.log_up_to_date(self) && self.set_voted_for(term, req.candidate_id) {
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
    pub term: Term,
    voted_for: Option<Id>,
}

impl State {
    pub(super) fn init_election_data(&self) {
        let data = bincode::serialize(&ElectionData::default()).unwrap();
        let _ig_existing_key_val_err = self
            .db
            .compare_and_swap(db::ELECTION_DATA, None as Option<&[u8]>, Some(data))
            .unwrap();
    }
    pub fn election_data(&self) -> ElectionData {
        self.db.get_val(db::ELECTION_DATA).unwrap_or_default()
    }

    /// updates term and resets voted_for
    pub(super) fn set_term(&self, term: u32) {
        let data = ElectionData {
            term,
            voted_for: None,
        };
        self.db.set_val(db::ELECTION_DATA, data);
    }

    /// sets voted_for if the term did not change and it was not
    #[instrument(ret, skip(self), level = "debug")]
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
    #[instrument(ret, skip(arg, self), level = "debug")]
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
