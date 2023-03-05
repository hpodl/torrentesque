use std::net::SocketAddr;
use std::str::FromStr;

use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::io::BufWriter;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::net::ToSocketAddrs;

use serde_json;

use crate::requests::{RequestToTracker, TrackerResponse};

pub struct Server {
    peerlist: Vec<SocketAddr>,
}

impl Server {
    pub fn new() -> Self {
        Self { peerlist: vec![] }
    }

    pub async fn listen<T>(&mut self, addr: &T) -> io::Result<()>
    where
        T: ToSocketAddrs,
    {
        let listener = TcpListener::bind(addr).await?;
        while let Ok((mut stream, client_addr)) = listener.accept().await {
            let (reader, writer) = stream.split();
            let reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);

            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                println!("{:?}", line);
                let response = {
                    if let Ok(request) = serde_json::from_str::<RequestToTracker>(&line) {
                        match request {
                            RequestToTracker::GetPeers => {
                                TrackerResponse::Peers(self.peerlist.clone())
                            }
                            RequestToTracker::RegisterAsPeer => {
                                self.peerlist.push(client_addr);
                                TrackerResponse::Ok
                            }
                        }
                    } else {
                        TrackerResponse::InvalidRequest
                    }
                };

                writer.write(&serde_json::to_vec(&response)?).await?;
                writer.flush().await?;
                break;
            }
        }
        Ok(())
    }
}
