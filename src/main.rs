use std::net::SocketAddr;
use std::str::from_utf8;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time;

const PACKET_SIZE: usize = 2;

pub struct Client {
    data: RwLock<Vec<u8>>,
    address: SocketAddr,
}

impl Client {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            data: RwLock::new(Vec::new()),
            address,
        }
    }

    pub async fn get_data(&self, from: usize, count: usize) -> Vec<u8> {
        let lock = self.data.read().await;

        let data_copied = lock[from..from + count].to_vec();
        data_copied
    }

    pub async fn set_data(&mut self, data: Vec<u8>) {
        let lock = self.data.get_mut();
        *lock = data;
    }

    pub async fn send(&self, to: SocketAddr) -> std::io::Result<()> {
        let mut stream = TcpStream::connect(to).await?;

        let packet = self.get_data(0, PACKET_SIZE).await;
        println!("Wrote {} bytes", stream.write(&packet).await?);

        Ok(())
    }

    async fn listen(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.address).await?;

        let mut packet_buffer = [0u8; PACKET_SIZE];

        while let Ok((stream, _)) = listener.accept().await {
            println!("Lis");
            let mut buffered_stream = BufStream::new(stream);

            while let Ok(bytes_read) = buffered_stream.read_exact(&mut packet_buffer).await {
                let mut lock = self.data.write().await;

                lock.extend_from_slice(&packet_buffer[..bytes_read]);

                println!("Data now: {:?}", from_utf8(&lock));
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let first_addr = SocketAddr::from_str("127.0.0.50:2137").unwrap();
    let second_addr = SocketAddr::from_str("127.0.0.51:2137").unwrap();

    let mut first_client = Client::new(first_addr);
    let mut second_client = Client::new(second_addr);

    first_client.set_data("Msg.".as_bytes().to_owned()).await;
    second_client.set_data("eae.".as_bytes().to_owned()).await;

    // First client setup
    let client_ptr = Arc::new(first_client);

    let _listener_handle_1 = tokio::spawn({
        let ptr_cloned = Arc::clone(&client_ptr);
        async move { ptr_cloned.listen().await }
    });

    let sender_handle_1 = tokio::spawn({
        let ptr_cloned = Arc::clone(&client_ptr);
        async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            ptr_cloned.send(second_addr).await
        }
    });

    // Second client setup
    let client_ptr = Arc::new(second_client);

    let _listener_handle_2 = tokio::spawn({
        let ptr_cloned = Arc::clone(&client_ptr);
        async move { ptr_cloned.listen().await }
    });

    let sender_handle_2 = tokio::spawn({
        let ptr_cloned = Arc::clone(&client_ptr);
        async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            ptr_cloned.send(first_addr).await
        }
    });

    let third_addr = SocketAddr::from_str("127.0.0.52:2137").unwrap();
    let mut third_client = Client::new(third_addr);
    third_client.set_data("ZZZ.".as_bytes().to_owned()).await;
    let client_ptr = Arc::new(third_client);

    let sender_handle_3 = tokio::spawn({
        let ptr_cloned = Arc::clone(&client_ptr);
        async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            ptr_cloned.send(first_addr).await
        }
    });

    sender_handle_1.await??;
    sender_handle_2.await??;
    sender_handle_3.await??;
    time::sleep(Duration::from_millis(200)).await;

    // returning there leaks a lot of memory by not shutting down listener threads

    Ok(())
}

#[cfg(test)]
mod test {}
