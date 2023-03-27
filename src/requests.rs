use bit_vec::BitVec;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Serialize, Deserialize)]
/// Requests peers send to a tracker
pub enum RequestToTracker {
    GetPeers,
    RegisterAsPeer(SocketAddr),
}

#[derive(Serialize, Deserialize)]
/// Tracker's response to a peer's request (`RequestToTracker`)
pub enum TrackerResponse {
    Peers(Vec<SocketAddr>),
    InvalidRequest,
    RegisteredSuccesfully,
}

#[derive(Serialize, Deserialize)]
/// Requests a leech makes to its peers
pub enum LeechRequest {
    GetAvailability,
    GetPackets(usize, usize),
}

#[derive(Serialize, Deserialize, Debug)]
/// Seed's response to leech's request (`LeechRequest`)
pub enum SeedResponse {
    InvalidRequest,
    Availability(BitVec),
}
