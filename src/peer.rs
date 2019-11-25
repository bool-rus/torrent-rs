use crate::message::{Handshake, PeerMessage, Bitfield, BitfieldError};
use bytes::Bytes;
use crate::{parser, Cache};
use std::io;
use async_std::net::{TcpStream, ToSocketAddrs};
use async_std::prelude::*;
use async_std::io::prelude::*;
use async_std::task;
use async_std::sync::{Arc, Sender, Receiver, RwLock, Mutex};
use futures_util::{AsyncReadExt, StreamExt, SinkExt, future::join};
use crate::io::*;

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
    #[fail(display = "Block not found")]
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
/// Handle for connected peer
///
pub struct Peer {
    queue: Arc<Mutex<u8>>,
    bitfield: Arc<RwLock<Vec<u8>>>,
    state: Arc<RwLock<(PeerState, PeerState)>>, //(me, remote)
    sender: Sender<PeerMessage>,
}

async fn handshake(stream: TcpStream, handshake: Handshake) -> Result<(impl Read, impl Write), PeerError> {
    let mut bytes: Bytes = handshake.clone().into();
    let (mut reader, mut writer) = stream.split();
    let (_, response) = join(
        writer.write_all(bytes.as_ref()),
        read_handshake(&mut reader)
    ).await;
    if !handshake.validate(&response?) {
        return Err(PeerError::Handshake);
    };
    Ok((reader, writer))
}

impl Peer {
    pub async fn new<R, W, C>(read: R, write: W, cache: C) -> Result<Self, PeerError>
        where R: Read + Send + Sync + Unpin + 'static, W: Write + Send + Sync + Unpin + 'static, C: Cache + Unpin {
        let (mut sender, receiver) = async_std::sync::channel(10);
        let peer = Peer {
            queue: Arc::new(Mutex::new(0)),
            bitfield: Arc::new(RwLock::new(vec![])),
            state: Arc::new(RwLock::new((PeerState::Chocked, PeerState::Unchocked))),
            sender: sender.clone(),
        };
        task::spawn(Self::sender(MessageSink::new(write), receiver));
        sender.send(PeerMessage::Bitfield(cache.bitfield().await)).await;
        task::spawn(peer.clone().daemon(MessageStream::from(read), cache));
        Ok(peer)
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
            self.sender.send(PeerMessage::Request { block, offset, length }).await;
            Ok(())
        }
    }
    async fn daemon<S, C>(mut self, mut stream: S, cache: C) -> Result<(), PeerError>
        where S: Stream<Item=PeerMessage> + Unpin, C: Cache + Unpin {
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
                    let interest = cache.bitfield().await
                        .interest(&bitfield).unwrap_or(vec![]);
                    let interest = interest.iter().find(|&x| *x > 0);
                    let mut response;
                    if interest.is_some() {
                        response = PeerMessage::Interested;
                    } else {
                        response = PeerMessage::NotInterested;
                    }
                    *self.bitfield.write().await = bitfield;
                    Some(response)
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
                    *queue = if *queue > 0 { *queue - 1 } else { 0 };
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

#[cfg(test)]
mod test {
    use super::*;
    use async_std::net::TcpListener;
    use crate::*;
    use std::thread;

    fn make_h1() -> Handshake {
        Handshake {
            protocol: "123456789012345678901234567890123".to_string(),
            extentions: ['e' as u8; 8],
            info_hash: ['i' as u8; 20].into(),
            peer_id: ['p' as u8; 20].into(),
        }
    }

    fn make_h2() -> Handshake {
        Handshake {
            protocol: "12345".to_string(),
            extentions: [1; 8],
            info_hash: [4; 20].into(),
            peer_id: [2; 20].into(),
        }
    }

    async fn listen(h: Handshake) -> Result<(impl Read, impl Write), PeerError> {
        let mut listener = TcpListener::bind("127.0.0.1:64343").await?;
        let (stream, _) = listener.accept().await?;
        let res = handshake(stream, h).await;
        res
    }

    async fn connect(h: Handshake) -> Result<(impl Read, impl Write), PeerError> {
        let stream = TcpStream::connect("127.0.0.1:64343").await?;
        let res = handshake(stream, h).await;
        res
    }

    #[test]
    fn test_connect_handshake_ok() -> Result<(), PeerError> {
        let th = thread::spawn(|| {
            task::block_on(listen(make_h1()))
        });
        task::block_on(connect(make_h1()))?;
        th.join().unwrap()?;
        Ok(())
    }
}