use client_protocol::PathString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum FromRS {
    VotedForYou(Term),
    RequestVote(Term, ChangeIdx),
    NotVoting,
    Error,
}


#[derive(Debug, Serialize, Deserialize)]
pub enum ToWs {
    Sync,
}


#[derive(Debug, Serialize, Deserialize)]
pub enum FromWs {
    Directory(()),
}

pub type Term = u64;
pub type ChangeIdx = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToRs {
    HeartBeat(Term, ChangeIdx),
    RequestVote(Term, ChangeIdx),
    DirectoryChange(Term, ChangeIdx, Change),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Change {
    DirAdded(PathString),
}
