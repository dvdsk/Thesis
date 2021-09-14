use client_protocol::PathString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum FromRS {
    Election(ElectionMsg),
}

type Term = u64;
type ChangeIdx = u64;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ElectionMsg {
    HeartBeat(Term, ChangeIdx),
    RequestVote(Term, ChangeIdx),
    VotedForYou(Term),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMsg {
    GetServerList,
    DirectoryChange(Change, ChangeIdx),
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToRs {
    Election(ElectionMsg),
    Control(ControlMsg),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Change {
    DirAdded(PathString),
}
