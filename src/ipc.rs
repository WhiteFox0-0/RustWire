#[derive(Debug)]
pub enum IPC {
    PeerConnected { peer_id: [u8; 20] },
    PeerDisconnected { peer_id: [u8; 20] },
    BlockDownloaded { peer_id: [u8; 20], index: u32, begin: u32, block: Vec<u8> },
    PieceHave { peer_id: [u8; 20], index: u32 },
    BitfieldReceived { peer_id: [u8; 20], bitfield: Vec<u8> },
}