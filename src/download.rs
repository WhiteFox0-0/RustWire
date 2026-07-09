pub struct Download {
    pub num_pieces: usize,
    pub piece_length: u32,
    pub total_length: u64,
    pub info_hash: [u8; 20],
    pub my_bitfield: Vec<bool>,
    // Per-piece block completion: piece_index -> set of completed block offsets
    pub piece_blocks_done: Vec<std::collections::HashSet<u32>>,
}

impl Download {
    pub fn new(
        num_pieces: usize,
        piece_length: u32,
        total_length: u64,
        info_hash: [u8; 20],
    ) -> Self {
        Download {
            num_pieces,
            piece_length,
            total_length,
            info_hash,
            my_bitfield: vec![false; num_pieces],
            piece_blocks_done: vec![Default::default(); num_pieces],
        }
    }

    pub fn need_piece(&self, index: u32) -> bool {
        !self.my_bitfield.get(index as usize).copied().unwrap_or(false)
    }

    /// Length of a given piece, accounting for the possibly-shorter last piece.
    pub fn piece_len(&self, index: u32) -> u32 {
        if index as usize == self.num_pieces - 1 {
            let remainder = self.total_length % self.piece_length as u64;
            if remainder == 0 { self.piece_length } else { remainder as u32 }
        } else {
            self.piece_length
        }
    }
}