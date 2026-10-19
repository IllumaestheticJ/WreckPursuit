//! Focused API behaviour: auth, rate limits, matchmaking edge cases, economy sequence
//! conflicts, leaderboard periods, admin, crews, persistence.

mod common;

use std::sync::Arc;

use common::*;
use serde_json::json;
use wp_contracts::{DamageMode, PlayerId, Role, WantedLevel};
use wp_economy::{AccountStore, Economy, StoreError};
use wp_meta::api::*;
use wp_meta::client::RemoteAccountStore;
use wp_meta::config::{Config, RateLimits};

fn racer_slots(n: u32) -> RoleCounts {
    RoleCounts {
        racer: n,
        police_chase: 2,
        police_captain: 1,
        spectator: 0,
    }
}

#[test]
fn auth_errors_and_sessions() {
    let srv = TestServer::start();
    let c = srv.client();
    let s = c.signup("Ann@Example.test", "password-123", "Ann").unwrap();
    assert_eq!(
        c.signup("ann@example.test", "password-123", "Other")
            .unwrap_err()
            .code(),
        Some("email_taken")
    );
    assert_eq!(
        c.signup("b@example.test", "password-123", "ANN")
            .unwrap_err()
            .code(),
        Some("name_taken")
    );
    assert_eq!(
        c.signup("c@example.test", "short", "Cee")
            .unwrap_err()
            .code(),
        Some("bad_request")
    );
    assert_eq!(
        c.login("ann@example.test", "wrong-password")
            .unwrap_err()
            .code(),
        Some("invalid_credentials")
    );
    assert_eq!(
        c.login("nobody@example.test", "password-123")
            .unwrap_err()
            .code(),
        Some("invalid_credentials")
    );
    let me = c.with_token(&s.token).me().unwrap();
    assert_eq!(me.display_name, "Ann");
    assert_eq!(me.clean_rating, 1500.0);

    // Logout revokes only that session.
    let s2 = c.login(" ANN@example.test ", "password-123").unwrap();
    c.with_token(&s.token).logout().unwrap();
    assert_eq!(c.with_token(&s.token).me().unwrap_err().status(), Some(401));
    assert!(c.with_token(&s2.token).me().is_ok());

    // Sessions expire.
    srv.advance(srv.config.session_ttl_ms + 1);
    assert_eq!(
        c.with_token(&s2.token).me().unwrap_err().status(),
        Some(401)
    );
    assert_eq!(c.me().unwrap_err().status(), Some(401), "no token");

    // Linking is a stub.
    let s3 = c.login("ann@example.test", "password-123").unwrap();
    let err = c
        .with_token(&s3.token)
        .call_empty(
            "POST",
            "/v1/auth/link",
            Some(&json!({ "provider": "steam" })),
        )
        .unwrap_err();
    assert_eq!(err.status(), Some(501));
    let providers: ProvidersResponse = c.call("GET", "/v1/auth/providers", None::<&()>).unwrap();
    assert_eq!(providers.login, ["password"]);
}

#[test]
fn signup_and_login_are_rate_limited() {
    let srv = TestServer::start_with(Config {
        rate_limits: RateLimits::production(),
        ..Config::test()
    });
    let c = srv.client();
    let burst = RateLimits::production().signup_per_ip.burst as usize;
    for i in 0..burst {
        c.signup(&format!("u{i}@x.test"), "password-123", &format!("user{i}"))
            .unwrap();
    }
    let err = c.signup("late@x.test", "password-123", "late").unwrap_err();
    assert_eq!(err.status(), Some(429));
    srv.advance(60_000);
    c.signup("late@x.test", "password-123", "late").unwrap();

    let per_email = RateLimits::production().login_per_email.burst as usize;
    for _ in 0..per_email {
        let _ = c.login("u0@x.test", "wrong-password");
    }
    assert_eq!(
        c.login("u0@x.test", "password-123").unwrap_err().status(),
        Some(429),
        "even the right password waits once the email bucket is empty"
    );
}

#[test]
fn bad_json_is_a_json_400() {
    let srv = TestServer::start();
    let err = srv
        .client()
        .call_empty("POST", "/v1/auth/signup", Some(&json!({ "email": 5 })))
        .unwrap_err();
    assert_eq!(err.status(), Some(400));
    assert_eq!(err.code(), Some("bad_request"));
}

#[test]
fn economy_commits_are_sequence_checked() {
    let srv = TestServer::start();
    let eco = Economy::embedded();
    let inst = FakeInstance::register(&srv, "eco", DamageMode::Arcade, "open", racer_slots(4));
    let (id, c) = player(&srv, "saver");
    matchmake(
        &c,
        Some(&prefs(Role::Racer, DamageMode::Arcade, "D")),
        &[Arc::clone(&inst)],
    );

    // Two "instances" load the same version; the second commit loses.
    let mut store = RemoteAccountStore::new(inst.client.clone(), inst.id);
    let mut a = store.load(PlayerId(id)).unwrap().unwrap();
    let mut b = a.clone();
    let ((), ev_a) = eco.adjust(&mut a, 100, "first").unwrap();
    let ((), ev_b) = eco.adjust(&mut b, 200, "second").unwrap();
    store.commit(&a, &ev_a).unwrap();
    let err = store.commit(&b, &ev_b).unwrap_err();
    let StoreError::SeqConflict { expected, got } = err else {
        panic!("expected a sequence conflict, got {err:?}")
    };
    assert_eq!(expected, ev_a.last().unwrap().seq + 1);
    assert_eq!(got, ev_b[0].seq);
    assert_eq!(c.my_economy().unwrap(), a);
    let n = store.events_since(PlayerId(id), 0).unwrap().len() as u64;
    assert_eq!(n, a.next_event_seq);

    // Path / body mismatch is refused.
    let (other, _) = player(&srv, "other");
    let err = inst
        .client
        .call_empty(
            "POST",
            &format!("/v1/instances/{}/accounts/{other}/economy/commit", inst.id),
            Some(&CommitRequest {
                account: a.clone(),
                events: vec![],
            }),
        )
        .unwrap_err();
    assert_eq!(err.status(), Some(400));
}

#[test]
fn instance_auth_is_checked() {
    let srv = TestServer::start();
    let a = FakeInstance::register(&srv, "a", DamageMode::Arcade, "open", racer_slots(2));
    let b = FakeInstance::register(&srv, "b", DamageMode::Arcade, "open", racer_slots(2));
    // b's token cannot act as a.
    let err = b
        .client
        .heartbeat(a.id, &HeartbeatRequest::default())
        .unwrap_err();
    assert_eq!(err.status(), Some(401));
    // Registration needs the instance key.
    let err = srv
        .client()
        .with_token("wrong-key")
        .register_instance(&RegisterInstanceRequest {
            name: "x".into(),
            region: "eu-west".into(),
            city: "c".into(),
            mode: DamageMode::Arcade,
            bracket: "open".into(),
            address: "ws://x".into(),
            slots: racer_slots(1),
        })
        .unwrap_err();
    assert_eq!(err.status(), Some(401));
    // Reporting more players than slots is rejected.
    let err = a
        .client
        .heartbeat(
            a.id,
            &HeartbeatRequest {
                population: RoleCounts {
                    racer: 3,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_err();
    assert_eq!(err.status(), Some(400));
    // Deregistered tokens stop working.
    a.client.deregister(a.id).unwrap();
    assert_eq!(
        a.client
            .heartbeat(a.id, &HeartbeatRequest::default())
            .unwrap_err()
            .status(),
        Some(401)
    );
}

#[test]
fn brackets_modes_and_capacity_gate_assignment() {
    let srv = TestServer::start();
    let c_inst = FakeInstance::register(&srv, "c-only", DamageMode::Arcade, "C", racer_slots(1));
    let (_, b_player) = player(&srv, "bplayer");
    let st = b_player
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "B"))
        .unwrap();
    assert_eq!(st.state, TicketState::Queued, "C server does not take B");
    let (_, sim_player) = player(&srv, "simmer");
    let st = sim_player
        .queue(&prefs(Role::Racer, DamageMode::Simulation, "C"))
        .unwrap();
    assert_eq!(
        st.state,
        TicketState::Queued,
        "Arcade server does not take Sim"
    );

    let (_, c1) = player(&srv, "cfirst");
    let (_, c2) = player(&srv, "csecond");
    let s1 = c1
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "C"))
        .unwrap();
    assert_eq!(s1.state, TicketState::Assigned);
    let s2 = c2
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "C"))
        .unwrap();
    assert_eq!(
        s2.state,
        TicketState::Queued,
        "one racer slot, already reserved"
    );
    assert_eq!(
        c2.queue(&prefs(Role::Racer, DamageMode::Arcade, "C"))
            .unwrap_err()
            .code(),
        Some("already_queued")
    );

    // An `open` server registering takes the waiting B player (and the C overflow).
    FakeInstance::register(&srv, "open", DamageMode::Arcade, "open", racer_slots(4));
    assert_eq!(
        b_player.ticket_status().unwrap().state,
        TicketState::Assigned
    );
    assert_eq!(c2.ticket_status().unwrap().state, TicketState::Assigned);
    assert_eq!(
        sim_player.ticket_status().unwrap().state,
        TicketState::Queued
    );
    drop(c_inst);
}

#[test]
fn join_tokens_are_single_use_and_reminted() {
    let srv = TestServer::start();
    let inst = FakeInstance::register(&srv, "j", DamageMode::Arcade, "open", racer_slots(2));
    let (_, c) = player(&srv, "joiner");
    let first = c
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
        .unwrap()
        .assignment
        .unwrap();
    let second = c.ticket_status().unwrap().assignment.unwrap();
    assert_eq!(
        inst.connect(&first.join_token).unwrap_err().code(),
        Some("invalid_join_token"),
        "polling again replaced the token"
    );
    inst.connect(&second.join_token).unwrap();
    assert!(inst.connect(&second.join_token).is_err(), "single use");
    assert_eq!(c.ticket_status().unwrap().state, TicketState::Joined);
}

#[test]
fn tickets_and_seats_expire() {
    let srv = TestServer::start();
    let (_, waiting) = player(&srv, "waiting");
    waiting
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
        .unwrap();
    srv.advance(srv.config.queue_ttl_ms + 1);
    assert_eq!(waiting.ticket_status().unwrap().state, TicketState::Expired);

    // A reserved seat nobody uses is released.
    let inst = FakeInstance::register(&srv, "e", DamageMode::Arcade, "open", racer_slots(1));
    let (_, ghost) = player(&srv, "ghost");
    let (_, next) = player(&srv, "nextup");
    ghost
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
        .unwrap();
    let st = next
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
        .unwrap();
    assert_eq!(st.state, TicketState::Queued);
    srv.advance(2 * srv.config.join_ttl_ms + 1);
    inst.heartbeat();
    assert_eq!(ghost.ticket_status().unwrap().state, TicketState::Expired);
    let a = next
        .ticket_status()
        .unwrap()
        .assignment
        .expect("seat freed");
    // Join tokens expire.
    srv.advance(srv.config.join_ttl_ms + 1);
    inst.heartbeat();
    assert!(inst.connect(&a.join_token).is_err());
}

#[test]
fn dead_instances_lose_their_reservations() {
    let srv = TestServer::start();
    let a = FakeInstance::register(&srv, "dies", DamageMode::Arcade, "open", racer_slots(4));
    let (_, c) = player(&srv, "stranded");
    let st = c
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
        .unwrap();
    assert_eq!(st.assignment.unwrap().instance_id, a.id);
    let b = FakeInstance::register(&srv, "lives", DamageMode::Arcade, "open", racer_slots(4));
    // `a` stops heartbeating; `b` keeps going.
    for _ in 0..4 {
        srv.advance(srv.config.heartbeat_interval_ms);
        b.heartbeat();
    }
    let st = c.ticket_status().unwrap();
    assert_eq!(
        st.assignment.unwrap().instance_id,
        b.id,
        "requeued onto a live server"
    );
    let list = srv.client().instances().unwrap();
    assert_eq!(list.len(), 1);
    let health = srv.admin().admin_health().unwrap();
    assert_eq!(health.instances.len(), 2, "health still shows the dead one");
    assert!(health
        .instances
        .iter()
        .any(|i| i.instance_id == a.id && i.state == InstanceState::Down));
}

#[test]
fn draining_instances_take_no_new_players() {
    let srv = TestServer::start();
    let inst = FakeInstance::register(&srv, "drain", DamageMode::Arcade, "open", racer_slots(4));
    inst.client
        .heartbeat(
            inst.id,
            &HeartbeatRequest {
                draining: true,
                ..Default::default()
            },
        )
        .unwrap();
    let (_, c) = player(&srv, "late");
    let st = c
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
        .unwrap();
    assert_eq!(st.state, TicketState::Queued);
    inst.heartbeat();
    assert_eq!(c.ticket_status().unwrap().state, TicketState::Assigned);
}

#[test]
fn parties_queue_together_or_not_at_all() {
    let srv = TestServer::start();
    let small = FakeInstance::register(&srv, "small", DamageMode::Arcade, "open", racer_slots(2));
    let (lead_id, lead) = player(&srv, "leader");
    let (m1_id, m1) = player(&srv, "member1");
    let (_, m2) = player(&srv, "member2");
    let p = lead.create_party().unwrap();
    assert_eq!(
        m1.join_party(p.party_id, "WRONGCOD").unwrap_err().code(),
        Some("bad_party_code")
    );
    m1.join_party(p.party_id, &p.code.to_lowercase()).unwrap();
    let info = m2.join_party(p.party_id, &p.code).unwrap();
    assert_eq!(info.members.len(), 3);
    assert_eq!(info.leader_id, lead_id);
    assert_eq!(
        m1.queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
            .unwrap_err()
            .code(),
        Some("not_party_leader")
    );
    assert_eq!(
        lead.queue(&prefs(Role::PoliceCaptain, DamageMode::Arcade, "D"))
            .unwrap_err()
            .status(),
        Some(400)
    );
    // Three racers do not fit in two seats: nobody is placed.
    let st = lead
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
        .unwrap();
    assert_eq!(st.state, TicketState::Queued);
    assert_eq!(st.party_id, Some(p.party_id));
    let big = FakeInstance::register(&srv, "big", DamageMode::Arcade, "open", racer_slots(6));
    for c in [&lead, &m1, &m2] {
        let a = c.ticket_status().unwrap().assignment.unwrap();
        assert_eq!(a.instance_id, big.id);
    }
    // Leaving cancels the party's waiting tickets and hands over leadership.
    lead.call_empty("POST", "/v1/parties/leave", None::<&()>)
        .unwrap();
    assert_eq!(m1.ticket_status().unwrap().state, TicketState::Cancelled);
    let info: PartyInfo = m1
        .call("GET", &format!("/v1/parties/{}", p.party_id), None::<&()>)
        .unwrap();
    assert_eq!(info.leader_id, m1_id);
    drop(small);
}

#[test]
fn feature_flags_gate_queues_and_reach_instances() {
    let srv = TestServer::start();
    let inst = FakeInstance::register(&srv, "f", DamageMode::Arcade, "open", racer_slots(2));
    let admin = srv.admin();
    admin
        .admin_set_flag("player_police_enabled", json!(false))
        .unwrap();
    admin
        .admin_set_flag("illegal_day_race_frequency", json!(0.4))
        .unwrap();
    let (_, cop) = player(&srv, "wannabe");
    assert_eq!(
        cop.queue(&prefs(Role::PoliceChase, DamageMode::Arcade, "D"))
            .unwrap_err()
            .code(),
        Some("feature_disabled")
    );
    let flags = inst.heartbeat().flags;
    assert_eq!(flags["illegal_day_race_frequency"], json!(0.4));
    assert_eq!(srv.client().flags().unwrap(), flags);
    admin.admin_delete_flag("player_police_enabled").unwrap();
    cop.queue(&prefs(Role::PoliceChase, DamageMode::Arcade, "D"))
        .unwrap();
    assert_eq!(
        admin
            .admin_set_flag("Bad Key", json!(1))
            .unwrap_err()
            .status(),
        Some(400)
    );
    let audit = admin.admin_audit(10).unwrap();
    assert_eq!(audit[0].action, "flag_delete");
    assert_eq!(audit[1].details["old"], json!(0.15));
}

#[test]
fn admin_requires_the_admin_token() {
    let srv = TestServer::start();
    assert_eq!(srv.client().admin_health().unwrap_err().status(), Some(401));
    assert_eq!(
        srv.client()
            .with_token("test-admin-tokem")
            .admin_health()
            .unwrap_err()
            .status(),
        Some(401)
    );
    let (_, p) = player(&srv, "notadmin");
    assert_eq!(p.admin_health().unwrap_err().status(), Some(401));

    let off = TestServer::start_with(Config {
        admin_token: String::new(),
        ..Config::test()
    });
    assert_eq!(
        off.client()
            .with_token("")
            .admin_health()
            .unwrap_err()
            .code(),
        Some("admin_disabled")
    );
}

#[test]
fn temporary_bans_expire_and_debits_cannot_create_debt() {
    let srv = TestServer::start();
    let (id, _) = player(&srv, "naughty");
    let admin = srv.admin();
    let view = admin.admin_ban(id, "spam", Some(3600)).unwrap();
    assert_eq!(view.banned_until_ms, Some(START_MS + 3_600_000));
    let login = || {
        srv.client()
            .login("naughty@example.test", "hunter22-password")
    };
    assert_eq!(login().unwrap_err().code(), Some("banned"));
    srv.advance(3_600_001);
    login().unwrap();

    let cash = view.economy.unwrap().cash;
    let err = admin.admin_adjust(id, -(cash + 1), "too much").unwrap_err();
    assert_eq!(err.code(), Some("economy_refused"));
    assert_eq!(
        admin.admin_adjust(id, 10, "").unwrap_err().status(),
        Some(400),
        "a note is required"
    );
    assert_eq!(admin.admin_adjust(id, -cash, "zero out").unwrap().cash, 0);
    assert_eq!(admin.admin_account(999).unwrap_err().status(), Some(404));
}

#[test]
fn leaderboard_periods_roll_and_reset() {
    let srv = TestServer::start();
    let inst = FakeInstance::register(&srv, "lb", DamageMode::Arcade, "open", racer_slots(4));
    let (racer, c) = player(&srv, "wanted");
    matchmake(
        &c,
        Some(&prefs(Role::Racer, DamageMode::Arcade, "D")),
        &[Arc::clone(&inst)],
    );
    let pursuit = |key: &str, bounty| {
        inst.client
            .report_pursuit(
                inst.id,
                &PursuitReport {
                    report_key: key.into(),
                    racer_id: racer,
                    max_wanted: WantedLevel::L2,
                    bounty,
                    outcome: PursuitOutcome::Escaped,
                    captured_by: None,
                    assists: vec![],
                },
            )
            .unwrap()
    };
    pursuit("a", 5_000);
    assert!(pursuit("a", 5_000).duplicate);
    pursuit("b", 3_000);
    let anon = srv.client();
    let top = |period: &str| anon.leaderboard("bounty", period, 10).unwrap();
    assert_eq!(top("daily").rows[0].score, 5_000.0, "max, not latest");

    // Next day: daily is empty, weekly and all-time keep it.
    srv.advance(24 * 3_600_000);
    assert!(top("daily").rows.is_empty());
    assert_eq!(top("weekly").rows[0].score, 5_000.0);
    // Next week: weekly empty too.
    srv.advance(7 * 24 * 3_600_000);
    assert!(top("weekly").rows.is_empty());
    assert_eq!(top("all").rows[0].score, 5_000.0);

    // Admin reset of the all-time board.
    srv.admin().admin_reset_board("bounty", "all").unwrap();
    assert!(top("all").rows.is_empty());

    assert_eq!(
        anon.leaderboard("nope", "all", 10).unwrap_err().status(),
        Some(404)
    );
    assert_eq!(
        anon.leaderboard("bounty", "monthly", 10)
            .unwrap_err()
            .status(),
        Some(400)
    );
    // Reports about players the instance never hosted are refused.
    let (stranger, _) = player(&srv, "stranger");
    let err = inst
        .client
        .report_pursuit(
            inst.id,
            &PursuitReport {
                report_key: "c".into(),
                racer_id: stranger,
                max_wanted: WantedLevel::L1,
                bounty: 1,
                outcome: PursuitOutcome::Escaped,
                captured_by: None,
                assists: vec![],
            },
        )
        .unwrap_err();
    assert_eq!(err.code(), Some("not_hosted"));
}

#[test]
fn crews_and_territory() {
    let srv = TestServer::start();
    let inst = FakeInstance::register(&srv, "turf", DamageMode::Arcade, "open", racer_slots(4));
    let (boss, b) = player(&srv, "boss");
    let (grunt, g) = player(&srv, "grunt");
    let crew: CrewInfo = b
        .call(
            "POST",
            "/v1/crews",
            Some(&CreateCrewRequest {
                name: "Night Shift".into(),
                tag: "nsx".into(),
            }),
        )
        .unwrap();
    assert_eq!(crew.tag, "NSX");
    let err = g
        .call_empty(
            "POST",
            "/v1/crews",
            Some(&CreateCrewRequest {
                name: "night shift".into(),
                tag: "ZZ".into(),
            }),
        )
        .unwrap_err();
    assert_eq!(err.code(), Some("crew_taken"));
    let joined: CrewInfo = g
        .call(
            "POST",
            &format!("/v1/crews/{}/join", crew.crew_id),
            None::<&()>,
        )
        .unwrap();
    assert_eq!(joined.members, [boss, grunt]);
    assert_eq!(g.me().unwrap().crew_id, Some(crew.crew_id));

    inst.client
        .report_territory(
            inst.id,
            &TerritoryReport {
                district_id: "docks".into(),
                crew_id: Some(crew.crew_id),
            },
        )
        .unwrap();
    let get = || -> CrewInfo {
        srv.client()
            .call("GET", &format!("/v1/crews/{}", crew.crew_id), None::<&()>)
            .unwrap()
    };
    assert_eq!(get().territory, ["harbor_city/docks"]);

    b.call_empty("POST", "/v1/crews/leave", None::<&()>)
        .unwrap();
    assert_eq!(get().leader_id, grunt, "leadership passes on");
    g.call_empty("POST", "/v1/crews/leave", None::<&()>)
        .unwrap();
    let err = srv
        .client()
        .call_empty("GET", &format!("/v1/crews/{}", crew.crew_id), None::<&()>)
        .unwrap_err();
    assert_eq!(err.status(), Some(404), "empty crew disbanded");
}

#[test]
fn data_survives_a_restart() {
    let dir = std::env::temp_dir().join(format!("wp_meta_restart_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("meta.sqlite");
    let cfg = Config {
        db_path: Some(db.clone()),
        ..Config::test()
    };
    let id = {
        let srv = TestServer::start_with(cfg.clone());
        player(&srv, "durable").0
    };
    let srv = TestServer::start_with(cfg);
    let s = srv
        .client()
        .login("durable@example.test", "hunter22-password")
        .unwrap();
    assert_eq!(s.account_id, id);
    drop(srv);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_route_is_in_the_openapi_spec() {
    let spec = wp_meta::routes::OPENAPI_YAML;
    for (method, path) in wp_meta::routes::ROUTES {
        let path_line = format!("\n  {path}:\n");
        let start = spec
            .find(&path_line)
            .unwrap_or_else(|| panic!("{path} missing from openapi.yaml"));
        // The path's block runs until the next top-level path entry.
        let rest = &spec[start + path_line.len() - 1..];
        let end = rest.find("\n  /").unwrap_or(rest.len());
        assert!(
            rest[..end].contains(&format!("\n    {method}:")),
            "{method} {path} missing from openapi.yaml"
        );
    }
}

#[test]
fn only_the_current_host_touches_an_account() {
    let srv = TestServer::start();
    let old = FakeInstance::register(&srv, "old", DamageMode::Arcade, "D", racer_slots(2));
    let (id, c) = player(&srv, "mover");
    matchmake(
        &c,
        Some(&prefs(Role::Racer, DamageMode::Arcade, "D")),
        &[Arc::clone(&old)],
    );
    let old_store = RemoteAccountStore::new(old.client.clone(), old.id);
    assert!(old_store.load(PlayerId(id)).unwrap().is_some());

    // The player moves to another server; the old one loses access.
    let new = FakeInstance::register(&srv, "new", DamageMode::Simulation, "D", racer_slots(2));
    matchmake(
        &c,
        Some(&prefs(Role::Racer, DamageMode::Simulation, "D")),
        &[Arc::clone(&new)],
    );
    assert!(old_store.load(PlayerId(id)).is_err());
    let new_store = RemoteAccountStore::new(new.client.clone(), new.id);
    assert!(new_store.load(PlayerId(id)).unwrap().is_some());
}

/// Regression (lead run 3 flake): a verified join must hold its seat until a heartbeat
/// reports the player, or the matcher overbooks the instance in between.
#[test]
fn joined_players_hold_their_seat_until_reported() {
    let srv = TestServer::start();
    let inst = FakeInstance::register(&srv, "tiny", DamageMode::Arcade, "open", racer_slots(1));
    let (first, a) = player(&srv, "first");
    matchmake(
        &a,
        Some(&prefs(Role::Racer, DamageMode::Arcade, "D")),
        &[Arc::clone(&inst)],
    );
    // No heartbeat yet: the reported population is still 0.
    let (_, b) = player(&srv, "second");
    let st = b
        .queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
        .unwrap();
    assert_eq!(
        st.state,
        TicketState::Queued,
        "seat still held by the joined player"
    );
    inst.heartbeat();
    assert_eq!(b.ticket_status().unwrap().state, TicketState::Queued);
    let health = srv.admin().admin_health().unwrap();
    assert_eq!(health.instances[0].population.racer, 1);
    assert_eq!(
        health.instances[0].reserved.total(),
        0,
        "hold released once reported"
    );
    // The first player leaves; the next heartbeat frees the seat.
    inst.disconnect(first);
    inst.heartbeat();
    assert_eq!(b.ticket_status().unwrap().state, TicketState::Assigned);
}

/// An instance that never lists players stops holding seats after the hold period.
#[test]
fn unreported_join_holds_expire() {
    let srv = TestServer::start();
    let inst = FakeInstance::register(&srv, "quiet", DamageMode::Arcade, "open", racer_slots(1));
    let (joined, a) = player(&srv, "joined");
    matchmake(
        &a,
        Some(&prefs(Role::Racer, DamageMode::Arcade, "D")),
        &[Arc::clone(&inst)],
    );
    inst.disconnect(joined);
    let (_, b) = player(&srv, "waiter");
    assert_eq!(
        b.queue(&prefs(Role::Racer, DamageMode::Arcade, "D"))
            .unwrap()
            .state,
        TicketState::Queued
    );
    srv.advance(wp_meta::directory::UNREPORTED_JOIN_HOLD_MS + 1);
    // Heartbeat with an empty player list keeps the instance alive.
    inst.heartbeat();
    assert_eq!(b.ticket_status().unwrap().state, TicketState::Assigned);
}
