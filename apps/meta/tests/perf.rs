//! Load measurement (ignored by default):
//! `cargo test --release -p wp_meta --test perf -- --ignored --nocapture`.
//! Calls the domain functions directly (no HTTP) to time the matcher and result writes.

use std::time::Instant;

use wp_contracts::{DamageMode, Role};
use wp_economy::Economy;
use wp_meta::api::*;
use wp_meta::config::{Config, HashCost};
use wp_meta::{auth, directory, leaderboards, matchmaking, results};

#[test]
#[ignore]
fn matchmaking_and_results_throughput() {
    let cfg = Config::test();
    let db = wp_meta::db::Db::open_in_memory().unwrap();
    let eco = Economy::embedded();
    let now = 1_800_000_000_000;
    let mut conn = db.lock();
    const PLAYERS: u32 = 2_000;
    const INSTANCES: u32 = 100;

    let t = Instant::now();
    let mut ids = Vec::new();
    for i in 0..PLAYERS {
        let s = auth::signup(
            &mut conn,
            &eco,
            &format!("p{i}@perf.test"),
            "perf-password",
            &format!("perf{i}"),
            HashCost::Fast,
            cfg.session_ttl_ms,
            now,
        )
        .unwrap();
        ids.push(s.account_id);
    }
    let signup_ms = t.elapsed().as_secs_f64() * 1e3;

    for i in 0..INSTANCES {
        directory::register(
            &conn,
            &RegisterInstanceRequest {
                name: format!("i{i}"),
                region: "eu-west".into(),
                city: "c".into(),
                mode: if i % 4 == 0 {
                    DamageMode::Simulation
                } else {
                    DamageMode::Arcade
                },
                bracket: "open".into(),
                address: "ws://x".into(),
                slots: RoleCounts {
                    racer: 16,
                    police_chase: 4,
                    police_captain: 1,
                    spectator: 2,
                },
            },
            cfg.heartbeat_interval_ms,
            now,
        )
        .unwrap();
    }

    let t = Instant::now();
    for (k, id) in ids.iter().enumerate() {
        let prefs = MatchPrefs {
            role: if k % 5 == 0 {
                Role::PoliceChase
            } else {
                Role::Racer
            },
            mode: if k % 4 == 0 {
                DamageMode::Simulation
            } else {
                DamageMode::Arcade
            },
            bracket: "C".into(),
            region: "eu-west".into(),
            city: None,
        };
        matchmaking::queue(&mut conn, *id, &prefs, &cfg, now).unwrap();
    }
    let queue_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let mut joined = 0;
    for id in &ids {
        let st = matchmaking::status(&conn, *id, &cfg, now).unwrap();
        if let Some(a) = st.assignment {
            matchmaking::verify_join(&conn, a.instance_id, &a.join_token, &cfg, now).unwrap();
            joined += 1;
        }
    }
    let join_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let mut races = 0;
    for (n, chunk) in ids.chunks(8).enumerate() {
        // Race the players of this chunk that share the first one's instance.
        let home: Option<InstanceId> = conn
            .query_row(
                "SELECT instance_id FROM tickets WHERE account_id = ?1 AND state = 'joined'",
                [chunk[0]],
                |r| r.get(0),
            )
            .ok();
        let Some(home) = home else { continue };
        let entries: Vec<RaceEntry> = chunk
            .iter()
            .filter(|id| results::ensure_hosted(&conn, home, **id).is_ok())
            .enumerate()
            .map(|(p, id)| RaceEntry {
                account_id: *id,
                position: p as u32 + 1,
                finish_time_ms: Some(100_000 + p as u64),
                clean: p % 2 == 0,
            })
            .collect();
        if entries.len() < 2 {
            continue;
        }
        results::report_race(
            &mut conn,
            home,
            &RaceReport {
                report_key: format!("r{n}"),
                route_id: "perf".into(),
                race_type: "sprint".into(),
                mode: DamageMode::Arcade,
                bracket: "C".into(),
                entries,
            },
            now,
        )
        .unwrap();
        races += 1;
    }
    let race_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let board =
        leaderboards::top(&conn, "clean_rating", leaderboards::Period::All, 100, now).unwrap();
    let top_ms = t.elapsed().as_secs_f64() * 1e3;

    println!(
        "signup (fast hash) x{PLAYERS}: {signup_ms:.0} ms ({:.3} ms each)",
        signup_ms / PLAYERS as f64
    );
    println!(
        "queue + match x{PLAYERS} against {INSTANCES} instances: {queue_ms:.0} ms ({:.3} ms each)",
        queue_ms / PLAYERS as f64
    );
    println!("status + verify_join x{joined}: {join_ms:.0} ms");
    println!(
        "race reports x{races} (<=8 racers): {race_ms:.0} ms ({:.3} ms each)",
        race_ms / races.max(1) as f64
    );
    println!(
        "leaderboard top 100 of {}: {top_ms:.2} ms",
        board.rows.len()
    );
    assert!(joined > PLAYERS / 2);
}
