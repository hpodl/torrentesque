use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::time;

const PACKET_SIZE: usize = 2;

pub struct Client {
    data: Vec<u8>,
}

impl Client {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn data(&self) -> &[u8] {
        self.data.as_ref()
    }

    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
    }

    async fn send(&self) -> std::io::Result<()> {
        let mut stream = TcpStream::connect("localhost:4877").await?;

        for &byte in &self.data {
            println!("Wrote {} bytes", stream.write(&[byte]).await?);
        }
        Ok(())
    }

    async fn listen(&mut self) -> std::io::Result<()> {
        let listener = TcpListener::bind("localhost:4877").await?;

        let mut packet_buffer = [0u8; PACKET_SIZE];

        while let Ok((stream, _)) = listener.accept().await {
            let mut buffered_stream = BufStream::new(stream);

            while let Ok(bytes_read) = buffered_stream.read_exact(&mut packet_buffer).await {
                self.data.extend_from_slice(&packet_buffer[..bytes_read]);
            }

            println!("Data now: {:?}", self.data);
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut sender = Client::new();
    sender.set_data("Message.".as_bytes().to_owned());

    let listener_handle = tokio::spawn(async {
        let mut listener = Client::new();
        listener.listen().await
    });

    time::sleep(Duration::from_millis(100)).await;

    sender.send().await?;
    sender.send().await?;

    listener_handle.await??;
    Ok(())
}

#[cfg(test)]
mod test {}
