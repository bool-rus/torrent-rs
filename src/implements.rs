use super::*;
use std::collections::HashMap;
use futures_util::future::{AbortHandle, Abortable, AbortRegistration};
use crate::peer::{Peer, PeerError};


pub struct Connection<T: Cache> {
    cache: T,
    peers: Vec<Peer>,
}

impl<T: Cache> Connection<T> {
    async fn new(cache: T, info: TorrentInfo, abort: AbortRegistration) -> Result<Connection<T>, Error> {
        unimplemented!()
    }
    async fn download(&self, block: u32, offset: u32, length: u32) -> bool {
        for p in self.peers.iter() {
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
        Connection::new(cache, info, abort_registration);
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