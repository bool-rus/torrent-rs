use crate::message::{PeerMessage, Handshake};
use futures_util::sink::Sink;
use async_std::task::{Context, Poll};
use failure::_core::pin::Pin;
use bytes::{Bytes, BytesMut};
use async_std::io::prelude::*;
use async_std::sync::{Sender, Receiver};
use futures_util::stream::Stream;
use crate::parser;

enum State<T> {
    Done,
    Busy(T, usize)
}

pub struct MessageStream<R> {
    reader: R,
    state: State<Vec<u8>>,
}

impl<R: Read + Unpin> From<R> for MessageStream<R> {
    fn from(reader: R) -> Self {
        MessageStream { reader, state: State::Done, }
    }
}

impl<R: Read + Unpin> Stream for MessageStream<R> {
    type Item = PeerMessage;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        use super::message::SIZE_BYTES;
        use super::parser::*;
        let unpin = self.get_mut();
        let MessageStream {reader, state} = unpin;
        match state {
            State::Done => {
                let mut buf = [0u8; SIZE_BYTES];
                match R::poll_read( Pin::new(reader), cx, &mut buf ) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(len)) if len == SIZE_BYTES => {
                        let size = parse_u32(&buf) as usize;
                        *state = State::Busy(vec![0u8; size], 0);
                        Self::poll_next(Pin::new(unpin), cx)
                    },
                    Poll::Ready(Ok(len)) => {
                        *state = State::Busy(buf.to_vec(), len);
                        Poll::Pending
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(None),
                }
            },
            State::Busy(bytes, cursor) => {
                let buf = &mut bytes.as_mut_slice()[*cursor..];
                match R::poll_read( Pin::new(reader), cx, buf ) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(len)) if len == buf.len() => {
                        let (_, msg) = parse_message(bytes, bytes.len() as u32).unwrap();
                        *state = State::Done;
                        Poll::Ready(Some(msg))
                    }
                    Poll::Ready(Ok(len)) => {
                        *cursor += len;
                        Poll::Pending
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(None)
                }
            }
        }
    }
}

pub struct MessageSink<W> {
    writer: W,
    state: State<Bytes>,
}

impl<W> MessageSink<W> {
    pub fn new(writer: W) -> Self {
        MessageSink { writer, state: State::Done }
    }
}

impl<W: Write + Unpin> Sink<PeerMessage> for MessageSink<W> {
    type Error = std::io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.state {
            State::Done => {Poll::Ready(Ok(()))},
            State::Busy(_, _) => {Poll::Pending},
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: PeerMessage) -> Result<(), Self::Error> {
        match self.state {
            State::Done => {
                self.state = State::Busy(item.into(), 0);
                Ok(())
            },
            State::Busy(_, _) => {Err(unimplemented!())},
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let Self{writer, state} = self.get_mut();
        match state {
            State::Done => {Poll::Ready(Ok(()))},
            State::Busy(obj, offset) => {
                let obj = obj.as_ref();
                let obj = &obj[*offset..];
                match Pin::new(writer).poll_write(cx, obj) {
                    Poll::Ready(res) => {
                        match res {
                            Ok(size) if size == obj.len() => {
                                *state = State::Done;
                                Poll::Ready(Ok(()))
                            },
                            Ok(size) => {
                                *offset += size;
                                Poll::Pending
                            }
                            Err(e) => {Poll::Ready(Err(e))},
                        }
                    },
                    Poll::Pending => {Poll::Pending},
                }
            },
        }

    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.state {
            State::Done => { Pin::new(&mut self.writer).poll_close(cx) },
            State::Busy(_, _) => {Poll::Pending},
        }
    }
}


pub async fn read_handshake<T: ReadExt + Unpin>(read: &mut T) -> Result<Handshake, std::io::Error> {
    let mut buf = [0u8];
    read.read_exact(&mut buf).await?;
    let [protocol_size] = buf;
    let mut buf = BytesMut::with_capacity(super::message::HANDSHAKE_DEFAULT_SIZE - 1 + protocol_size as usize);
    read.read_exact(buf.as_mut()).await?;
    let (_, handshake) = parser::parse_handshake(buf.as_ref(), protocol_size).unwrap();
    Ok(handshake)
}

#[derive(Clone)]
pub struct MessageChannel<Q,A>(Sender<Q>, Receiver<A>);

impl<Q, A> MessageChannel<Q, A> {
    pub fn new() -> (MessageChannel<A,Q>, MessageChannel<Q,A>) {
        Self::with_capacity(10)
    }
    pub fn with_capacity(cap: usize) -> (MessageChannel<A,Q>, MessageChannel<Q,A>) {
        use async_std::sync::channel;
        let (req_s, req_r) = channel(cap);
        let (res_s, res_r) = channel(cap);
        (MessageChannel(res_s, req_r), MessageChannel(req_s, res_r))
    }
    pub async fn recv(&self) -> Option<A> {
        self.1.recv().await
    }
    pub async fn send(&self, msg: Q) {
        self.0.send(msg).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_std::task;
    use bytes::{BytesMut, BufMut};
    use nom::AsBytes;
    use futures_util::StreamExt;

    #[test]
    fn test_message_channel() {
        task::block_on(async {
            let (mine, their) = MessageChannel::new();
            mine.send("bgg").await;
            assert_eq!(their.recv().await, Some("bgg"));
            their.send(2).await;
            assert_eq!(mine.recv().await, Some(2));
        })
    }
    fn as_read<R: Read>(r:R) -> impl Read { r }
    #[test]
    fn test_stream() {
        let mut buf = BytesMut::with_capacity(1024usize);
        let messages = vec![
            PeerMessage::KeepAlive,
            PeerMessage::Request {
                block: 1,
                offset: 5,
                length: 40
            },
            PeerMessage::Bitfield(vec![1,2,3,4,5,6,7]),
            PeerMessage::Interested,
            PeerMessage::Unchoke,
            PeerMessage::Piece {
                block: 5,
                offset: 23,
                data: vec![1u8,23,3,13,45,1,34,23,52,10].into()
            }
        ];
        messages.iter().for_each(|msg| {buf.put(msg.clone())});
        let read = as_read(buf.as_bytes());
        let mut stream = MessageStream::from(read);
        task::block_on(async move {
            for msg in messages {
                let item = stream.next().await.unwrap();
                assert_eq!(item, msg );
            };
        })

    }
}