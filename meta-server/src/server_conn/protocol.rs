use client_protocol::PathString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum RsMsg {}

#[derive(Debug, Serialize, Deserialize)]
pub enum WsMsg {
    HeartBeat,
    GetServerList,
    DirectoryChange(Change),
    Test,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Change {
    DirAdded(PathString),
}
