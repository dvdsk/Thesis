use client_protocol::PathString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum ToWs {
    Election(ElectionMsg),
}

type Term = u64;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ElectionMsg {
    HeartBeat(Term),
    RequestVote(Term),
    VotedForYou(Term),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMsg {
    GetServerList,
    DirectoryChange(Change),
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
