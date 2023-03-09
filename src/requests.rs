use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Serialize, Deserialize)]
pub enum RequestToTracker {
    GetPeers,
    RegisterAsPeer(SocketAddr),
}

#[derive(Serialize, Deserialize)]
pub enum TrackerResponse {
    Peers(Vec<SocketAddr>),
    InvalidRequest,
    Ok,
}

#[derive(Serialize, Deserialize)]
pub enum LeechRequest {
    GetAvailability,
    GetPackets(usize, usize),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum SeedResponse {
    InvalidRequest,
}
