use super::*;
use bytes::{Bytes, BytesMut, BufMut};

pub type TorrentExtentions = [u8; 8];

pub const SIZE_BYTES: usize = 4;
const PORT_BYTES: usize = 2;
pub(crate) const HANDSHAKE_DEFAULT_SIZE: usize = 49;

#[derive(Debug, Fail)]
#[fail(display = "{}", 0)]
pub struct BitfieldError(String);

pub trait Bitfield: AsRef<[u8]> + Sized {
    fn empty(count: u32) -> Self;
    fn full(count: u32) -> Self;
    fn add_bit(&mut self, index: u32) -> Result<(), BitfieldError>;
    fn remove_bit(&mut self, index: u32) -> Result<(), BitfieldError>;
    fn have_bit(&self, index: u32) -> bool;
    fn interest<T: Bitfield>(&self, rhs: T) -> Result<Self, BitfieldError>;
}

impl Bitfield for Vec<u8> {
    fn empty(blocks_count: u32) -> Self {
        let mut capacity = blocks_count / 8;
        if blocks_count % 8 != 0 {
            capacity += 1;
        }
        let mut ret = Vec::with_capacity(capacity as usize);
        for _ in 0..capacity {
            ret.push(0u8);
        }
        ret
    }

    fn full(blocks_count: u32) -> Self {
        let mut overhead = (blocks_count % 8) as u8;
        if overhead > 0 {
            overhead = 8 - overhead;
        }
        let mut capacity = blocks_count / 8;
        if overhead > 0 {
            capacity += 1;
        }

        let mut ret = Vec::with_capacity(capacity as usize);
        for i in 1..capacity { //все, кроме последнего
            ret.push(0xffu8);
        }
        ret.push(0xffu8 << overhead);
        ret
    }

    fn add_bit(&mut self, index: u32) -> Result<(), BitfieldError> {
        let (byte_index, mask) = calc_byte_index_and_mask(index);
        let byte = self.get_mut(byte_index).unwrap();
        *byte = *byte | mask;
        Ok(())
    }

    fn remove_bit(&mut self, index: u32) -> Result<(), BitfieldError> {
        let (byte_index, mask) = calc_byte_index_and_mask(index);
        let mask = 0xffu8 ^ mask; //example: 11101111
        let byte = self.get_mut(byte_index).unwrap();
        *byte = *byte & mask;
        Ok(())
    }

    fn have_bit(&self, index: u32) -> bool {
        let (byte_index, mask) = calc_byte_index_and_mask(index);
        (self.get(byte_index).unwrap_or(&0u8).to_owned() & mask) > 0
    }

    fn interest<T: Bitfield>(&self, rhs: T) -> Result<Self, BitfieldError> {
        let me: &[u8] = self.as_ref();
        let another = rhs.as_ref();
        if me.len() != another.len() {
            Err(BitfieldError("Different sizes of Bitfields".to_string()))
        } else {
            Ok(me.iter()
                .zip(another.iter())
                .map(|(&a, &b)| (a & b) ^ b)
                .collect()
            )
        }
    }
}

fn calc_byte_index_and_mask(index: u32) -> (usize, u8){
    let offset = (index % 8) as u8;
    let mask = 1u8 << 7u8 - offset; //example: 00010000
    ((index / 8) as usize, mask)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Handshake {
    pub protocol: String,
    pub extentions: TorrentExtentions,
    pub info_hash: InfoHash,
    pub peer_id: PeerId,
}

impl Into<Bytes> for Handshake {
    fn into(self) -> Bytes {
        let protocol = self.protocol.as_bytes();
        let size = protocol.len();
        let mut ret = BytesMut::with_capacity(size + HANDSHAKE_DEFAULT_SIZE);
        ret.put_u8(size as u8);
        ret.put(protocol);
        ret.put(self.extentions.as_ref());
        ret.put(self.info_hash.as_ref());
        ret.put(self.peer_id.as_ref());
        ret.into()
    }
}

impl Handshake {
    pub fn validate(&self, another: &Handshake) -> bool {
        self.info_hash.eq(&another.info_hash)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request {
        block: u32,
        offset: u32,
        length: u32,
    },
    Piece {
        block: u32,
        offset: u32,
        data: Bytes,
    },
    Cancel {
        block: u32,
        offset: u32,
        length: u32,
    },
    Port(u16),
}

fn make_empty_message(message_id: u8) -> Bytes {
    let size = 1;
    let mut ret = BytesMut::with_capacity(size + SIZE_BYTES);
    let size = size as u32;
    ret.put_u32_be(size);
    ret.put(message_id);
    ret.into()
}

impl Into<Bytes> for PeerMessage {
    fn into(self) -> Bytes {
        match self {
            PeerMessage::KeepAlive => Bytes::from([0u8;4].as_ref()),
            PeerMessage::Choke => make_empty_message(b'0'),
            PeerMessage::Unchoke => make_empty_message(b'1'),
            PeerMessage::Interested => make_empty_message(b'2'),
            PeerMessage::NotInterested => make_empty_message(b'3'),
            PeerMessage::Have(index) => {
                let size = 1 + SIZE_BYTES;
                let mut ret = BytesMut::with_capacity(size + SIZE_BYTES);
                ret.put_u32_be(size as u32);
                ret.put(b'4');
                ret.put_u32_be(index);
                ret.into()
            }
            PeerMessage::Bitfield(bitfield) => {
                let body: &[u8] = bitfield.as_ref();
                let size = 1 + body.len();
                let mut ret = BytesMut::with_capacity(size + SIZE_BYTES);
                ret.put_u32_be(size as u32);
                ret.put(b'5');
                ret.put(body);
                ret.into()
            }
            PeerMessage::Request { block, offset, length } => {
                let size = 1 + 3 * SIZE_BYTES;
                let mut ret = BytesMut::with_capacity(size + SIZE_BYTES);
                ret.put_u32_be(size as u32);
                ret.put(b'6');
                ret.put_u32_be(block);
                ret.put_u32_be(offset);
                ret.put_u32_be(length);
                ret.into()
            }
            PeerMessage::Piece { block, offset, data } => {
                let size = 1 + 2 * SIZE_BYTES + data.len();
                let mut ret = BytesMut::with_capacity(size + SIZE_BYTES);
                ret.put_u32_be(size as u32);
                ret.put(b'7');
                ret.put_u32_be(block);
                ret.put_u32_be(offset);
                ret.put(data);
                ret.into()
            }
            PeerMessage::Cancel { block, offset, length } => {
                let size = 1 + 3 * SIZE_BYTES;
                let mut ret = BytesMut::with_capacity(size + SIZE_BYTES);
                ret.put_u32_be(size as u32);
                ret.put(b'8');
                ret.put_u32_be(block);
                ret.put_u32_be(offset);
                ret.put_u32_be(length);
                ret.into()
            }
            PeerMessage::Port(port) => {
                let size = 1 + PORT_BYTES;
                let mut ret = BytesMut::with_capacity(size + SIZE_BYTES);
                ret.put_u32_be(size as u32);
                ret.put(b'9');
                ret.put_u16_be(port);
                ret.into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn test_empty_messages() {
        let bytes: Bytes = PeerMessage::KeepAlive.into();
        assert_eq!([0u8, 0, 0, 0].as_ref(), bytes.as_ref());
        let bytes: Bytes = PeerMessage::Choke.into();
        assert_eq!([0u8, 0, 0, 1, b'0'].as_ref(), bytes.as_ref());
        let bytes: Bytes = PeerMessage::Unchoke.into();
        assert_eq!([0u8, 0, 0, 1, b'1'].as_ref(), bytes.as_ref());
        let bytes: Bytes = PeerMessage::Interested.into();
        assert_eq!([0u8, 0, 0, 1, b'2'].as_ref(), bytes.as_ref());
        let bytes: Bytes = PeerMessage::NotInterested.into();
        assert_eq!([0u8, 0, 0, 1, b'3'].as_ref(), bytes.as_ref());
    }

    #[test]
    fn test_simple_messages() {
        let bytes: Bytes = PeerMessage::Have(0x342f21cc).into();
        assert_eq!([0u8, 0, 0, 5, b'4', 0x34, 0x2f, 0x21, 0xcc].as_ref(), bytes.as_ref());
    }

    #[test]
    fn test_bit_ops() {
        let offset = 1 % 8;
        let mask = 1u8 << 7 - offset;
        assert_eq!(0b01000000, mask);

        let mask = 0xffu8 ^ mask;
        assert_eq!(0b10111111, mask);

        let a = 0b11100011u8;
        let b = 0b00111001u8;
        assert_eq!(0b00100001u8, a & b);
        assert_eq!(0b00011000u8, (a & b) ^ b);
    }

    #[test]
    fn test_bitfield() {
        let bitfield = Vec::full(16);
        assert_eq!([0b11111111, 0b11111111], bitfield.as_ref());
        let mut bitfield = Vec::full(19);
        assert_eq!([0b11111111, 0b11111111, 0b11100000], bitfield.as_ref());
        bitfield.remove_bit(10).unwrap(); //помним, что нумерация с нуля
        assert_eq!([255u8, 0b11011111, 0b11100000], bitfield.as_ref());
        let mut bitfield = Vec::empty(20);
        assert_eq!([0u8, 0, 0], bitfield.as_ref());
        bitfield.add_bit(15).unwrap();
        assert_eq!([0u8, 1, 0], bitfield.as_ref());

        let a = vec![0b00000000u8, 0b00011100, 0b11100011];
        let b = vec![0b11100011u8, 0b00011100, 0b00111001];
        assert_eq!(vec![0b11100011u8, 0b00000000, 0b00011000], a.interest(b).unwrap())
    }
}