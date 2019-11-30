use super::*;
use std::collections::HashMap;
use futures_util::future::{AbortHandle, Abortable, AbortRegistration};
use crate::peer::{PeerHandle, PeerError};
use std::net::{IpAddr, TcpListener};
use async_std::sync::{Arc, RwLock};
use async_std::task;
use async_std::net::{TcpStream, ToSocketAddrs};
use async_std::io::prelude::*;
use futures_util::AsyncReadExt;
use crate::message::Handshake;

#[derive(Debug,PartialEq)]
pub struct Peer {
    id: Option<PeerId>,
    ip: IpAddr,
    port: u16,
}

#[derive(Clone)]
pub struct Connection<T> {
    config: TorrentConfig,
    info_hash: InfoHash,
    cache: T,
    handles: Arc<RwLock<HashMap<PeerId, PeerHandle>>>,
}
unsafe impl<T:Send> Send for Connection<T> {}
unsafe impl<T:Sync> Sync for Connection<T> {}

impl<T: Cache + Unpin> Connection<T> {
    fn new(cache: T, peers: Vec<Peer>, info_hash: InfoHash, config: TorrentConfig) -> Self {
        let handles = Arc::new(RwLock::new(Default::default()));

        let connection = Connection {
            config,
            info_hash,
            cache: cache.clone(),
            handles: handles.clone()
        };
        for peer in peers {
            let connection = connection.clone();
            task::spawn(async move {
                if let Ok(stream) = TcpStream::connect((peer.ip, peer.port)).await {
                    connection.add_peer(stream).await; //TODO must use
                };
            });
        }
        connection
    }
    pub async fn add_peer<S>(&self, stream: S) -> Result<(), PeerError>
    where S: Read + Write + Unpin + Send + Sync + 'static {
        let handles = self.handles.write().await;

        let handle =  PeerHandle::new(
            stream,
            self.cache.clone(),
            self.config.make_handshake(self.info_hash)
        ).await?;
        self.handles.write().await.insert(handle.get_id(), handle);
        Ok(())
    }
    pub async fn download(&self, block: u32, offset: u32, length: u32) -> bool {
        for p in self.handles.read().await.values() {
            if p.request(block, offset, length).await.is_ok() {
                return true
            }
        }
        false
    }
    pub fn stop(&self) {
        unimplemented!()
    }
}
