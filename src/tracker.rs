use serde_bytes::ByteBuf;
use serde_derive::Deserialize;
use tokio;

use crate::{error::TorrentError, query_params, torrent::Metainfo};

#[derive(Debug, Deserialize)]
pub struct TrackerResponse {
    pub interval: Option<u64>, // seconds between re-announces
    #[serde(rename = "min interval")]
    pub min_interval: Option<u64>, // optional, minimum allowed re-announce interval
    #[serde(rename = "tracker id")]
    pub tracker_id: Option<String>, // echo back on future requests, if present
    pub complete: Option<u64>, // number of seeders
    pub incomplete: Option<u64>, // number of leechers
    pub peers: Option<ByteBuf>, // compact format — raw bytes, decode via parse_compact_peers
    #[serde(rename = "failure reason")]
    pub failure_reason: Option<String>, // present instead of everything else if the request failed
    #[serde(rename = "warning message")]
    pub warning_message: Option<String>,
}

pub struct Peer {
    pub addr: PeerAddr,
    pub peer_id: Option<[u8; 20]>, // learned during handshake
    pub am_choking: bool,          // true initially — you start choking them
    pub am_interested: bool,       // false initially
    pub peer_choking: bool,        // true initially — assume choked until told otherwise
    pub peer_interested: bool,     // false initially
    pub bitfield: Option<Vec<u8>>, // which pieces they have
    pub stream: Option<tokio::net::TcpStream>,
}

#[derive(Debug, Clone, Copy)]
pub struct PeerAddr {
    pub ip: std::net::Ipv4Addr,
    pub port: u16,
}

pub fn parse_compact_peers(bytes: &[u8]) -> Vec<PeerAddr> {
    bytes
        .chunks_exact(6)
        .map(|chunk| {
            let ip = std::net::Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
            let port = u16::from_be_bytes([chunk[4], chunk[5]]);
            PeerAddr { ip, port }
        })
        .collect()
}

fn percent_encode_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' | b'~' => {
                (*b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

fn generate_peer_id() -> [u8; 20] {
    use rand::RngExt;
    let mut peer_id = [0u8; 20];
    peer_id[..8].copy_from_slice(b"-RW0001-");
    rand::rng().fill(&mut peer_id[8..]);
    peer_id
}

pub async fn get_peers(
    metainfo: &Metainfo,
    listener_port: u16,
) -> Result<Vec<PeerAddr>, TorrentError> {
    let total_size = metainfo.info.length.unwrap_or_else(|| {
        metainfo
            .info
            .files
            .as_ref()
            .map(|files| files.iter().map(|f| f.length).sum())
            .unwrap_or(0)
    });

    let info_hash = percent_encode_bytes(&metainfo.info_hash);
    let peer_id_bytes = generate_peer_id();
    let peer_id = percent_encode_bytes(&peer_id_bytes);

    let url = format!(
        "{}?{}",
        metainfo.announce,
        query_params!({
            info_hash: info_hash,
            peer_id: peer_id,
            port: listener_port.to_string(),
            uploaded: 0,
            downloaded: 0,
            left: total_size,
            compact: 1,
            event: "started"
        })
    );

    let response = reqwest::get(&url).await.map_err(TorrentError::Http)?;

    let bytes = response.bytes().await.map_err(TorrentError::Http)?;

    let tracker_response: TrackerResponse = serde_bencode::from_bytes(&bytes)?;

    if let Some(reason) = &tracker_response.failure_reason {
        return Err(TorrentError::TrackerFailure(reason.clone()));
    }

    let peers_bytes = tracker_response.peers.ok_or_else(|| {
        TorrentError::MissingPeers("no peers field in tracker response".to_string())
    })?;
    let peers = parse_compact_peers(&peers_bytes).into_iter().collect();

    Ok(peers)
}
