use std::cell::RefCell;
use std::net::SocketAddr;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time;

const PACKET_SIZE: usize = 2;

pub struct Client {
    data: Mutex<RefCell<Vec<u8>>>,
    address: SocketAddr,
}

impl Client {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            data: Mutex::new(RefCell::new(Vec::new())),
            address,
        }
    }

    pub async fn get_data(&self, from: usize, count: usize) -> Vec<u8> {
        println!("GetData waiting for lock");
        let lock = self.data.lock().await;
        println!("GetData got the lock");

        let data_copied = lock.deref().borrow()[from..from + count].to_vec();
        data_copied
    }

    pub async fn set_data(&mut self, data: Vec<u8>) {
        let lock = self.data.get_mut();
        *lock.deref().borrow_mut() = data;
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
            let mut buffered_stream = BufStream::new(stream);

            while let Ok(bytes_read) = buffered_stream.read_exact(&mut packet_buffer).await {
                println!("Listener waiting for lock");
                let lock = self.data.lock().await;
                println!("Listener got the lock");
                lock.deref()
                    .borrow_mut()
                    .extend_from_slice(&packet_buffer[..bytes_read]);
            }

            println!("Data now: {:?}", self.data);
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

    sender_handle_1.await??;
    sender_handle_2.await??;

    Ok(())
}

#[cfg(test)]
mod test {}
