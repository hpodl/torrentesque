use std::net::SocketAddr;

use serde_json;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use crate::requests::{RequestToTracker, TrackerResponse};
use std::str;

const PACKET_SIZE: usize = 2;

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
            .write(&serde_json::to_vec(&RequestToTracker::RegisterAsPeer).unwrap())
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
        let listener = TcpListener::bind(self.address).await?;

        let mut packet_buffer = [0u8; 1024];

        while let Ok((stream, _)) = listener.accept().await {
            println!("Client listening.");
            let mut buffered_stream = BufStream::new(stream);

            while let Ok(bytes_read) = buffered_stream.read(&mut packet_buffer).await {
                println!("Read {:?}", str::from_utf8(&packet_buffer[..bytes_read]));
            }
        }
        Ok(())
    }
}
