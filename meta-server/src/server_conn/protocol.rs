use client_protocol::PathString;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum RsMsg {}

#[derive(Serialize, Deserialize)]
pub enum WsMsg {
    DirectoryChange(Change),
    Test,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Change {
    DirAdded(PathString),
}
