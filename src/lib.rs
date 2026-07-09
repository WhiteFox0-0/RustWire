mod error;
mod torrent;
mod tracker;
mod peer_connection;
mod message;
mod ipc;
mod download;

pub use torrent::FromBencode;
pub use torrent::Metainfo;
pub use torrent::parse_torrent;

pub use tracker::Peer;
pub use tracker::PeerAddr;
pub use tracker::TrackerResponse;
pub use tracker::get_peers;

pub use download::Download;

pub use peer_connection::PeerConnection;

pub use ipc::IPC;

pub use error::TorrentError;

#[macro_export]
macro_rules! query_params {
    ({ $($key: ident : $val: expr), * $(,)? }) => ({
        let mut param_string: Vec<String> = Vec::new();
        $(
            param_string.push(format!("{}={}", stringify!($key), $val));
        )*
        param_string.join("&")
    })
}
