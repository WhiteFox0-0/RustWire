use crate::download::Download;
use crate::ipc::IPC;
use crate::message::Message;

use std::collections::HashMap;
use std::io::{Read, Write, ErrorKind};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

const PROTOCOL: &[u8] = b"BitTorrent protocol";
const BLOCK_SIZE: u32 = 16 * 1024;
const PIPELINE_DEPTH: usize = 5;

#[derive(Clone)]
pub struct PeerMetadata {
    pub peer_id: [u8; 20],
    pub choked: bool,       // true = they are choking us
    pub choking: bool,      // true = we are choking them
    pub interested: bool,   // true = they are interested in us
    pub interesting: bool,  // true = we are interested in them
    pub bitfield: Vec<bool>,
}

impl PeerMetadata {
    fn new(peer_id: [u8; 20], num_pieces: usize) -> Self {
        PeerMetadata {
            peer_id,
            choked: true,
            choking: true,
            interested: false,
            interesting: false,
            bitfield: vec![false; num_pieces],
        }
    }
}

pub struct PeerConnection {
    halt: bool,
    download_mutex: Arc<Mutex<Download>>,
    stream: TcpStream,
    me: PeerMetadata,
    them: PeerMetadata,
    incoming_tx: Sender<IPC>,
    outgoing_tx: Sender<Message>,
    upload_in_progress: bool,
    to_request: HashMap<(u32, u32), (u32, u32, u32)>,
}


impl PeerConnection {
    pub fn new(
        mut stream: TcpStream,
        download_mutex: Arc<Mutex<Download>>,
        my_peer_id: [u8; 20],
        incoming_tx: Sender<IPC>,
        outgoing_tx: Sender<Message>,
    ) -> std::io::Result<PeerConnection> {
        let (info_hash, num_pieces) = {
            let dl = download_mutex.lock().unwrap();
            (dl.info_hash, dl.num_pieces)
        };

        let their_peer_id = Self::handshake(&mut stream, &info_hash, &my_peer_id)?;

        let me = PeerMetadata::new(my_peer_id, num_pieces);
        let them = PeerMetadata::new(their_peer_id, num_pieces);

        incoming_tx.send(IPC::PeerConnected { peer_id: their_peer_id }).ok();

        Ok(PeerConnection {
            halt: false,
            download_mutex,
            stream,
            me,
            them,
            incoming_tx,
            outgoing_tx,
            upload_in_progress: false,
            to_request: HashMap::new(),
        })
    }

    fn handshake(
        stream: &mut TcpStream,
        info_hash: &[u8; 20],
        my_peer_id: &[u8; 20],
    ) -> std::io::Result<[u8; 20]> {
        let mut handshake = Vec::with_capacity(68);
        handshake.push(PROTOCOL.len() as u8);
        handshake.extend_from_slice(PROTOCOL);
        handshake.extend_from_slice(&[0u8; 8]);
        handshake.extend_from_slice(info_hash);
        handshake.extend_from_slice(my_peer_id);
        stream.write_all(&handshake)?;

        let mut response = [0u8; 68];
        stream.read_exact(&mut response)?;

        let pstrlen = response[0] as usize;
        if pstrlen != PROTOCOL.len() || &response[1..20] != PROTOCOL {
            return Err(std::io::Error::new(ErrorKind::InvalidData, "bad protocol string"));
        }
        if &response[28..48] != info_hash {
            return Err(std::io::Error::new(ErrorKind::InvalidData, "info_hash mismatch"));
        }

        let mut peer_id = [0u8; 20];
        peer_id.copy_from_slice(&response[48..68]);
        Ok(peer_id)
    }

    /// Main blocking read loop. Run on its own thread.
    pub fn run(&mut self) {
        while !self.halt {
            match self.read_message() {
                Ok(msg) => self.handle_message(msg),
                Err(e) => {
                    eprintln!("peer {:?} error: {}", self.them.peer_id, e);
                    break;
                }
            }
        }
        self.incoming_tx.send(IPC::PeerDisconnected { peer_id: self.them.peer_id }).ok();
    }

    fn read_message(&mut self) -> std::io::Result<Message> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf);

        if len == 0 {
            return Ok(Message::KeepAlive);
        }

        let mut payload = vec![0u8; len as usize];
        self.stream.read_exact(&mut payload)?;
        Message::deserialize(&payload)
    }

    fn handle_message(&mut self, msg: Message) {
        match msg {
            Message::Choke => {
                self.them.choking = true;
            }
            Message::Unchoke => {
                self.them.choking = false;
                self.fill_requests();
            }
            Message::Interested => {
                self.them.interested = true;
            }
            Message::NotInterested => {
                self.them.interested = false;
            }
            Message::Have(index) => {
                if let Some(slot) = self.them.bitfield.get_mut(index as usize) {
                    *slot = true;
                }
                self.incoming_tx
                    .send(IPC::PieceHave { peer_id: self.them.peer_id, index })
                    .ok();
                self.maybe_send_interested();
            }
            Message::Bitfield(bytes) => {
                for i in 0..self.them.bitfield.len() {
                    let byte = bytes.get(i / 8).copied().unwrap_or(0);
                    let bit = (byte >> (7 - (i % 8))) & 1;
                    self.them.bitfield[i] = bit == 1;
                }
                self.incoming_tx
                    .send(IPC::BitfieldReceived { peer_id: self.them.peer_id, bitfield: bytes })
                    .ok();
                self.maybe_send_interested();
            }
            Message::Request { index, begin, length } => {
                if !self.me.choking {
                    self.upload_in_progress = true;
                    // Hook into disk layer to actually read + send the block:
                    // let block = read_block_from_disk(index, begin, length);
                    // self.outgoing_tx.send(Message::Piece { index, begin, block }).ok();
                }
            }
            Message::Piece { index, begin, block } => {
                self.to_request.remove(&(index, begin));
                self.incoming_tx
                    .send(IPC::BlockDownloaded {
                        peer_id: self.them.peer_id,
                        index,
                        begin,
                        block,
                    })
                    .ok();
                self.fill_requests();
            }
            Message::Cancel { .. } => {
                // remove matching queued upload if you track outbound pieces
            }
            Message::KeepAlive => {}
        }
    }

    fn maybe_send_interested(&mut self) {
        let want = self.them.bitfield.iter().enumerate().any(|(i, &have)| {
            have && self.download_mutex.lock().unwrap().need_piece(i as u32)
        });
        if want && !self.me.interesting {
            self.me.interesting = true;
            self.outgoing_tx.send(Message::Interested).ok();
        } else if !want && self.me.interesting {
            self.me.interesting = false;
            self.outgoing_tx.send(Message::NotInterested).ok();
        }
    }

    fn fill_requests(&mut self) {
        if self.them.choking {
            return;
        }
        while self.to_request.len() < PIPELINE_DEPTH {
            match self.next_block_to_request() {
                Some((index, begin, length)) => {
                    self.to_request.insert((index, begin), (index, begin, length));
                    self.outgoing_tx.send(Message::Request { index, begin, length }).ok();
                }
                None => break,
            }
        }
    }

    fn next_block_to_request(&self) -> Option<(u32, u32, u32)> {
        let dl = self.download_mutex.lock().unwrap();

        for (i, &has_it) in self.them.bitfield.iter().enumerate() {
            if !has_it || !dl.need_piece(i as u32) {
                continue;
            }
            let index = i as u32;
            let piece_len = dl.piece_len(index);
            let done = &dl.piece_blocks_done[i];

            let mut begin = 0u32;
            while begin < piece_len {
                if !done.contains(&begin) && !self.to_request.contains_key(&(index, begin)) {
                    let length = BLOCK_SIZE.min(piece_len - begin);
                    return Some((index, begin, length));
                }
                begin += BLOCK_SIZE;
            }
        }
        None
    }

    pub fn halt(&mut self) {
        self.halt = true;
    }
}

/// Run on its own thread with a cloned write-half of the peer's socket.
pub fn run_writer(mut write_stream: TcpStream, outgoing_rx: Receiver<Message>) {
    for msg in outgoing_rx {
        let bytes = msg.serialize();
        if let Err(e) = write_stream.write_all(&bytes) {
            eprintln!("write error, dropping peer connection: {}", e);
            break;
        }
    }
}