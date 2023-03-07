use std::net::SocketAddr;

use serde::__private::from_utf8_lossy;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use crate::requests::{LeechRequest, RequestToTracker, SeedResponse, TrackerResponse};
use crate::torrent_file::TorrentFile;
pub struct Client {
    address: SocketAddr,
    torrent_file: TorrentFile,
}

impl Client {
    pub fn new(address: SocketAddr, torrent_file: TorrentFile) -> Self {
        Self {
            address,
            torrent_file,
        }
    }
    pub async fn request_peerlist(&self, tracker_addr: &SocketAddr) -> io::Result<Vec<SocketAddr>> {
        let mut stream = TcpStream::connect(tracker_addr).await?;
        stream
            .write_all(&serde_json::to_vec(&RequestToTracker::GetPeers).unwrap())
            .await?;
        stream.write_all("\n".as_bytes()).await?;
        stream.flush().await?;

        let mut buf = [0u8; 1024];
        let bytes_read = stream.read(&mut buf).await?;
        let response = serde_json::from_slice::<TrackerResponse>(&buf[..bytes_read])?;

        match response {
            TrackerResponse::Peers(peers) => Ok(peers),
            TrackerResponse::InvalidRequest => Err(io::Error::new(
                io::ErrorKind::Other,
                "Sent invalid request.",
            )),
            _ => {
                unreachable!()
            }
        }
    }

    pub async fn register_as_peer(&self, tracker_addr: &SocketAddr) -> io::Result<()> {
        let mut stream = TcpStream::connect(tracker_addr).await?;
        stream
            .write_all(
                &serde_json::to_vec(&RequestToTracker::RegisterAsPeer(self.address)).unwrap(),
            )
            .await?;
        stream.write_all("\n".as_bytes()).await?;
        stream.flush().await?;

        Ok(())
    }

    /// Launches the seed loop, which stops when a message is passed through `shutdown_channel`
    pub async fn seed_loop(&self, shutdown_channel: oneshot::Receiver<()>) -> io::Result<()> {
        tokio::select! {
                err = self.do_seed_loop() => err,
                _ = shutdown_channel => {
                    println!("Shutting down");
                    Ok(())
            }
        }
    }

    /// Actual `seed_loop` body
    async fn do_seed_loop(&self) -> io::Result<()> {
        let listener = TcpListener::bind(self.address).await?;
        let mut packet_buffer = [0u8; 1024];

        while let Ok((stream, _)) = listener.accept().await {
            let mut buffered_stream = BufStream::new(stream);

            while let Ok(bytes_read) = buffered_stream.read(&mut packet_buffer).await {
                if bytes_read == 0 {
                    break;
                }
                let response =
                    match serde_json::from_slice::<LeechRequest>(&packet_buffer[..bytes_read]) {
                        Ok(LeechRequest::GetAvailability) => todo!(),
                        Ok(LeechRequest::GetPackets(start, count)) => SeedResponse::Packets(
                            self.torrent_file.get_packets(start, count).await?,
                        ),
                        _ => SeedResponse::InvalidRequest,
                    };
                buffered_stream
                    .write_all(&serde_json::to_vec(&response).unwrap())
                    .await?;
                buffered_stream.flush().await?;
            }
        }
        Ok(())
    }

    /// Launches the leech loop, which stops when a message is passed through `shutdown_channel`
    pub async fn leech_loop(
        &mut self,
        tracker_addr: &SocketAddr,
        shutdown_channel: oneshot::Receiver<()>,
    ) -> io::Result<()> {
        tokio::select! {
            res = shutdown_channel => {res.unwrap(); Ok(())},
            res = self.do_leech_loop(tracker_addr) => {res},
        }
    }

    /// Actual `leech_loop` body
    async fn do_leech_loop(&mut self, tracker_addr: &SocketAddr) -> io::Result<()> {
        let peerlist = self.request_peerlist(tracker_addr).await?;
        let len = peerlist.len();
        assert!(len > 0);

        // PRNG seed
        let mut random = len;

        // PRNG closure (cryptographically insecure)
        let mut gen_usize = || {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            random
        };

        let mut buf = [0u8; 1024];
        for (i, is_available) in self.torrent_file.packet_availability().iter().enumerate() {
            if is_available {
                continue;
            }

            let peer_addr = peerlist[gen_usize() % len];
            let mut stream = TcpStream::connect(peer_addr).await?;
            stream
                .write_all(&serde_json::to_vec(&LeechRequest::GetPackets(i, 1))?)
                .await?;

            let bytes_read = stream.read(&mut buf).await?;
            let response = serde_json::from_slice::<SeedResponse>(&buf[..bytes_read])
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

            if let SeedResponse::Packets(packets) = response {
                println!("Received {}B from: {}", packets.len(), peer_addr);
                self.torrent_file.write_packets(i, &packets).await?;
            }
        }
        Ok(())
    }
}
