use thiserror::Error;

#[derive(Debug, Error)]
pub enum TorrentError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("bencode error: {0}")]
    Bencode(#[from] serde_bencode::Error),

    #[error("torrent file is not a bencoded dictionary")]
    NotADictionary,

    #[error("missing 'info' key")]
    MissingInfoKey,

    #[error("info dict has both 'length' and 'files'")]
    BothLengthAndFiles,

    #[error("info dict has neither 'length' nor 'files'")]
    NeitherLengthNorFiles,

    #[error("'pieces' length {0} is not a multiple of 20")]
    InvalidPiecesLength(usize),

    #[error("a file entry has an empty path list")]
    EmptyFilePath,

    #[error("'files' list is empty")]
    EmptyFilesList,

    #[error("'piece length' is zero")]
    ZeroPieceLength,

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("tracker returned failure: {0}")]
    TrackerFailure(String),

    #[error("tracker response missing peers field: {0}")]
    MissingPeers(String),
}
