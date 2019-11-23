use crate::message::{Handshake, PeerMessage, Bitfield, BitfieldError};
use bytes::Bytes;
use crate::{parser, Cache, CacheClient};
use std::io;
use async_std::net::{TcpStream};
use async_std::prelude::*;
use async_std::io::prelude::*;
use async_std::task;
use async_std::sync::{Arc, Sender, Receiver, RwLock, Mutex};
use futures_util::{AsyncReadExt, StreamExt};
use futures_util::SinkExt;
use crate::io::*;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Fail)]
pub enum PeerError {
    #[fail(display = "{}", 0)]
    IoError(io::Error),
    #[fail(display = "{}", 0)]
    Simple(String),
    #[fail(display = "Handshake error")]
    Handshake,
    #[fail(display = "Peer is busy")]
    PeerIsBusy,
    #[fail(display = "{}", 0)]
    Bitfield(BitfieldError),
    #[fail(display="Block not found")]
    BlockNotFound,
}

impl From<io::Error> for PeerError {
    fn from(e: io::Error) -> Self {
        PeerError::IoError(e)
    }
}

impl From<BitfieldError> for PeerError {
    fn from(e: BitfieldError) -> Self {
        PeerError::Bitfield(e)
    }
}

enum PeerState {
    Chocked,
    Unchocked,
}

#[derive(Clone)]
pub struct Peer {
    queue: Arc<Mutex<u8>>,
    bitfield: Arc<RwLock<Vec<u8>>>,
    state: Arc<RwLock<(PeerState, PeerState)>>, //(me, remote)
    sender: Sender<PeerMessage>,
}

impl Peer {
    pub async fn new(stream: TcpStream, cache: CacheClient, handshake: Handshake) -> Result<Self, PeerError> {
        let mut bytes: Bytes = handshake.clone().into();
        let (mut reader, mut writer) = stream.split();
        writer.write_all(bytes.as_ref()).await?;
        let response = super::io::read_handshake(&mut reader).await?;
        if !handshake.validate(&response) {
            return Err(PeerError::Handshake);
        };
        let (sender, receiver) = async_std::sync::channel(10);
        let peer = Peer {
            queue: Arc::new(Mutex::new(0)),
            bitfield: Arc::new(RwLock::new(vec![])),
            state: Arc::new(RwLock::new((PeerState::Chocked, PeerState::Unchocked))),
            sender: sender.clone(),
        };
        task::spawn(Self::sender(MessageSink::new(writer), receiver));
        task::spawn(peer.clone().daemon(MessageStream::from(reader), cache));
        Ok(peer)
    }
    pub async fn have(&self, block: u32) -> bool {
        self.bitfield.read().await.have_bit(block)
    }
    pub async fn request(&self, block: u32, offset: u32, length: u32) -> Result<(), PeerError> {
        {
            if !self.bitfield.read().await.have_bit(block) {
                return Err(PeerError::BlockNotFound);
            }
        }
        let mut queue = self.queue.lock().await;
        if *queue > 5 {
            Err(PeerError::PeerIsBusy)
        } else {
            *queue += 1;
            self.sender.send(PeerMessage::Request {block, offset, length}).await;
            Ok(())
        }
    }
    async fn daemon<S>(mut self, mut stream: S, cache: CacheClient) -> Result<(), PeerError>
        where S : Stream<Item=PeerMessage> + Unpin {
        //implement keepalive timer
        while let Some(msg) = stream.next().await {
            use PeerMessage::*;
            let resp = match msg {
                KeepAlive => Some(KeepAlive),
                Choke => {
                    let mut state = &mut *self.state.write().await;
                    state.0 = PeerState::Chocked;
                    None
                }
                Unchoke => {
                    let mut state = &mut *self.state.write().await;
                    state.0 = PeerState::Unchocked;
                    None
                }
                Interested => None,
                NotInterested => None,
                Have(bit) => {
                    self.bitfield.write().await.add_bit(bit)?;
                    None
                }
                Bitfield(bitfield) => {
                    *self.bitfield.write().await = bitfield;
                    None
                }
                Request { block, offset, length } => {
                    use PeerState::*;
                    match *self.state.read().await {
                        (Unchocked, Unchocked) => match cache.get_piece(block, offset, length).await {
                            None => None,
                            Some(data) => Some(Piece { block, offset, data }),
                        },
                        _ => None
                    }
                }
                Piece { block, offset, data } => {
                    cache.put(block, offset, data).await;
                    let mut queue = self.queue.lock().await;
                    *queue = if *queue > 0 { *queue -1 } else {0};
                    None
                }
                Cancel { .. } => {
                    //TODO: implement by spec
                    None
                }
                Port(port) => { unimplemented!() }
            };
            if let Some(resp) = resp {
                self.sender.send(resp).await;
            }
        }
        Ok(())
    }

    async fn sender<W: Write + Unpin>(mut sink: MessageSink<W>, receiver: Receiver<PeerMessage>) -> Result<(), PeerError> {
        //implement keepalive timer
        while let Some(msg) = receiver.recv().await {
            sink.send(msg).await?;
        }
        Ok(())
    }
}
