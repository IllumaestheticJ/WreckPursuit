//! The meta join-token flow end to end: the real meta service in-process (meta's own test
//! harness), `wp-server` linked to it, and native websocket clients. A matchmade player joins
//! with its join token (named and seated as meta says, and handed meta's reconnect token);
//! a forged or missing token is rejected; an admin kick from meta disconnects the player.

#[path = "../../meta/tests/common/mod.rs"]
mod meta_common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use meta_common::{player, prefs, TestServer};
use wp_contracts::{DamageMode, Role, VehicleInput};
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

/// Pump several clients (each with its own input) until `done` or `secs` pass.
async fn pump_all(
    peers: &mut [(Client, WsTransport, VehicleInput)],
    start: Instant,
    secs: f64,
    mut done: impl FnMut(&[(Client, WsTransport, VehicleInput)]) -> bool,
) -> bool {
    let until = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < until {
        let now = start.elapsed().as_secs_f64() * 1000.0;
        for (c, t, input) in peers.iter_mut() {
            while let Ok(Some(f)) = t.poll() {
                let _ = c.receive(&f, now);
            }
            c.update(now, input);
            for (ch, b) in c.drain_outgoing() {
                let _ = t.send(ch, &b);
            }
        }
        if done(peers) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
    done(peers)
}

/// Economy write-through and race reports: two matchmade racers run a lobby sprint. Stakes
/// and payouts land in meta as ledger events; an admin adjustment made in meta mid-race makes
/// the server's next commit conflict, and the server reloads and re-applies its change, so
/// meta ends with the adjustment *and* the payout; the race is reported (clean ratings).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn race_money_and_results_are_written_through_to_meta() {
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
        // Daytime: the base stake.
        time_of_day_h: 12.0,
        time_ratio: 1.0,
        data_dir: repo_data_dir().display().to_string(),
        ..Default::default()
    };
    let running = run_with(cfg, null_log(), vars)
        .await
        .expect("server with meta");
    let url = format!("ws://{}", running.ws_addr);
    let start = Instant::now();

    // Two racers matchmake and join, in order (player 1 = A, player 2 = B).
    let mut peers = Vec::new();
    let mut ids = Vec::new();
    let mut sessions = Vec::new();
    for name in ["racea", "raceb"] {
        let (account, session) = tokio::task::block_in_place(|| player(&meta, name));
        let ticket = tokio::task::block_in_place(|| {
            session
                .queue(&prefs(Role::Racer, DamageMode::Arcade, "C"))
                .expect("queue")
        });
        let a = ticket.assignment.expect("assigned");
        let t = connect_ws(&url).await.unwrap();
        let mut c = client(name);
        c.connect_with_join_token(0.0, 0, &a.join_token);
        peers.push((c, t, VehicleInput::default()));
        ids.push(account);
        sessions.push(session);
        let n = peers.len();
        assert!(
            pump_all(&mut peers, start, 10.0, |p| p[..n].iter().all(
                |(c, _, _)| { c.own_vehicle().is_some() && c.account.0 > 0 }
            ))
            .await,
            "{name} joined with its meta account"
        );
    }
    let cash0: Vec<i64> = tokio::task::block_in_place(|| {
        sessions
            .iter()
            .map(|s| s.my_economy().expect("economy").cash)
            .collect()
    });
    assert_eq!(
        peers
            .iter()
            .map(|(c, _, _)| c.account.0)
            .collect::<Vec<_>>(),
        cash0,
        "the server holds meta's accounts"
    );

    // A hosts a sprint straight ahead of both cars (grid spawns x = -20 / -12 m, facing +Y);
    // B enters; A starts it.
    let now = start.elapsed().as_secs_f64() * 1000.0;
    peers[0].0.send_reliable(
        &wp_net::protocol::ClientMsg::RaceCreate {
            laps: 1,
            checkpoints: vec![[-160, 600], [-160, 1400]],
        },
        now,
    );
    assert!(pump_all(&mut peers, start, 5.0, |p| !p[1].0.races.is_empty()).await);
    let race = *peers[1].0.races.keys().next().unwrap();
    peers[1].0.race_command(
        wp_contracts::RaceCommand::Enter {
            event: wp_contracts::RaceEventId(race),
        },
        now,
    );
    assert!(
        pump_all(&mut peers, start, 5.0, |p| p[0].0.races[&race]
            .members
            .len()
            == 2)
        .await
    );
    let now = start.elapsed().as_secs_f64() * 1000.0;
    peers[0]
        .0
        .race_command(wp_contracts::RaceCommand::Ready, now);
    // Stakes are charged when the countdown ends: both accounts drop in meta.
    assert!(
        pump_all(&mut peers, start, 8.0, |p| p
            .iter()
            .zip(&cash0)
            .all(|((c, _, _), c0)| c.account.0 < *c0))
        .await,
        "stakes charged"
    );
    let stake = cash0[0] - peers[0].0.account.0;
    assert!(stake > 0);

    // Meanwhile an admin adjusts A in meta: the server's next commit for A conflicts.
    tokio::task::block_in_place(|| meta.admin().admin_adjust(ids[0], 1000, "test bonus"))
        .expect("adjust");

    // Both drive the course; payouts follow the finish.
    for p in peers.iter_mut() {
        p.2 = VehicleInput {
            throttle: 0.7,
            ..Default::default()
        };
    }
    assert!(
        pump_all(&mut peers, start, 40.0, |p| p
            .iter()
            .all(|(c, _, _)| c.race_results.is_some()))
        .await,
        "race finished"
    );
    let results = peers[0].0.race_results.clone().unwrap().1;
    println!("results {results:?}");
    let payout: Vec<i64> = ids
        .iter()
        .enumerate()
        .map(|(k, _)| {
            results
                .iter()
                .find(|r| r.player == k as u32 + 1)
                .map_or(0, |r| r.payout)
        })
        .collect();
    assert!(payout.iter().any(|p| *p > 0), "someone was paid");

    // Meta converges: A = start − stake + adjustment + payout (the payout was re-applied on
    // top of the reloaded account), B = start − stake + payout; the server agrees.
    let want = [
        cash0[0] - stake + 1000 + payout[0],
        cash0[1] - stake + payout[1],
    ];
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = Vec::new();
    while Instant::now() < deadline {
        pump_all(&mut peers, start, 0.3, |_| false).await;
        got = tokio::task::block_in_place(|| {
            sessions
                .iter()
                .map(|s| s.my_economy().expect("economy").cash)
                .collect::<Vec<_>>()
        });
        if got == want && peers.iter().map(|(c, _, _)| c.account.0).eq(want) {
            break;
        }
    }
    if got != want {
        let events: Vec<wp_economy::EconomyEvent> = tokio::task::block_in_place(|| {
            sessions[0]
                .call("GET", "/v1/me/events?from_seq=0", None::<&()>)
                .expect("events")
        });
        for e in &events {
            println!(
                "A event seq {} {:?} cash {:+} rep {:+} → {}",
                e.seq, e.reason, e.cash_delta, e.rep_delta, e.cash_after
            );
        }
    }
    assert_eq!(got, want, "meta cash (stake {stake}, payouts {payout:?})");
    assert_eq!(
        peers
            .iter()
            .map(|(c, _, _)| c.account.0)
            .collect::<Vec<_>>(),
        want,
        "server cash"
    );

    // The race was reported: clean finishers are rated.
    let board = tokio::task::block_in_place(|| {
        meta.client()
            .leaderboard("clean_rating", "all", 10)
            .expect("board")
    });
    println!("clean_rating {:?}", board.rows);
    assert!(
        board.rows.iter().any(|r| ids.contains(&r.account_id)),
        "race reported to meta"
    );
}
