use bytes::Bytes;
pub use bip_metainfo::{Info as TorrentInfo, Metainfo, InfoHash, InfoHash as PeerId, InfoHash as NodeId};

pub use super::error::Error;
use std::sync::Arc;
use async_std::net::ToSocketAddrs;
pub use crate::message::TorrentExtentions;
use crate::message::Handshake;

#[derive(Clone)]
pub struct TorrentConfig {
    pub peer_id: PeerId,
    pub protocol: String,
    pub extentions: TorrentExtentions,
}

impl TorrentConfig {
    pub fn make_handshake(&self, info_hash: InfoHash) -> Handshake {
        Handshake {
            protocol: self.protocol.clone(),
            extentions: self.extentions.clone(),
            info_hash,
            peer_id: self.peer_id.clone()
        }
    }
}

#[async_trait]
pub trait Cache : Clone + Send + Sync + 'static {
    async fn put(&self, index: u32, offset: u32, bytes: Bytes) ;
    async fn bitfield(&self) -> Vec<u8>;
    async fn get_piece(&self, block: u32, offset: u32, length: u32) -> Option<Bytes> ;
}

#[async_trait]
impl<T: Cache> Cache for Arc<T> {
    async fn put(&self, index: u32, offset: u32, bytes: Bytes) {
        (*self).put(index, offset, bytes).await
    }

    async fn bitfield(&self) -> Vec<u8> {
        (*self).bitfield().await
    }

    async fn get_piece(&self, block: u32, offset: u32, length: u32) -> Option<Bytes> {
        (*self).get_piece(block, offset, length).await
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