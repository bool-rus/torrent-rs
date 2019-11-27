use crate::message::*;
use bytes::Bytes;
use crate::{Cache};
use std::io;
use async_std::prelude::*;
use async_std::io::prelude::*;
use async_std::task;
use async_std::sync::{Arc, Sender, Receiver, RwLock, Mutex};
use futures_util::{StreamExt, SinkExt, future::join};
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


impl Peer {
    async fn do_handshake<R, W>(reader: &mut R, writer: &mut W, handshake: Handshake) -> Result<(), PeerError>
        where R: Read + Send + Sync + Unpin + 'static,
              W: Write + Send + Sync + Unpin + 'static {
        let mut bytes: Bytes = handshake.clone().into();
        let (_, response) = join(
            writer.write_all(bytes.as_ref()),
            read_handshake(reader)
        ).await;
        if !handshake.validate(&response?) {
            return Err(PeerError::Handshake);
        };
        Ok(())
    }
    pub async fn new<R, W, C>(mut read: R, mut write: W, cache: C, handshake: Handshake) -> Result<Self, PeerError>
        where R: Read + Send + Sync + Unpin + 'static,
              W: Write + Send + Sync + Unpin + 'static, C: Cache + Unpin {
        Self::do_handshake(&mut read, &mut write, handshake).await;
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
    use async_std::net::{TcpListener, TcpStream};
    use futures_util::{AsyncWriteExt, AsyncReadExt};
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
    fn make_h1_2() -> Handshake {
        Handshake {
            protocol: "bnakldsfygn askjfysnfgdfklsjdfshj".to_string(),
            extentions: ['x' as u8; 8],
            info_hash: ['i' as u8; 20].into(),
            peer_id: ['z' as u8; 20].into(),
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


    #[test]
    fn test_connect_handshake_ok() -> Result<(), PeerError> {
        let (me, remote) = MessageChannel::<u8,u8>::with_capacity(1024);
        let th = thread::spawn(move || {
            let (mut r,mut w) = remote.split();
            task::block_on( Peer::do_handshake(&mut r, &mut w, make_h1()))
        });
        let (mut r, mut w) = me.split();
        task::block_on(Peer::do_handshake(&mut r, &mut w, make_h1_2()))?;
        th.join().unwrap()?;
        Ok(())
    }
    #[test]
    fn test_connect_handshake_err() -> Result<(), PeerError> {
        let (me, remote) = MessageChannel::<u8,u8>::with_capacity(1024);
        let th = thread::spawn(move || {
            let (mut r,mut w) = remote.split();
            task::block_on( Peer::do_handshake(&mut r, &mut w, make_h2()))
        });
        let (mut r, mut w) = me.split();
        let result = task::block_on(Peer::do_handshake(&mut r, &mut w, make_h1()));
        match result {
            Err(PeerError::Handshake) => {
                //good err
            },
            Ok(_) => unreachable!(),
            obj => {obj?;}
        }
        match th.join().unwrap() {
            Err(PeerError::Handshake) => {
                //good err
            },
            Ok(_) => unreachable!(),
            obj => {obj?;}
        }
        Ok(())
    }


}