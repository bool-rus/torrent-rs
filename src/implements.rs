use super::*;
use std::collections::HashMap;
use futures_util::future::{AbortHandle, Abortable, AbortRegistration};
use crate::peer::{PeerHandle, PeerError};
use std::net::IpAddr;
use async_std::sync::{Arc, RwLock};
use async_std::task;
use async_std::net::TcpStream;
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
    handshake: Handshake,
    cache: T,
    handles: Arc<RwLock<HashMap<PeerId, PeerHandle>>>,
}
unsafe impl<T:Send> Send for Connection<T> {}
unsafe impl<T:Sync> Sync for Connection<T> {}

impl<T: Cache + Unpin> Connection<T> {
    fn new<A>(cache: T, peers: Vec<Peer>, listen: A, info_hash: InfoHash, abort: AbortRegistration) -> Self {
        let handles = Arc::new(RwLock::new(Default::default()));
        let handshake = Handshake {
            protocol: "".to_string(), //TODO implement it
            extentions: [1u8;8], //TODO implement it
            info_hash,
            peer_id: [1;20].into() //TODO implement it
        };
        let connection = Connection {
            handshake: handshake.clone(),
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

        let handle =  PeerHandle::new(stream, self.cache.clone(), self.handshake.clone()).await?;
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
}

pub struct Service {
    started: HashMap<InfoHash, AbortHandle>,
}



impl Service {
    fn start<C: Cache>(&mut self, cache: C, info: TorrentInfo) {
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        self.started.insert(info.info_hash(), abort_handle);
        //Connection::new(cache, info, abort_registration);
        unimplemented!()
    }
    fn running(&self) -> Vec<&InfoHash> {
        self.started.keys().into_iter().collect()
    }
    fn stop(&mut self, hash: &InfoHash) -> bool {
        if let Some(handle) = self.started.remove(hash) {
            handle.abort();
            true
        } else {
            false
        }
    }
}