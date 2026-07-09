# RustWire

A BitTorrent client implemented in Rust, with a terminal UI for selecting `.torrent` files and watching downloads progress in real time.

## 

<!-- Add your demo video here -->

[https://github.com/user-attachments/assets/PLACEHOLDER](https://github.com/user-attachments/assets/52feb482-fbc1-4b4d-afa5-43f2d9c3f791)

<!-- Or embed a local file / GIF instead: -->
<!-- ![demo](./demo.gif) -->

## Features

- Parses `.torrent` files (bencode metadata, info hash computation)
- Announces to trackers and retrieves peer lists
- BitTorrent peer wire protocol (handshake, choke/interested, piece requests)
- Terminal UI (built with `ratatui` + `crossterm`):
  - Browse `.torrent` files in `test_data/`
  - Select a file and start a download
  - Live progress bar showing pieces downloaded
  - Scrolling log of peer/swarm activity

## Usage

```bash
cargo run --bin rustwire
```

- `↑` / `↓` — navigate torrent file list
- `Enter` — select a torrent and start downloading
- `q` / `Esc` — quit

## License
MIT License.

<!-- Add license info here -->
