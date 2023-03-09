use std::net::SocketAddr;

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

                match serde_json::from_slice::<LeechRequest>(&packet_buffer[..bytes_read]) {
                    Ok(LeechRequest::GetAvailability) => todo!(),
                    Ok(LeechRequest::GetPackets(start, count)) => {
                        let data = self.torrent_file.get_packets(start, count).await?;
                        buffered_stream.write_all(&data).await?;
                    }
                    _ => {
                        buffered_stream
                            .write_all(&serde_json::to_vec(&SeedResponse::InvalidRequest).unwrap())
                            .await?;
                    }
                };

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

        let mut seed = len;

        // Pseudorandom number generator from the "Xorshift RNGs" paper by George Marsaglia.
        // Code taken `https://github.com/rust-lang/rust/blob/6a179026decb823e6ad8ba1c81729528bc5d695f/library/core/src/slice/sort.rs#L677`
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
        let mut buf = vec![0u8; self.torrent_file.packet_size()];
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
            println!("Got {} bytes", bytes_read);

            self.torrent_file.write_packets(i, &buf[..bytes_read]).await?;

        }
        Ok(())
    }
}
