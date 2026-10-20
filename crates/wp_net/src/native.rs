//! Native transport (feature `native`; tokio + tungstenite). Never reachable from `wp_wasm`.
//!
//! - [`spawn_game_loop`]: a dedicated thread that owns a [`ServerInstance`], ticks it at
//!   60 Hz against the wall clock, and exchanges frames with the IO tasks over channels.
//! - [`serve_ws`]: accepts websocket connections; every binary message is one frame. Both
//!   [`Channel`]s map onto the one TCP stream (reliable), as the prototype allows.
//! - [`WsTransport`] / [`connect_ws`]: a client-side [`Transport`] (used by `--bots`).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::netsim::NetSim;
use crate::server::{ConnId, ServerInstance, ServerStats};
use crate::{Channel, NetError, Transport};

/// What the IO side sends to the game loop.
#[derive(Debug)]
pub enum IoEvent {
    /// A websocket finished its handshake.
    Open {
        /// IO-side socket id.
        sock: u64,
        /// Peer address.
        peer: SocketAddr,
        /// Where to put frames for this socket.
        tx: mpsc::UnboundedSender<OutFrame>,
    },
    /// A binary frame arrived.
    Frame {
        /// Socket.
        sock: u64,
        /// Payload.
        bytes: Vec<u8>,
    },
    /// The socket closed.
    Close {
        /// Socket.
        sock: u64,
    },
}

/// What the game loop sends to one socket.
#[derive(Debug)]
pub enum OutFrame {
    /// Send this frame.
    Frame(Vec<u8>),
    /// Close the socket.
    Close,
}

/// Live status published by the game loop (admin endpoint).
#[derive(Debug, Clone, Default)]
pub struct StatusReport {
    /// Instance counters.
    pub stats: ServerStats,
    /// `(player id, name)`.
    pub players: Vec<(u32, String)>,
    /// Measured net ticks per second over the last report interval.
    pub tick_rate_hz: f64,
    /// Mean wall time of one net tick (4 physics ticks + output), ms.
    pub ms_per_tick_avg: f64,
    /// Worst net tick in the last interval, ms.
    pub ms_per_tick_max: f64,
    /// Seconds since the loop started.
    pub uptime_s: f64,
}

/// Structured log sink: `(event, fields)`.
pub type LogFn = Arc<dyn Fn(&str, Vec<(&'static str, String)>) + Send + Sync>;

/// Run `make()`'s instance on a new thread at `net_hz` until `events` disconnects.
/// The instance is constructed on that thread, so it need not be `Send`.
pub fn spawn_game_loop<S, F>(
    make: F,
    events: std_mpsc::Receiver<IoEvent>,
    status: Arc<Mutex<StatusReport>>,
    log: LogFn,
    net_hz: u32,
) -> JoinHandle<()>
where
    S: NetSim + 'static,
    F: FnOnce() -> ServerInstance<S> + Send + 'static,
{
    spawn_game_loop_with(
        make,
        events,
        status,
        log,
        net_hz,
        |_: &mut ServerInstance<S>| {},
    )
}

/// [`spawn_game_loop`] with a host hook run on the game thread after every net tick, before
/// output is flushed (e.g. the meta service link: join verification, kicks, reports).
pub fn spawn_game_loop_with<S, F, H>(
    make: F,
    events: std_mpsc::Receiver<IoEvent>,
    status: Arc<Mutex<StatusReport>>,
    log: LogFn,
    net_hz: u32,
    mut hook: H,
) -> JoinHandle<()>
where
    S: NetSim + 'static,
    F: FnOnce() -> ServerInstance<S> + Send + 'static,
    H: FnMut(&mut ServerInstance<S>) + Send + 'static,
{
    std::thread::Builder::new()
        .name("wp-game-loop".into())
        .spawn(move || {
            let mut inst = make();
            let period = Duration::from_secs_f64(1.0 / net_hz as f64);
            let start = Instant::now();
            let mut next = start;
            let mut socks: BTreeMap<u64, (ConnId, mpsc::UnboundedSender<OutFrame>)> =
                BTreeMap::new();
            let mut conns: BTreeMap<ConnId, u64> = BTreeMap::new();
            let (mut window_ticks, mut window_ms, mut window_max) = (0u32, 0.0f64, 0.0f64);
            let mut window_start = Instant::now();
            loop {
                loop {
                    match events.try_recv() {
                        Ok(IoEvent::Open { sock, peer, tx }) => {
                            let conn = inst.connect();
                            socks.insert(sock, (conn, tx));
                            conns.insert(conn, sock);
                            log(
                                "connect",
                                vec![("sock", sock.to_string()), ("peer", peer.to_string())],
                            );
                        }
                        Ok(IoEvent::Frame { sock, bytes }) => {
                            if let Some((conn, _)) = socks.get(&sock) {
                                inst.receive(*conn, &bytes);
                            }
                        }
                        Ok(IoEvent::Close { sock }) => {
                            if let Some((conn, _)) = socks.remove(&sock) {
                                conns.remove(&conn);
                                inst.disconnect(conn);
                                log("disconnect", vec![("sock", sock.to_string())]);
                            }
                        }
                        Err(std_mpsc::TryRecvError::Empty) => break,
                        Err(std_mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                let t0 = Instant::now();
                inst.net_tick();
                hook(&mut inst);
                for out in inst.drain_outgoing() {
                    let Some(sock) = conns.get(&out.conn) else {
                        continue;
                    };
                    if let Some((_, tx)) = socks.get(sock) {
                        let _ = tx.send(OutFrame::Frame(out.bytes));
                        if out.close {
                            let _ = tx.send(OutFrame::Close);
                        }
                    }
                }
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                window_ticks += 1;
                window_ms += ms;
                window_max = window_max.max(ms);
                if window_start.elapsed() >= Duration::from_secs(1) {
                    let secs = window_start.elapsed().as_secs_f64();
                    let report = StatusReport {
                        stats: inst.stats(),
                        players: inst
                            .player_names()
                            .into_iter()
                            .map(|(p, n)| (p.0, n))
                            .collect(),
                        tick_rate_hz: window_ticks as f64 / secs,
                        ms_per_tick_avg: window_ms / window_ticks as f64,
                        ms_per_tick_max: window_max,
                        uptime_s: start.elapsed().as_secs_f64(),
                    };
                    if let Ok(mut s) = status.lock() {
                        *s = report;
                    }
                    (window_ticks, window_ms, window_max) = (0, 0.0, 0.0);
                    window_start = Instant::now();
                }
                next += period;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else if now - next > Duration::from_millis(250) {
                    // Hopelessly behind (debugger, overload): drop the backlog.
                    log(
                        "tick_overrun",
                        vec![(
                            "behind_ms",
                            format!("{:.0}", (now - next).as_secs_f64() * 1000.0),
                        )],
                    );
                    next = now;
                }
            }
        })
        .expect("spawn game loop thread")
}

static NEXT_SOCK: AtomicU64 = AtomicU64::new(1);

/// Accept websocket connections forever, forwarding frames to the game loop.
pub async fn serve_ws(
    listener: TcpListener,
    events: std_mpsc::Sender<IoEvent>,
    log: LogFn,
) -> std::io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        tokio::spawn(handle_socket(stream, peer, events.clone(), log.clone()));
    }
}

async fn handle_socket(
    stream: TcpStream,
    peer: SocketAddr,
    events: std_mpsc::Sender<IoEvent>,
    log: LogFn,
) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            log(
                "handshake_failed",
                vec![("peer", peer.to_string()), ("error", e.to_string())],
            );
            return;
        }
    };
    let sock = NEXT_SOCK.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::unbounded_channel();
    if events.send(IoEvent::Open { sock, peer, tx }).is_err() {
        return;
    }
    let (mut sink, mut source) = ws.split();
    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Some(OutFrame::Frame(bytes)) => {
                    if sink.send(Message::binary(bytes)).await.is_err() {
                        break;
                    }
                }
                Some(OutFrame::Close) | None => {
                    let _ = sink.send(Message::Close(None)).await;
                    break;
                }
            },
            msg = source.next() => match msg {
                Some(Ok(Message::Binary(b))) => {
                    if events.send(IoEvent::Frame { sock, bytes: b.to_vec() }).is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            }
        }
    }
    let _ = events.send(IoEvent::Close { sock });
}

/// Client-side websocket [`Transport`]. Frames are pumped by a background task.
pub struct WsTransport {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    closed: Arc<AtomicBool>,
}

impl WsTransport {
    /// True once the socket closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
}

/// Connect to `url` (e.g. `ws://127.0.0.1:8080`).
pub async fn connect_ws(url: &str) -> Result<WsTransport, NetError> {
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| NetError::Transport(e.to_string()))?;
    let (to_net, mut from_app) = mpsc::unbounded_channel::<Vec<u8>>();
    let (to_app, from_net) = mpsc::unbounded_channel::<Vec<u8>>();
    let closed = Arc::new(AtomicBool::new(false));
    let closed_task = closed.clone();
    let (mut sink, mut source) = ws.split();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                out = from_app.recv() => match out {
                    Some(bytes) => {
                        if sink.send(Message::binary(bytes)).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        let _ = sink.send(Message::Close(None)).await;
                        break;
                    }
                },
                msg = source.next() => match msg {
                    Some(Ok(Message::Binary(b))) => {
                        if to_app.send(b.to_vec()).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
        }
        closed_task.store(true, Ordering::Relaxed);
    });
    Ok(WsTransport {
        tx: to_net,
        rx: from_net,
        closed,
    })
}

impl Transport for WsTransport {
    fn send(&mut self, _channel: Channel, bytes: &[u8]) -> Result<(), NetError> {
        self.tx
            .send(bytes.to_vec())
            .map_err(|_| NetError::Transport("socket closed".into()))
    }

    fn poll(&mut self) -> Result<Option<Vec<u8>>, NetError> {
        match self.rx.try_recv() {
            Ok(b) => Ok(Some(b)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(NetError::Transport("socket closed".into()))
            }
        }
    }
}
