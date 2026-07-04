use rustwire::{TorrentError, get_peers, parse_torrent};

#[tokio::main]
async fn main() -> Result<(), TorrentError> {
    let metainfo = parse_torrent(
        "/Users/apple/workspace/RustWire/test_data/ubuntu-26.04-desktop-amd64.iso.torrent",
    )?;
    println!("Announce: {}", metainfo.announce);
    let peers = get_peers(&metainfo, 6881).await?;

    println!("Got {} peers:", peers.len());
    for peer in &peers {
        println!("  {}:{}", peer.ip, peer.port);
    }

    Ok(())
}
