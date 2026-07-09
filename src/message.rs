#[derive(Debug, Clone)]
pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request { index: u32, begin: u32, length: u32 },
    Piece { index: u32, begin: u32, block: Vec<u8> },
    Cancel { index: u32, begin: u32, length: u32 },
}

impl Message {
    /// Serialize into wire format: <len_prefix><id><payload>
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        match self {
            Message::KeepAlive => {
                buf.extend_from_slice(&0u32.to_be_bytes());
            }
            Message::Choke => {
                buf.extend_from_slice(&1u32.to_be_bytes());
                buf.push(0);
            }
            Message::Unchoke => {
                buf.extend_from_slice(&1u32.to_be_bytes());
                buf.push(1);
            }
            Message::Interested => {
                buf.extend_from_slice(&1u32.to_be_bytes());
                buf.push(2);
            }
            Message::NotInterested => {
                buf.extend_from_slice(&1u32.to_be_bytes());
                buf.push(3);
            }
            Message::Have(index) => {
                buf.extend_from_slice(&5u32.to_be_bytes());
                buf.push(4);
                buf.extend_from_slice(&index.to_be_bytes());
            }
            Message::Bitfield(bytes) => {
                let len = 1 + bytes.len() as u32;
                buf.extend_from_slice(&len.to_be_bytes());
                buf.push(5);
                buf.extend_from_slice(bytes);
            }
            Message::Request { index, begin, length } => {
                buf.extend_from_slice(&13u32.to_be_bytes());
                buf.push(6);
                buf.extend_from_slice(&index.to_be_bytes());
                buf.extend_from_slice(&begin.to_be_bytes());
                buf.extend_from_slice(&length.to_be_bytes());
            }
            Message::Piece { index, begin, block } => {
                let len = 9 + block.len() as u32;
                buf.extend_from_slice(&len.to_be_bytes());
                buf.push(7);
                buf.extend_from_slice(&index.to_be_bytes());
                buf.extend_from_slice(&begin.to_be_bytes());
                buf.extend_from_slice(block);
            }
            Message::Cancel { index, begin, length } => {
                buf.extend_from_slice(&13u32.to_be_bytes());
                buf.push(8);
                buf.extend_from_slice(&index.to_be_bytes());
                buf.extend_from_slice(&begin.to_be_bytes());
                buf.extend_from_slice(&length.to_be_bytes());
            }
        }

        buf
    }

    /// Parse a message body (id byte + payload, length already consumed by caller).
    pub fn deserialize(payload: &[u8]) -> std::io::Result<Message> {
        if payload.is_empty() {
            return Ok(Message::KeepAlive);
        }

        let id = payload[0];
        let body = &payload[1..];

        let msg = match id {
            0 => Message::Choke,
            1 => Message::Unchoke,
            2 => Message::Interested,
            3 => Message::NotInterested,
            4 => Message::Have(u32::from_be_bytes(body[0..4].try_into().unwrap())),
            5 => Message::Bitfield(body.to_vec()),
            6 => Message::Request {
                index: u32::from_be_bytes(body[0..4].try_into().unwrap()),
                begin: u32::from_be_bytes(body[4..8].try_into().unwrap()),
                length: u32::from_be_bytes(body[8..12].try_into().unwrap()),
            },
            7 => Message::Piece {
                index: u32::from_be_bytes(body[0..4].try_into().unwrap()),
                begin: u32::from_be_bytes(body[4..8].try_into().unwrap()),
                block: body[8..].to_vec(),
            },
            8 => Message::Cancel {
                index: u32::from_be_bytes(body[0..4].try_into().unwrap()),
                begin: u32::from_be_bytes(body[4..8].try_into().unwrap()),
                length: u32::from_be_bytes(body[8..12].try_into().unwrap()),
            },
            other => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown message id {}", other),
                ));
            }
        };

        Ok(msg)
    }
}