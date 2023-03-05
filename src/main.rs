use std::net::SocketAddr;
use std::str::FromStr;
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
    let server_handle = tokio::spawn(async move { server.listen(&server_addr).await });

    let mut client = Client::new(client_addr);
    client.register_as_peer(&server_addr).await?;
    let sender_handle = tokio::spawn({
        async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            client.update_peerlist(&server_addr).await
        }
    });

    sender_handle.await??;
    server_handle.await??;
    Ok(())
}

#[cfg(test)]
mod test {
    
}