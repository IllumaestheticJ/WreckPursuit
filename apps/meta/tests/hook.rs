//! The server-side hook (`wp_meta::instance`) as `apps/server` will use it: a non-blocking
//! worker driven from a tick loop. This test plays F's server against the real meta service.

mod common;

use std::time::{Duration, Instant};

use common::*;
use wp_contracts::{DamageMode, PlayerId, Role};
use wp_economy::Economy;
use wp_meta::api::*;
use wp_meta::instance::{ticks_to_ms, MetaLinkConfig, MetaReply, MetaRequest, MetaWorker};

/// Poll the worker like a tick loop until `pick` matches a reply (other replies are kept).
fn wait_for<T>(
    w: &MetaWorker,
    seen: &mut Vec<MetaReply>,
    mut pick: impl FnMut(&MetaReply) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        seen.extend(w.poll());
        if let Some(i) = seen.iter().position(|r| pick(r).is_some()) {
            return pick(&seen.remove(i)).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a meta reply"
        );
        std::thread::sleep(Duration::from_millis(2)); // one "tick"
    }
}

#[test]
fn server_hook_end_to_end() {
    let srv = TestServer::start();
    let eco = Economy::embedded();
    let vars = |k: &str| match k {
        "WP_META_URL" => Some(srv.base.clone()),
        "WP_META_INSTANCE_KEY" => Some(srv.config.instance_key.clone()),
        "WP_META_REGION" => Some("eu-west".into()),
        _ => None,
    };
    let slots = RoleCounts {
        racer: 4,
        police_chase: 2,
        police_captain: 1,
        spectator: 0,
    };
    let mut cfg = MetaLinkConfig::from_vars(vars, DamageMode::Arcade, slots, "ws://127.0.0.1:8080")
        .expect("hook on")
        .expect("valid config");
    cfg.heartbeat_every = Some(Duration::from_millis(20));
    let worker = MetaWorker::start(&cfg).expect("register");
    let mut seen = Vec::new();

    // First heartbeat brings the flags.
    let hb = wait_for(&worker, &mut seen, |r| match r {
        MetaReply::Heartbeat(Ok(h)) => Some(h.clone()),
        _ => None,
    });
    assert!(hb.flags.contains_key("illegal_day_race_frequency"));

    // Two players matchmake and connect with their join tokens.
    let mut accounts = Vec::new();
    for (tag, name) in [(11u64, "hooka"), (12, "hookb")] {
        let (id, c) = player(&srv, name);
        let st = c
            .queue(&prefs(Role::Racer, DamageMode::Arcade, "C"))
            .unwrap();
        let a = st.assignment.expect("assigned");
        assert_eq!(a.instance_id, worker.instance_id());
        worker.send(MetaRequest::VerifyJoin {
            tag,
            join_token: a.join_token,
        });
        let joined = wait_for(&worker, &mut seen, |r| match r {
            MetaReply::Joined { tag: t, result } if *t == tag => Some(result.clone()),
            _ => None,
        })
        .expect("join verified");
        assert_eq!(joined.account_id, id);
        assert_eq!(joined.role, Role::Racer);
        accounts.push(id);
    }
    // A bad token is refused (server sends Reject).
    worker.send(MetaRequest::VerifyJoin {
        tag: 99,
        join_token: "wpj_bogus".into(),
    });
    let bad = wait_for(&worker, &mut seen, |r| match r {
        MetaReply::Joined { tag: 99, result } => Some(result.clone()),
        _ => None,
    });
    assert_eq!(bad.unwrap_err().code(), Some("invalid_join_token"));

    worker.set_presence(HeartbeatRequest {
        population: RoleCounts {
            racer: 2,
            ..Default::default()
        },
        players: accounts.clone(),
        draining: false,
        tick_ms: 1.2,
    });

    // Economy: load, settle locally, write through.
    for id in &accounts {
        worker.send(MetaRequest::LoadAccount { account: *id });
        let mut acct = wait_for(&worker, &mut seen, |r| match r {
            MetaReply::AccountLoaded { account, result } if account == id => Some(result.clone()),
            _ => None,
        })
        .unwrap()
        .expect("account exists");
        assert_eq!(acct.player, PlayerId(*id));
        let ((), events) = eco.adjust(&mut acct, 250, "race purse (test)").unwrap();
        worker.send(MetaRequest::Commit {
            account: Box::new(acct),
            events,
        });
        let committed = wait_for(&worker, &mut seen, |r| match r {
            MetaReply::Committed { account, result } if account == id => Some(result.clone()),
            _ => None,
        });
        committed.expect("commit");
    }

    // Race results from tick times.
    worker.send(MetaRequest::Race(RaceReport {
        report_key: format!("i{}-race-1", worker.instance_id()),
        route_id: "hook_loop".into(),
        race_type: "sprint".into(),
        mode: DamageMode::Arcade,
        bracket: "C".into(),
        entries: accounts
            .iter()
            .enumerate()
            .map(|(k, id)| RaceEntry {
                account_id: *id,
                position: k as u32 + 1,
                finish_time_ms: Some(ticks_to_ms(43_200 + k as u64 * 240)),
                clean: true,
            })
            .collect(),
    }));
    let race = wait_for(&worker, &mut seen, |r| match r {
        MetaReply::RaceReported(res) => Some(res.clone()),
        _ => None,
    })
    .expect("race reported");
    assert_eq!(race.ratings.len(), 2);

    // An admin kick arrives through the worker's own heartbeats.
    srv.admin().admin_kick(accounts[1], "afk").unwrap();
    let kicks = wait_for(&worker, &mut seen, |r| match r {
        MetaReply::Heartbeat(Ok(h)) if !h.kicks.is_empty() => Some(h.kicks.clone()),
        _ => None,
    });
    assert_eq!(kicks[0].account_id, accounts[1]);

    // Shutdown deregisters: the directory no longer lists the instance.
    let id = worker.instance_id();
    worker.shutdown();
    assert!(srv
        .client()
        .instances()
        .unwrap()
        .iter()
        .all(|i| i.instance_id != id));
}
