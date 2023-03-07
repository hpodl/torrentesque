use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

use client::Client;
use server::Server;
use tokio::sync::oneshot;
use tokio::time::sleep;
use torrent_file::TorrentFile;

mod client;
mod requests;
mod server;
mod torrent_file;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let torrent_size = 10usize;
    let packet_size = 2usize;


    let client_addr = SocketAddr::from_str("127.0.0.166:5468").unwrap();
    let client2_addr = SocketAddr::from_str("127.0.0.167:7846").unwrap();
    let server_addr = SocketAddr::from_str("127.0.0.168:11256").unwrap();

    let (tracker_wx, tracker_rx) = oneshot::channel::<()>();
    let mut server = Server::new();
    let server_handle = tokio::spawn(async move { server.listen(&server_addr, tracker_rx).await });

    // Otherwise might not bind before the client attempts connecting to the server
    sleep(Duration::from_millis(250)).await;

    let (seed_wx, seed_rx) = oneshot::channel::<()>();
    let seed = Client::new(client_addr, TorrentFile::from_complete(".testfiles/mainfile1", torrent_size, packet_size).unwrap());

    let seed_handle = tokio::spawn({
        async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            seed.register_as_peer(&server_addr).await?;
            seed.seed_loop(seed_rx).await
        }
    });
    sleep(Duration::from_millis(50)).await;

    let (leech_wx, leech_rx) = oneshot::channel::<()>();
    let mut leech = Client::new(client2_addr, TorrentFile::new(".testfiles/mainfile2", torrent_size, packet_size).unwrap());
    let leech_handle = tokio::spawn(async move { leech.leech_loop(&server_addr, leech_rx).await });

    sleep(Duration::from_millis(125)).await;

    leech_wx.send(()).unwrap();
    tracker_wx.send(()).unwrap();
    seed_wx.send(()).unwrap();

    seed_handle.await??;
    server_handle.await??;

    leech_handle.await??;

    Ok(())
}

#[cfg(test)]
mod test {}
