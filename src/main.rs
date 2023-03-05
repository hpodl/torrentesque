use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

use client::Client;
use server::Server;
use tokio::sync::oneshot;
use tokio::time::sleep;

mod client;
mod requests;
mod server;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let client_addr = SocketAddr::from_str("127.0.0.166:5468").unwrap();
    let client2_addr = SocketAddr::from_str("127.0.0.167:7846").unwrap();
    let server_addr = SocketAddr::from_str("127.0.0.168:11256").unwrap();

    let mut server = Server::new();
    let server_handle = tokio::spawn(async move { server.listen(&server_addr).await });
    sleep(Duration::from_millis(250)).await;


    let mut client = Client::new(client_addr);
    let leech_handle = tokio::spawn({
        async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            client.update_peerlist(&server_addr).await
        }
    });

    let (wx, rx) = oneshot::channel::<()>();
    let client2 = Client::new(client2_addr);
    client2.register_as_peer(&server_addr).await?;
    let seed_handle = tokio::spawn(async move { client2.listen(rx).await });



    leech_handle.await??;
    server_handle.await??;
    
    wx.send(()).unwrap();
    seed_handle.await??;

    Ok(())
}

#[cfg(test)]
mod test {}
