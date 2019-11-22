use bytes::Bytes;
pub use bip_metainfo::{Info as TorrentInfo, InfoHash, InfoHash as PeerId, InfoHash as NodeId};

pub use super::error::Error;
use async_std::sync::{Sender, Receiver};

#[async_trait]
pub trait Cache : Clone + Send {
    async fn put(self, index: u32, offset: u32, bytes: Bytes) -> Self ;
    async fn get_block(self, block: u32) -> Option<Bytes> ;
    async fn get_piece(self, block: u32, offset: u32, length: u32) -> bool ;
}

pub enum CacheRequest {
    Put { index: u32, offset: u32, bytes: Bytes },
    GetBlock { block: u32, callback: Sender<Option<Bytes>>},
    GetPiece { block: u32, offset: u32, length: u32, callback: Sender<Option<Bytes>> }
}

pub struct CacheClient {
    sender: Sender<CacheRequest>,
    callback: Sender<Option<Bytes>>,
    receiver: Receiver<Option<Bytes>>,
}

impl CacheClient {
    pub fn new(sender: Sender<CacheRequest>) -> Self {
        let (callback,receiver) = async_std::sync::channel(1);
        CacheClient { sender, callback, receiver }
    }
    pub async fn put(&self, index: u32, offset: u32, bytes: Bytes) {
        self.sender.send(CacheRequest::Put{index, offset, bytes}).await;
    }

    pub async fn get_block(&self, block: u32) -> Option<Bytes> {
        let callback = self.callback.clone();
        self.sender.send(CacheRequest::GetBlock {block, callback}).await;
        self.receiver.recv().await.unwrap()
    }

    pub async fn get_piece(&self, block: u32, offset: u32, length: u32) -> Option<Bytes> {
        let callback = self.callback.clone();
        self.sender.send(CacheRequest::GetPiece {block, offset, length, callback}).await;
        self.receiver.recv().await.unwrap()
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use std::sync::Arc;
    use async_std::task::{block_on, spawn};
    use async_std::io::prelude::*;

    fn is_send<T: Send>(obj: T) -> T {obj}

    fn use_cache<T: Write + Unpin>(mut w: T) {
        let f = async move {
            w.write_all(&b"bgg"[..]);
        };
        //let f = is_send(f);
        //spawn(f);
    }
}