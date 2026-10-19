//! End-to-end over real websockets: a native client joins and drives, the admin endpoint
//! reports it, and `--bots` connects scripted clients.

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wp_contracts::{Role, VehicleInput};
use wp_net::client::{default_damage_factory, Client, ClientConfig, Phase};
use wp_net::native::connect_ws;
use wp_net::testkit::{load_data, repo_data_dir};
use wp_net::Transport;
use wp_server::{null_log, run, AppConfig};

fn test_config() -> AppConfig {
    AppConfig {
        port: 0,
        admin_port: 0,
        bind: "127.0.0.1".into(),
        data_dir: repo_data_dir().display().to_string(),
        ..Default::default()
    }
}

async fn http_get(addr: std::net::SocketAddr, path: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

fn body_json(resp: &str) -> serde_json::Value {
    let body = resp.split("\r\n\r\n").nth(1).unwrap();
    serde_json::from_str(body).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_joins_drives_and_admin_reports_it() {
    let running = run(test_config(), null_log()).await.unwrap();
    let (cars, modes) = load_data(&repo_data_dir()).unwrap();
    let mut t = connect_ws(&format!("ws://{}", running.ws_addr))
        .await
        .unwrap();
    let mut client = Client::new(ClientConfig {
        name: "e2e".into(),
        input_source: wp_contracts::InputSource::Keyboard,
        cars,
        modes,
        damage_factory: default_damage_factory,
    });
    let start = Instant::now();
    client.connect(0.0, 0);
    let mut requested = false;
    let throttle = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    while start.elapsed() < Duration::from_secs(3) {
        tokio::time::sleep(Duration::from_millis(16)).await;
        let now = start.elapsed().as_secs_f64() * 1000.0;
        while let Some(f) = t.poll().unwrap() {
            client.receive(&f, now).unwrap();
        }
        if !requested && *client.phase() == Phase::Joined {
            client.request_role(Role::Racer, now);
            requested = true;
        }
        client.update(now, &throttle);
        for (ch, b) in client.drain_outgoing() {
            t.send(ch, &b).unwrap();
        }
    }
    assert_eq!(*client.phase(), Phase::Joined);
    assert_eq!(client.role(), Role::Racer);
    let s = client.predicted_state().expect("predicting own car");
    assert!(s.pos[1] > 5.0, "drove forward: {:?}", s.pos);
    let stats = client.stats();
    assert!(stats.reconciles > 20, "{stats:?}");
    assert!(stats.rtt_ms < 100.0);

    let resp = http_get(running.admin_addr, "/status").await;
    assert!(resp.starts_with("HTTP/1.1 200"));
    let j = body_json(&resp);
    assert_eq!(j["players"], 1);
    assert!(j["tick_rate_hz"].as_f64().unwrap() > 50.0, "{j}");
    assert!(http_get(running.admin_addr, "/nope")
        .await
        .starts_with("HTTP/1.1 404"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bots_flag_connects_scripted_clients() {
    let cfg = AppConfig {
        bots: 6,
        arena_radius_m: Some(60.0),
        ..test_config()
    };
    let running = run(cfg, null_log()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(3500)).await;
    let j = body_json(&http_get(running.admin_addr, "/status").await);
    assert_eq!(j["players"], 6, "{j}");
    assert_eq!(j["connected"], 6, "{j}");
    assert!(j["downlink_bps_max"].as_f64().unwrap() > 0.0);
    assert!(j["downlink_bps_max"].as_f64().unwrap() <= 40_000.0);
}

#[test]
fn config_from_flags_and_toml() {
    let dir = std::env::temp_dir().join(format!("wp_server_cfg_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("server.toml");
    std::fs::write(&file, "port = 9000\nseed = 42\nmode = \"sim\"\n").unwrap();
    let args: Vec<String> = [
        "--config",
        file.to_str().unwrap(),
        "--port",
        "9100",
        "--bots",
        "3",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let cfg = AppConfig::from_args(&args).unwrap();
    assert_eq!(cfg.port, 9100, "flags override the file");
    assert_eq!(cfg.seed, 42);
    assert_eq!(cfg.mode, "sim");
    assert_eq!(cfg.bots, 3);
    assert!(AppConfig::from_args(&["--wat".into()]).is_err());
    assert!(AppConfig::from_args(&["--port".into()]).is_err());
}
