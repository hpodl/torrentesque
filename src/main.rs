use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use client::Client;
use server::Server;

mod client;
mod requests;
mod server;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let client_addr = SocketAddr::from_str("127.0.0.50:2137").unwrap();
    let server_addr = SocketAddr::from_str("127.0.0.51:2137").unwrap();

    let mut server = Server::new();
    let server_handle = tokio::spawn(async move { server.listen(server_addr).await });

    let client = Client::new(client_addr);
    let client_ptr = Arc::new(client);
    let sender_handle = tokio::spawn({
        let ptr_cloned = Arc::clone(&client_ptr);
        async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            ptr_cloned.send(server_addr).await
        }
    });

    sender_handle.await??;
    server_handle.await??;
    Ok(())
}

#[cfg(test)]
mod test {}
