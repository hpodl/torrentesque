use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time;

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
            .write_all(&serde_json::to_vec(&RequestToTracker::GetPeers)?)
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

    /// Registers as a peer at `tracker_addr` tracker
    pub async fn register_as_peer(&self, tracker_addr: &SocketAddr) -> io::Result<()> {
        let mut stream = TcpStream::connect(tracker_addr).await?;
        stream
            .write_all(&serde_json::to_vec(&RequestToTracker::RegisterAsPeer(
                self.address,
            ))?)
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

        while let Ok((mut stream, _)) = listener.accept().await {
            while let Ok(bytes_read) = stream.read(&mut packet_buffer).await {
                if bytes_read == 0 {
                    break;
                }

                match serde_json::from_slice::<LeechRequest>(&packet_buffer[..bytes_read]) {
                    Ok(LeechRequest::GetAvailability) => {
                        stream
                            .write_all(&serde_json::to_vec(&SeedResponse::Availability(
                                self.torrent_file.read_packet_availability().await,
                            ))?)
                            .await?;
                    }
                    Ok(LeechRequest::GetPackets(start, count)) => {
                        let data = self.torrent_file.read_packets(start, count).await?;
                        stream.write_all(&data).await?;
                    }
                    _ => {
                        stream
                            .write_all(&serde_json::to_vec(&SeedResponse::InvalidRequest)?)
                            .await?;
                    }
                };

                stream.flush().await?;
            }
        }
        Ok(())
    }

    /// Launches the leech loop, which stops when a message is passed through `shutdown_channel`
    pub async fn leech_loop(
        &self,
        tracker_addr: &SocketAddr,
        shutdown_channel: oneshot::Receiver<()>,
    ) -> io::Result<()> {
        tokio::select! {
            _ = shutdown_channel => { self.save_progress().await },
            res = self.do_leech_loop(tracker_addr) => { res },
        }
    }

    /// Actual `leech_loop` body
    async fn do_leech_loop(&self, tracker_addr: &SocketAddr) -> io::Result<()> {
        let mut buf = vec![0u8; self.torrent_file.packet_size()];
        let packet_availability = self.torrent_file.read_packet_availability().await;

        for (i, _) in packet_availability
            .iter()
            .enumerate()
            .filter(|(_, available)| !available)
        {
            let mut stream = self.peer_stream_with_packet(i, tracker_addr).await?;

            stream
                .write_all(&serde_json::to_vec(&LeechRequest::GetPackets(i, 1))?)
                .await?;

            // Packets are read raw (instead of having their own serializable enum entry) to save bandwidth
            let bytes_read = stream.read(&mut buf).await?;
            println!(
                "[{}]: Packet {i} - got {} bytes from {}",
                self.address,
                bytes_read,
                stream.peer_addr()?
            );

            self.torrent_file
                .write_packets(i, &buf[..bytes_read])
                .await?;
        }
        Ok(())
    }

    pub async fn save_progress(&self) -> io::Result<()> {
        self.torrent_file.save_progress_to_file().await
    }

    /// Loops until it finds a peer with `packet_index` packet available and returns a TcpStream to that peer
    async fn peer_stream_with_packet(
        &self,
        packet_index: usize,
        tracker_addr: &SocketAddr,
    ) -> io::Result<TcpStream> {
        let peerlist = {
            let mut peerlist = self.request_peerlist(tracker_addr).await?;

            while peerlist.is_empty() {
                time::sleep(Duration::from_millis(50)).await;
                peerlist = self.request_peerlist(tracker_addr).await?;
            }
            peerlist
        };

        let peer_count = peerlist.len();
        let mut seed = peer_count * packet_index;

        // Code taken from `https://github.com/rust-lang/rust/blob/6a179026decb823e6ad8ba1c81729528bc5d695f/library/core/src/slice/sort.rs#L677`
        // Pseudorandom number generator from the "Xorshift RNGs" paper by George Marsaglia.
        let mut gen_usize = || {
            if usize::BITS <= 32 {
                let mut r = seed as u32;
                r ^= r << 13;
                r ^= r >> 17;
                r ^= r << 5;
                seed = r as usize;
                seed
            } else {
                let mut r = seed as u64;
                r ^= r << 13;
                r ^= r >> 7;
                r ^= r << 17;
                seed = r as usize;
                seed
            }
        };

        loop {
            let peer_addr = peerlist[gen_usize() % peer_count];
            let mut stream = TcpStream::connect(peer_addr).await?;

            let seed_response = {
                let mut availability_buf = vec![0u8; self.torrent_file.packet_size() / 8];
                stream
                    .write_all(&serde_json::to_vec(&LeechRequest::GetAvailability)?)
                    .await?;
                let bytes_read = stream.read(&mut availability_buf).await?;
                serde_json::from_slice::<SeedResponse>(&availability_buf[..bytes_read])
            }?;

            if let SeedResponse::Availability(availability) = seed_response {
                match availability.get(packet_index) {
                    Some { 0: true } => break Ok(stream), // is Some(true) - peer has the packet
                    _ => continue,
                }
            }
        }
    }
}
