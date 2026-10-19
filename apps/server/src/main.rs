//! `wp-server [--port 8080]` — headless server (owned by workstream 07).
//! **Stub:** accepts websocket connections, streams `{"tick":N}` at 60 Hz and echoes text.

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("wp-server listening on ws://0.0.0.0:{port}");
    wp_net::native::serve_tick_echo(listener).await
}

#[cfg(test)]
mod tests {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    #[tokio::test]
    async fn streams_ticks_and_echoes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(wp_net::native::serve_tick_echo(listener));
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        let first = ws.next().await.unwrap().unwrap();
        assert!(first.to_text().unwrap().starts_with("{\"tick\":"));
        ws.send(Message::text("hello")).await.unwrap();
        let mut echoed = false;
        for _ in 0..200 {
            if ws.next().await.unwrap().unwrap().to_text().unwrap() == "hello" {
                echoed = true;
                break;
            }
        }
        assert!(echoed);
    }
}
