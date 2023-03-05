use std::net::SocketAddr;

use serde_json;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use crate::requests::{LeechRequest, RequestToTracker, SeedResponse, TrackerResponse};

pub struct Client {
    address: SocketAddr,
    peerlist: Vec<SocketAddr>,
}

impl Client {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            peerlist: vec![],
        }
    }
    pub async fn update_peerlist(&mut self, tracker_addr: &SocketAddr) -> io::Result<()> {
        let mut stream = TcpStream::connect(tracker_addr).await?;
        stream
            .write(&serde_json::to_vec(&RequestToTracker::GetPeers).unwrap())
            .await?;
        stream.write("\n".as_bytes()).await?;
        stream.flush().await?;

        let mut buf = [0u8; 1024];
        if let Ok(bytes_read) = stream.read(&mut buf).await {
            if let Ok(request) = serde_json::from_slice::<TrackerResponse>(&buf[..bytes_read]) {
                match request {
                    TrackerResponse::Peers(peers) => {
                        self.peerlist = peers;
                    }
                    _ => {
                        println!("Invalid request.")
                    }
                }
            }
        }

        println!("{:?}", self.peerlist);
        Ok(())
    }

    pub async fn register_as_peer(&self, tracker_addr: &SocketAddr) -> io::Result<()> {
        let mut stream = TcpStream::connect(tracker_addr).await?;
        stream
            .write(&serde_json::to_vec(&RequestToTracker::RegisterAsPeer(self.address)).unwrap())
            .await?;
        stream.write("\n".as_bytes()).await?;
        stream.flush().await?;

        Ok(())
    }

    pub async fn listen(&self, mut shutdown_channel: oneshot::Receiver<()>) -> io::Result<()> {
        tokio::select! {
                err = self.do_listen() => err,
                _ = &mut shutdown_channel => {
                    println!("Shutting down");
                    Ok(())
            }
        }
    }

    async fn do_listen(&self) -> io::Result<()> {
        let data = "Message.".as_bytes();
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
                        Ok(LeechRequest::GetPackets(from, count)) => {
                            SeedResponse::Packets(&data[from..from + count])
                        }
                        _ => SeedResponse::InvalidRequest,
                    };
                buffered_stream
                    .write(&serde_json::to_vec(&response).unwrap())
                    .await?;
                println!("Responding with {:?}", response);
            }
        }
        Ok(())
    }
}
