//! Native websocket transport (tokio + tungstenite). Stub server: every connected client
//! receives `{"tick": N}` text frames at [`wp_contracts::NET_TICK_HZ`] and any text it sends is
//! echoed back.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

/// Run the tick-echo server on an already-bound listener until the process exits.
/// One global tick counter is broadcast to all clients.
pub async fn serve_tick_echo(listener: TcpListener) -> std::io::Result<()> {
    let (tick_tx, tick_rx) = watch::channel(0u64);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs_f64(
            1.0 / wp_contracts::NET_TICK_HZ as f64,
        ));
        let mut tick = 0u64;
        loop {
            interval.tick().await;
            tick += 1;
            if tick_tx.send(tick).is_err() {
                break;
            }
        }
    });
    loop {
        let (stream, peer) = listener.accept().await?;
        tokio::spawn(handle_client(stream, peer, tick_rx.clone()));
    }
}

async fn handle_client(stream: TcpStream, peer: SocketAddr, mut ticks: watch::Receiver<u64>) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("[wp_net] handshake with {peer} failed: {e}");
            return;
        }
    };
    let (mut tx, mut rx) = ws.split();
    loop {
        tokio::select! {
            changed = ticks.changed() => {
                if changed.is_err() {
                    break;
                }
                let tick = *ticks.borrow_and_update();
                if tx.send(Message::text(format!("{{\"tick\":{tick}}}"))).await.is_err() {
                    break;
                }
            }
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    if tx.send(Message::Text(t)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            }
        }
    }
}
