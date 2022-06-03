use serde::{Serialize, Deserialize};

use crate::Idx;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
    CommitIdx(Idx),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    ReqCommitIdx,
}
