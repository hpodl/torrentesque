use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{self};

pub struct Client {}

impl Client {
    pub fn new() -> Self {
        Self {}
    }

    async fn send(&mut self) -> std::io::Result<()> {
        println!("Sending");
        let mut stream = TcpStream::connect("localhost:4877").await?;
        stream.write("Abcdef\n".as_bytes()).await?;
        Ok(())
    }

    async fn listen(&mut self) -> std::io::Result<()> {
        let listener = TcpListener::bind("localhost:4877").await?;

        while let Ok((stream, _)) = listener.accept().await {
            let buffered_stream = BufStream::new(stream);

            let mut lines = buffered_stream.lines();
            while let Ok(maybe_line) = lines.next_line().await {
                match maybe_line {
                    Some(line) => println!("{}", line),
                    None => break,
                }
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut sender = Client::new();

    let listener_handle = tokio::spawn(async {
        let mut listener = Client::new();
        listener.listen().await
    });
    time::sleep(Duration::from_millis(100)).await;

    for _ in 1..10 {
        sender.send().await?;
    }

    listener_handle.await??;
    Ok(())
}
