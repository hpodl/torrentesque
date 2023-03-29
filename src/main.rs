use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use client::Client;
use tracker::Tracker;
use tokio::fs::OpenOptions;
use tokio::sync::oneshot;
use tokio::time::{self, sleep};
use torrent_file::TorrentFile;

mod client;
mod requests;
mod tracker;
mod torrent_file;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let packet_size = 4096;
    let torrent_size = {
        let complete_torrent_file = OpenOptions::new()
            .read(true)
            .open(".testfiles/sent.png")
            .await?;
        complete_torrent_file.metadata().await.unwrap().len() as usize
    };

    let server_addr = SocketAddr::from_str("127.0.0.168:1111").unwrap();
    let seed1_addr = SocketAddr::from_str("127.0.0.166:2222").unwrap();
    let seed2_addr = SocketAddr::from_str("127.0.0.167:3333").unwrap();
    let leech_addr = SocketAddr::from_str("127.0.0.167:4444").unwrap();
    let full_peer_addr = SocketAddr::from_str("127.0.0.167:5555").unwrap();

    let (tracker_wx, tracker_rx) = oneshot::channel::<()>();
    let mut server = Tracker::new();
    let server_handle = tokio::spawn(async move { server.listen(&server_addr, tracker_rx).await });

    // Otherwise might not bind before the client attempts connecting to the server
    sleep(Duration::from_millis(250)).await;

    // First seed //
    let (seed_wx, seed_rx) = oneshot::channel::<()>();
    let seed = Client::new(
        seed1_addr,
        TorrentFile::from_complete(".testfiles/sent.png", packet_size).unwrap(),
    );

    let seed_handle = tokio::spawn({
        async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            seed.register_as_peer(&server_addr).await?;
            seed.seed_loop(seed_rx).await
        }
    });

    // Second seed //
    let (seed2_wx, seed2_rx) = oneshot::channel::<()>();
    let seed = Client::new(
        seed2_addr,
        TorrentFile::from_complete(".testfiles/sent.png", packet_size).unwrap(),
    );

    let seed2_handle = tokio::spawn({
        async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            seed.register_as_peer(&server_addr).await?;
            seed.seed_loop(seed2_rx).await
        }
    });

    sleep(Duration::from_millis(50)).await;

    // Full peer (seed + leech) //
    let (_f_leech_wx, f_leech_rx) = oneshot::channel::<()>(); // writer unused but not dropped
    let leech2 = Client::new(
        full_peer_addr,
        TorrentFile::new(".testfiles/received2.png", torrent_size, packet_size).unwrap(),
    );

    let peer_arc = Arc::new(leech2);
    let arc_copy = Arc::clone(&peer_arc);
    let f_peer_leech_handle =
        tokio::spawn(async move { arc_copy.leech_loop(&server_addr, f_leech_rx).await });

    let arc_copy = Arc::clone(&peer_arc);
    let (fseed_wx, fseed_rx) = oneshot::channel::<()>(); // writer unused but not dropped
    let f_peer_seed_handle = tokio::spawn(async move {
        arc_copy.register_as_peer(&server_addr).await?;
        arc_copy.seed_loop(fseed_rx).await
    });

    // Let's give the leech+seed peer time to get some packets, so that it can send them to the leech
    time::sleep(Duration::from_millis(25)).await;

    // Leech //
    let (_leech_wx, leech_rx) = oneshot::channel::<()>(); // writer unused but not dropped
    let leech = Client::new(
        leech_addr,
        TorrentFile::new(".testfiles/received.png", torrent_size, packet_size).unwrap(),
    );
    let leech_handle = tokio::spawn(async move { leech.leech_loop(&server_addr, leech_rx).await });

    leech_handle.await??;
    f_peer_leech_handle.await??;
    
    tracker_wx.send(()).unwrap();
    seed_wx.send(()).unwrap();
    seed2_wx.send(()).unwrap();
    fseed_wx.send(()).unwrap();
    
    f_peer_seed_handle.await??;
    seed_handle.await??;
    seed2_handle.await??;
    server_handle.await??;

    Ok(())
}

#[cfg(test)]
mod test {}
