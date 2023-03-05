use std::net::SocketAddr;

use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use serde_json;

use crate::requests::{RequestToTracker, TrackerResponse};

const PACKET_SIZE: usize = 2;

pub struct Client {
    address: SocketAddr,
}

impl Client {
    pub fn new(address: SocketAddr) -> Self {
        Self { address }
    }
    pub async fn send(&self, to: SocketAddr) -> io::Result<()> {
        let mut stream = TcpStream::connect(to).await?;
        stream
            .write(&serde_json::to_vec(&RequestToTracker::GetPeers).unwrap())
            .await?;
        stream.write("\n".as_bytes()).await?;
        stream.flush().await?;

        let mut buf = [0u8; 1024];
        stream.read(&mut buf).await?;

        println!("{:?}", buf);

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

        let mut packet_buffer = [0u8; PACKET_SIZE];

        while let Ok((stream, _)) = listener.accept().await {
            println!("Client listening.");
            let mut buffered_stream = BufStream::new(stream);

            while let Ok(_bytes_read) = buffered_stream.read_exact(&mut packet_buffer).await {
                todo!();
            }
        }
        Ok(())
    }
}
