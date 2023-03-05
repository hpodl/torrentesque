use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
pub enum RequestToTracker {
    GetPeers,
}

#[derive(Serialize, Deserialize)]
pub enum TrackerResponse {
    Peers(Vec<SocketAddr>),
    InvalidRequest,
}
