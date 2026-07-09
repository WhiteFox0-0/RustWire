use rustwire::{
    run_writer, PeerConnection, IPC, Download,
    parse_torrent, get_peers, PeerAddr,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState},
    Terminal,
};

use rand::RngExt;
use std::{
    fs,
    io,
    net::{IpAddr, SocketAddr, TcpStream},
    path::PathBuf,
    sync::mpsc::{channel, Receiver, Sender},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_LOG_LINES: usize = 500;

enum AppState {
    Selecting,
    Downloading,
}

struct App {
    torrent_files: Vec<PathBuf>,
    list_state: ListState,
    state: AppState,
    logs: Vec<String>,
    log_rx: Option<Receiver<String>>,
    progress_rx: Option<Receiver<(usize, usize)>>, // (pieces_done, total_pieces)
    pieces_done: usize,
    total_pieces: usize,
    should_quit: bool,
    selected_file: Option<String>,
}

impl App {
    fn new(torrent_files: Vec<PathBuf>) -> Self {
        let mut list_state = ListState::default();
        if !torrent_files.is_empty() {
            list_state.select(Some(0));
        }
        App {
            torrent_files,
            list_state,
            state: AppState::Selecting,
            logs: Vec::new(),
            log_rx: None,
            progress_rx: None,
            pieces_done: 0,
            total_pieces: 0,
            should_quit: false,
            selected_file: None,
        }
    }

    fn next(&mut self) {
        if self.torrent_files.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.torrent_files.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.torrent_files.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(0) | None => self.torrent_files.len() - 1,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(i));
    }

    fn start_download(&mut self) {
        let Some(i) = self.list_state.selected() else { return };
        let Some(path) = self.torrent_files.get(i).cloned() else { return };

        let (log_tx, log_rx) = channel::<String>();
        let (progress_tx, progress_rx) = channel::<(usize, usize)>();
        self.log_rx = Some(log_rx);
        self.progress_rx = Some(progress_rx);
        self.selected_file = Some(path.file_name().unwrap().to_string_lossy().to_string());
        self.state = AppState::Downloading;
        self.logs.clear();
        self.pieces_done = 0;
        self.total_pieces = 0;

        spawn_download_thread(path, log_tx, progress_tx);
    }

    fn push_log(&mut self, line: String) {
        self.logs.push(line);
        if self.logs.len() > MAX_LOG_LINES {
            self.logs.remove(0);
        }
    }

    fn drain_logs(&mut self) {
        let mut new_lines = Vec::new();
        if let Some(rx) = &self.log_rx {
            while let Ok(line) = rx.try_recv() {
                new_lines.push(line);
            }
        }
        for line in new_lines {
            self.push_log(line);
        }
    }

    fn drain_progress(&mut self) {
        let mut latest = None;
        if let Some(rx) = &self.progress_rx {
            while let Ok(update) = rx.try_recv() {
                latest = Some(update);
            }
        }
        if let Some((done, total)) = latest {
            self.pieces_done = done;
            self.total_pieces = total;
        }
    }
}

fn scan_torrent_files(dir: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("torrent") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn generate_peer_id() -> [u8; 20] {
    let mut peer_id = [0u8; 20];
    peer_id[..8].copy_from_slice(b"-RW0001-");
    rand::rng().fill(&mut peer_id[8..]);
    peer_id
}

fn spawn_peer(
    peer: PeerAddr,
    download_mutex: Arc<Mutex<Download>>,
    my_peer_id: [u8; 20],
    incoming_tx: Sender<IPC>,
) {
    std::thread::spawn(move || {
        let addr = SocketAddr::new(IpAddr::V4(peer.ip), peer.port);

        let stream = match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            Ok(s) => s,
            Err(_) => return, // silently drop dead peers; not worth a log line each
        };
        stream.set_read_timeout(Some(Duration::from_secs(120))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(10))).ok();

        let write_stream = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };

        let (outgoing_tx, outgoing_rx) = channel();
        std::thread::spawn(move || {
            run_writer(write_stream, outgoing_rx);
        });

        if let Ok(mut conn) =
            PeerConnection::new(stream, download_mutex, my_peer_id, incoming_tx, outgoing_tx)
        {
            conn.run();
        }
    });
}

/// Runs the whole parse -> tracker -> peer swarm pipeline on a dedicated
/// thread with its own Tokio runtime, streaming human-readable lines and
/// progress updates back to the UI thread.
fn spawn_download_thread(
    torrent_path: PathBuf,
    log_tx: Sender<String>,
    progress_tx: Sender<(usize, usize)>,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
        rt.block_on(async move {
            let path_str = torrent_path.to_string_lossy().to_string();
            log_tx.send(format!("Parsing {}", path_str)).ok();

            let metainfo = match parse_torrent(&path_str) {
                Ok(m) => m,
                Err(e) => {
                    log_tx.send(format!("ERROR: failed to parse torrent: {}", e)).ok();
                    return;
                }
            };

            let info_hash = metainfo.info_hash;
            let piece_length = metainfo.info.piece_length as u32;
            let total_length: u64 = metainfo.info.length.unwrap_or_else(|| {
                metainfo
                    .info
                    .files
                    .as_ref()
                    .map(|files| files.iter().map(|f| f.length).sum())
                    .unwrap_or(0)
            });
            let num_pieces = metainfo.info.pieces.len() / 20;

            log_tx.send(format!("Name: {}", metainfo.info.name)).ok();
            log_tx.send(format!("Total size: {} bytes", total_length)).ok();
            log_tx.send(format!("Pieces: {} ({} bytes each)", num_pieces, piece_length)).ok();

            progress_tx.send((0, num_pieces)).ok();

            let my_peer_id = generate_peer_id();
            let download = Download::new(num_pieces, piece_length, total_length, info_hash);
            let download_mutex = Arc::new(Mutex::new(download));

            log_tx.send("Contacting tracker...".to_string()).ok();
            let peers = match get_peers(&my_peer_id, &metainfo, 6881).await {
                Ok(p) => p,
                Err(e) => {
                    log_tx.send(format!("ERROR: tracker request failed: {}", e)).ok();
                    return;
                }
            };
            log_tx.send(format!("Tracker returned {} peers", peers.len())).ok();

            let (incoming_tx, incoming_rx) = channel::<IPC>();

            for peer in peers {
                spawn_peer(peer, download_mutex.clone(), my_peer_id, incoming_tx.clone());
            }
            drop(incoming_tx); // drop our own clone; threads hold the rest

            let mut pieces_done = 0usize;

            for event in incoming_rx {
                let line = match event {
                    IPC::PeerConnected { peer_id } => {
                        format!("[connected]    {}", hex_id(&peer_id))
                    }
                    IPC::PeerDisconnected { peer_id } => {
                        format!("[disconnected] {}", hex_id(&peer_id))
                    }
                    IPC::BitfieldReceived { peer_id, .. } => {
                        format!("[bitfield]     {}", hex_id(&peer_id))
                    }
                    IPC::PieceHave { peer_id, index } => {
                        format!("[have]         {} has piece {}", hex_id(&peer_id), index)
                    }
                    IPC::BlockDownloaded { peer_id, index, begin, block } => {
                        let block_len = block.len() as u32;
                        let mut dl = download_mutex.lock().unwrap();
                        dl.piece_blocks_done[index as usize].insert(begin);

                        // Approximate completion check: sum of recorded block
                        // offsets' worth of bytes vs. piece length. This assumes
                        // uniform block size and will be replaced once real disk
                        // I/O + SHA1 verification exists — that should be the
                        // actual source of truth for "piece done".
                        let piece_len = dl.piece_len(index);
                        let blocks_recorded = dl.piece_blocks_done[index as usize].len() as u32;
                        let covered = blocks_recorded * block_len;
                        let already_marked = dl.my_bitfield[index as usize];
                        if !already_marked && covered >= piece_len {
                            dl.my_bitfield[index as usize] = true;
                            pieces_done += 1;
                            progress_tx.send((pieces_done, num_pieces)).ok();
                        }
                        drop(dl);

                        format!(
                            "[block]        piece {:>5} begin {:>7} len {:>6} <- {}",
                            index, begin, block.len(), hex_id(&peer_id)
                        )
                    }
                };
                if log_tx.send(line).is_err() {
                    break; // UI thread gone
                }
            }

            log_tx.send("Swarm loop ended (no more peer threads active).".to_string()).ok();
        });
    });
}

fn hex_id(id: &[u8; 20]) -> String {
    id.iter().take(4).map(|b| format!("{:02x}", b)).collect::<String>()
}

fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    match app.state {
        AppState::Selecting => draw_selecting(frame, app),
        AppState::Downloading => draw_downloading(frame, app),
    }
}

fn draw_selecting(frame: &mut ratatui::Frame, app: &mut App) {
    let area = frame.area();

    let items: Vec<ListItem> = if app.torrent_files.is_empty() {
        vec![ListItem::new("(no .torrent files found in test_data/)")]
    } else {
        app.torrent_files
            .iter()
            .map(|p| {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(name),
                ]))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select a .torrent file — ↑/↓ move, Enter to download, q to quit "),
        )
        .highlight_style(Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_downloading(frame: &mut ratatui::Frame, app: &mut App) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let ratio = if app.total_pieces > 0 {
        (app.pieces_done as f64 / app.total_pieces as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let label = format!(
        "{} — {}/{} pieces ({:.1}%)",
        app.selected_file.as_deref().unwrap_or("?"),
        app.pieces_done,
        app.total_pieces,
        ratio * 100.0
    );

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Progress (q to quit) "))
        .gauge_style(
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .ratio(ratio)
        .label(label);
    frame.render_widget(gauge, chunks[0]);

    let log_area_height = chunks[1].height.saturating_sub(2) as usize;
    let start = app.logs.len().saturating_sub(log_area_height);
    let visible_logs: Vec<ListItem> = app.logs[start..]
        .iter()
        .map(|l| ListItem::new(l.as_str()))
        .collect();

    let log_list = List::new(visible_logs)
        .block(Block::default().borders(Borders::ALL).title(" Logs "));
    frame.render_widget(log_list, chunks[1]);
}

fn main() -> io::Result<()> {
    let torrent_files = scan_torrent_files("test_data");

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(torrent_files);
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| draw(f, &mut app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.state {
                        AppState::Selecting => match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                            KeyCode::Down => app.next(),
                            KeyCode::Up => app.previous(),
                            KeyCode::Enter => app.start_download(),
                            _ => {}
                        },
                        AppState::Downloading => {
                            if let KeyCode::Char('q') | KeyCode::Esc = key.code {
                                app.should_quit = true;
                            }
                        }
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.drain_logs();
            app.drain_progress();
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}