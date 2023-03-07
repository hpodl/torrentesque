use std::net::SocketAddr;

use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::net::ToSocketAddrs;
use tokio::select;
use tokio::sync::oneshot;

use crate::requests::{RequestToTracker, TrackerResponse};

pub struct Server {
    peerlist: Vec<SocketAddr>,
}

impl Server {
    pub fn new() -> Self {
        Self { peerlist: vec![] }
    }

    pub async fn listen<T>(
        &mut self,
        addr: &T,
        shutdown_channel: oneshot::Receiver<()>,
    ) -> io::Result<()>
    where
        T: ToSocketAddrs,
    {
        select! {
            res = shutdown_channel => {Ok(res.unwrap())},
            res = self.do_listen(addr) => {res},
        }
    }

    pub async fn do_listen<T>(&mut self, addr: &T) -> io::Result<()>
    where
        T: ToSocketAddrs,
    {
        let listener = TcpListener::bind(addr).await?;
        while let Ok((mut stream, _)) = listener.accept().await {
            let (reader, mut writer) = stream.split();
            let reader = BufReader::new(reader);

            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let response = {
                    match serde_json::from_str::<RequestToTracker>(&line) {
                        Ok(RequestToTracker::GetPeers) => {
                            TrackerResponse::Peers(self.peerlist.clone())
                        }
                        Ok(RequestToTracker::RegisterAsPeer(client_addr)) => {
                            self.peerlist.push(client_addr);
                            TrackerResponse::Ok
                        }

                        _ => TrackerResponse::InvalidRequest,
                    }
                };

                writer.write_all(&serde_json::to_vec(&response)?).await?;
            }
        }
        Ok(())
    }
}
