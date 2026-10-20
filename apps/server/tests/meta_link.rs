//! The meta join-token flow end to end: the real meta service in-process (meta's own test
//! harness), `wp-server` linked to it, and native websocket clients. A matchmade player joins
//! with its join token (named and seated as meta says, and handed meta's reconnect token);
//! a forged or missing token is rejected; an admin kick from meta disconnects the player.

#[path = "../../meta/tests/common/mod.rs"]
mod meta_common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use meta_common::{player, prefs, TestServer};
use wp_contracts::{DamageMode, Role};
use wp_net::client::{default_damage_factory, Client, ClientConfig, Phase};
use wp_net::native::{connect_ws, WsTransport};
use wp_net::testkit::{load_data, repo_data_dir};
use wp_net::Transport;
use wp_server::{null_log, run_with, AppConfig};

fn client(name: &str) -> Client {
    let (cars, modes) = load_data(&repo_data_dir()).unwrap();
    Client::new(ClientConfig {
        name: name.into(),
        input_source: wp_contracts::InputSource::Keyboard,
        cars,
        modes,
        damage_factory: default_damage_factory,
        city_ground: None,
    })
}

/// Pump `c` over `t` (60 Hz) until `done` or `secs` pass.
async fn pump(
    c: &mut Client,
    t: &mut WsTransport,
    start: Instant,
    secs: f64,
    mut done: impl FnMut(&Client) -> bool,
) -> bool {
    let until = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < until {
        let now = start.elapsed().as_secs_f64() * 1000.0;
        loop {
            match t.poll() {
                Ok(Some(f)) => {
                    let _ = c.receive(&f, now);
                }
                Ok(None) => break,
                Err(_) => return done(c),
            }
        }
        if done(c) {
            return true;
        }
        c.update(now, &Default::default());
        for (ch, b) in c.drain_outgoing() {
            let _ = t.send(ch, &b);
        }
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
    done(c)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn matchmade_players_join_with_tokens_and_kicks_apply() {
    let meta = TestServer::start();
    let (base, key) = (meta.base.clone(), meta.config.instance_key.clone());
    let vars: wp_server::MetaVars = Arc::new(move |k: &str| match k {
        "WP_META_URL" => Some(base.clone()),
        "WP_META_INSTANCE_KEY" => Some(key.clone()),
        "WP_META_REGION" => Some("eu-west".into()),
        _ => None,
    });
    let cfg = AppConfig {
        port: 0,
        admin_port: 0,
        bind: "127.0.0.1".into(),
        data_dir: repo_data_dir().display().to_string(),
        ..Default::default()
    };
    let running = run_with(cfg, null_log(), vars)
        .await
        .expect("server with meta");
    let url = format!("ws://{}", running.ws_addr);
    let start = Instant::now();

    // Matchmaking hands out a join token for this instance (blocking HTTP: off the runtime).
    let (account, session) = tokio::task::block_in_place(|| player(&meta, "netjoin"));
    let ticket = tokio::task::block_in_place(|| {
        session
            .queue(&prefs(Role::Racer, DamageMode::Arcade, "C"))
            .expect("queue")
    });
    let assignment = ticket.assignment.expect("assigned to the only instance");
    assert_eq!(assignment.address, url);

    // 1. The matchmade player joins: meta's name, the reserved role, meta's reconnect token.
    let mut t = connect_ws(&url).await.unwrap();
    let mut c = client("ignored-name");
    c.connect_with_join_token(0.0, 0, &assignment.join_token);
    let joined = pump(&mut c, &mut t, start, 10.0, |c| {
        *c.phase() == Phase::Joined && c.role() == Role::Racer && c.own_vehicle().is_some()
    })
    .await;
    assert!(joined, "phase {:?} role {:?}", c.phase(), c.role());
    assert!(
        !c.meta_reconnect_token.is_empty(),
        "meta reconnect token handed over"
    );

    // 2. Forged and missing tokens are rejected.
    for token in ["wpj_forged", ""] {
        let mut t2 = connect_ws(&url).await.unwrap();
        let mut c2 = client("intruder");
        c2.connect_with_join_token(0.0, 0, token);
        let rejected = pump(&mut c2, &mut t2, start, 10.0, |c| {
            matches!(c.phase(), Phase::Rejected(_))
        })
        .await;
        assert!(rejected, "token {token:?}: phase {:?}", c2.phase());
    }

    // 3. An admin kick in meta reaches the player through the next heartbeat.
    tokio::task::block_in_place(|| meta.admin().admin_kick(account, "test kick")).expect("kick");
    let kicked = pump(
        &mut c,
        &mut t,
        start,
        20.0,
        |c| matches!(c.phase(), Phase::Rejected(r) if r.contains("test kick")),
    )
    .await;
    assert!(kicked, "kick applied: phase {:?}", c.phase());
}
