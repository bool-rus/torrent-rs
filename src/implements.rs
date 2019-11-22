use super::*;
use std::collections::HashMap;
use futures_util::future::{AbortHandle, Abortable, AbortRegistration};
use crate::peer::Peer;


pub struct Connection<T: Cache> {
    cache: T,
    peers: Vec<Peer>,
}

impl<T: Cache> Connection<T> {
    async fn new(cache: T, info: TorrentInfo, abort: AbortRegistration) -> Result<Connection<T>, Error> {
        unimplemented!()
    }
    async fn download(&self, index: u32) -> bool {
        unimplemented!();
    }
}

pub struct Service {
    started: HashMap<InfoHash, AbortHandle>,
}


pub struct Handle;

impl Handle {
    async fn download(&self, index: usize) -> Bytes {

        unimplemented!()
    }
}

impl Service {
    fn start<C: Cache>(&mut self, cache: C, info: TorrentInfo) -> Handle {
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