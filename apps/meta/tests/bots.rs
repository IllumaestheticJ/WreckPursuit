//! End-to-end: the real service on a socket, three fake instances, 20 bots going through
//! signup → party → matchmake → join → race/pursuit results (with real economy settlement over
//! the instance API) → leaderboards, plus reconnect, kick and ban along the way.

mod common;

use std::sync::Arc;
use std::thread;

use common::*;
use wp_contracts::{DamageMode, DayPhase, PlayerId, Role, VehicleClass, WantedLevel};
use wp_economy::{AccountStore, Economy, RaceResult, RaceSetup, RaceType, ViolationCounts};
use wp_meta::api::*;
use wp_meta::client::{MetaClient, RemoteAccountStore};

const BOTS: usize = 20;

/// Role / mode / bracket of bot `i`.
fn bot_prefs(i: usize) -> MatchPrefs {
    match i {
        0..=9 => prefs(Role::Racer, DamageMode::Arcade, "C"),
        10..=13 => prefs(Role::Racer, DamageMode::Simulation, "B"),
        14..=18 => prefs(Role::PoliceChase, DamageMode::Arcade, "C"),
        _ => prefs(Role::PoliceCaptain, DamageMode::Arcade, "C"),
    }
}

#[test]
fn twenty_bots_signup_matchmake_results_leaderboard() {
    let srv = TestServer::start();
    let eco = Economy::embedded();

    // ---- instances: two Arcade/open, one Simulation/B.
    let arcade_slots = RoleCounts {
        racer: 6,
        police_chase: 3,
        police_captain: 1,
        spectator: 2,
    };
    let a1 = FakeInstance::register(&srv, "arcade-1", DamageMode::Arcade, "open", arcade_slots);
    let a2 = FakeInstance::register(&srv, "arcade-2", DamageMode::Arcade, "open", arcade_slots);
    let s1 = FakeInstance::register(
        &srv,
        "sim-b-1",
        DamageMode::Simulation,
        "B",
        RoleCounts {
            racer: 6,
            police_chase: 2,
            police_captain: 1,
            spectator: 0,
        },
    );
    let instances = vec![a1.clone(), a2.clone(), s1.clone()];
    assert_eq!(srv.client().instances().unwrap().len(), 3);

    // ---- phase 1: 20 bots sign up in parallel.
    let bots: Vec<(AccountId, MetaClient)> = (0..BOTS)
        .map(|i| {
            let base = srv.base.clone();
            thread::spawn(move || {
                let name = format!("bot{i:02}");
                let s = MetaClient::new(&base)
                    .signup(&format!("{name}@bots.test"), "bot-password-123", &name)
                    .expect("signup");
                // Log in again (separate session) to exercise login.
                let s2 = MetaClient::new(&base)
                    .login(&format!("{name}@bots.test"), "bot-password-123")
                    .expect("login");
                assert_eq!(s.account_id, s2.account_id);
                (s.account_id, MetaClient::new(&base).with_token(&s2.token))
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    for (id, c) in &bots {
        let acct = c.my_economy().unwrap();
        assert_eq!(acct.player, PlayerId(*id));
        assert!(acct.cash > 0, "starting cash from the economy tables");
    }

    // ---- phase 2: bots 0,1,2 form a party led by bot 0.
    let party = bots[0].1.create_party().unwrap();
    for b in &bots[1..3] {
        b.1.join_party(party.party_id, &party.code).unwrap();
    }

    // ---- phase 3: everyone matchmakes in parallel (party members just poll).
    let seats: Vec<Seat> = (0..BOTS)
        .map(|i| {
            let c = bots[i].1.clone();
            let inst = instances.clone();
            thread::spawn(move || {
                let p = bot_prefs(i);
                let queue = !(i == 1 || i == 2);
                if !queue {
                    // Wait until the leader has queued the party.
                    while c.ticket_status().is_err() {
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
                matchmake(&c, queue.then_some(&p), &inst)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    for i in &instances {
        let hb = i.heartbeat();
        assert!(hb.flags.contains_key("illegal_day_race_frequency"));
        assert!(hb.kicks.is_empty());
    }

    // Everyone seated, in the role they asked for, on a compatible instance.
    for (i, seat) in seats.iter().enumerate() {
        assert_eq!(seat.account_id, bots[i].0);
        assert_eq!(seat.role, bot_prefs(i).role);
        let on_sim = s1.seats.lock().unwrap().contains_key(&seat.account_id);
        assert_eq!(on_sim, (10..=13).contains(&i), "bot {i} on the wrong mode");
    }
    let total: u32 = instances.iter().map(|i| i.population().total()).sum();
    assert_eq!(total, BOTS as u32);
    // The party sits together.
    let party_home = instances
        .iter()
        .find(|i| i.seats.lock().unwrap().contains_key(&bots[0].0))
        .unwrap();
    for b in &bots[0..3] {
        assert!(party_home.seats.lock().unwrap().contains_key(&b.0));
        assert_eq!(
            party_home.seats.lock().unwrap()[&b.0].party_id,
            Some(party.party_id)
        );
    }
    // Capacity respected; fullest-first filling put all 4 sim racers on the one sim server.
    for i in &instances {
        let p = i.population();
        assert!(p.police_captain <= 1);
        assert!(p.racer <= 6);
    }
    assert_eq!(s1.population().racer, 4);
    let health = srv.admin().admin_health().unwrap();
    assert_eq!(health.accounts, BOTS as u32);
    assert_eq!(health.queued_tickets, 0);
    assert!(health.instances.iter().all(|i| i.reserved.total() == 0));

    // ---- phase 4: each instance runs a race, settles the economy over HTTP, reports results.
    let setup = |mode| RaceSetup {
        race_type: RaceType::Sprint,
        kind: None,
        legality: Default::default(),
        class: VehicleClass::Compact,
        route_length_m: 4000.0,
        phase: DayPhase::Night,
        mode,
    };
    for (n, inst) in [&a1, &a2].iter().enumerate() {
        let racers = inst.players_with_role(Role::Racer);
        assert!(racers.len() >= 2, "each arcade instance has a race");
        let mut store = RemoteAccountStore::new(inst.client.clone(), inst.id);
        let mut entries = Vec::new();
        for (pos, racer) in racers.iter().enumerate() {
            let placement = pos as u8 + 1;
            // Every third finisher bumped someone: not clean.
            let clean = pos % 3 != 1;
            let mut acct = store.load(PlayerId(*racer)).unwrap().expect("account");
            let car = match acct.active_car {
                Some(c) => c,
                None => {
                    let (c, ev) = eco.issue_floor_car(&mut acct).unwrap();
                    store.commit(&acct, &ev).unwrap();
                    c
                }
            };
            let result = RaceResult {
                placement,
                contacts: if clean { 0 } else { 2 },
                violations: ViolationCounts::default(),
                wanted: Default::default(),
            };
            let (_, ev) = eco
                .settle_race(&mut acct, &setup(DamageMode::Arcade), car, &result)
                .unwrap();
            store.commit(&acct, &ev).unwrap();
            entries.push(RaceEntry {
                account_id: *racer,
                position: placement as u32,
                finish_time_ms: Some(180_000 + pos as u64 * 1_500),
                clean,
            });
        }
        let report = RaceReport {
            report_key: format!("arcade-{n}-race-1"),
            route_id: "docks_sprint".into(),
            race_type: "sprint".into(),
            mode: DamageMode::Arcade,
            bracket: "C".into(),
            entries,
        };
        let r = inst.client.report_race(inst.id, &report).unwrap();
        assert!(!r.duplicate);
        assert_eq!(r.ratings.len(), racers.len());
        let sum: f64 = r.ratings.iter().map(|c| c.after - c.before).sum();
        assert!(sum.abs() < 1e-6, "rating is zero-sum within a race");
        // A retried report is acknowledged, not double counted.
        let again = inst.client.report_race(inst.id, &report).unwrap();
        assert!(again.duplicate);
        assert_eq!(again.race_id, r.race_id);
    }

    // Sim instance: time attack on one route.
    let sim_racers = s1.players_with_role(Role::Racer);
    let ta = RaceReport {
        report_key: "sim-ta-1".into(),
        route_id: "hill_climb".into(),
        race_type: "time_attack".into(),
        mode: DamageMode::Simulation,
        bracket: "B".into(),
        entries: sim_racers
            .iter()
            .enumerate()
            .map(|(k, id)| RaceEntry {
                account_id: *id,
                position: k as u32 + 1,
                finish_time_ms: Some(95_000 - k as u64 * 700),
                clean: k != 0,
            })
            .collect(),
    };
    s1.client.report_race(s1.id, &ta).unwrap();

    // Pursuits on arcade-1: one capture by a player cop, one escape.
    let a1_racers = a1.players_with_role(Role::Racer);
    let a1_cops = a1.players_with_role(Role::PoliceChase);
    let a2_cops = a2.players_with_role(Role::PoliceChase);
    let (capturing_inst, cop) = if let Some(c) = a1_cops.first() {
        (&a1, *c)
    } else {
        (&a2, a2_cops[0])
    };
    let victim = capturing_inst.players_with_role(Role::Racer)[0];
    capturing_inst
        .client
        .report_pursuit(
            capturing_inst.id,
            &PursuitReport {
                report_key: "p-1".into(),
                racer_id: victim,
                max_wanted: WantedLevel::L3,
                bounty: 4_200,
                outcome: PursuitOutcome::Captured,
                captured_by: Some(cop),
                assists: vec![],
            },
        )
        .unwrap();
    a1.client
        .report_pursuit(
            a1.id,
            &PursuitReport {
                report_key: "p-2".into(),
                racer_id: a1_racers[1],
                max_wanted: WantedLevel::L4,
                bounty: 9_000,
                outcome: PursuitOutcome::Escaped,
                captured_by: None,
                assists: vec![],
            },
        )
        .unwrap();

    // ---- phase 5: leaderboards.
    let anon = srv.client();
    let rating = anon.leaderboard("clean_rating", "all", 50).unwrap();
    let arcade_racers =
        a1.players_with_role(Role::Racer).len() + a2.players_with_role(Role::Racer).len();
    assert_eq!(
        rating.rows.len(),
        arcade_racers,
        "time attack does not rate"
    );
    assert!(rating.rows.windows(2).all(|w| w[0].score >= w[1].score));
    assert!(rating.rows[0].score > 1500.0 && rating.rows.last().unwrap().score < 1500.0);
    let daily = anon.leaderboard("clean_rating", "daily", 50).unwrap();
    assert_eq!(daily.period, "d:2026-10-19");
    let gained: f64 = daily.rows.iter().map(|r| r.score).sum();
    assert!(gained.abs() < 1e-6, "daily board holds rating gained");

    let ta_board = anon
        .leaderboard("time_attack:hill_climb", "weekly", 10)
        .unwrap();
    assert!(ta_board.ascending);
    assert_eq!(ta_board.period, "w:2026-10-19");
    assert_eq!(
        ta_board.rows.len(),
        sim_racers.len() - 1,
        "dirty run excluded"
    );
    assert!(ta_board.rows.windows(2).all(|w| w[0].score <= w[1].score));

    let bounty = anon.leaderboard("bounty", "all", 10).unwrap();
    assert_eq!(bounty.rows[0].score, 9_000.0);
    assert_eq!(bounty.rows[0].account_id, a1_racers[1]);
    let captures = anon.leaderboard("captures", "daily", 10).unwrap();
    assert_eq!(captures.rows.len(), 1);
    assert_eq!(captures.rows[0].account_id, cop);

    // Economy settled through the meta store: every arcade racer raced once.
    for id in a1.players_with_role(Role::Racer) {
        let acct = srv.admin().admin_account(id).unwrap().economy.unwrap();
        assert!(acct.stats.total_earned > 0 || acct.garage.len() == 1);
        assert!(!acct.garage.is_empty(), "floor car issued");
    }

    // ---- phase 6: reconnect. Bot 5 drops and comes back to the same instance.
    let home = instances
        .iter()
        .find(|i| i.seats.lock().unwrap().contains_key(&bots[5].0))
        .unwrap()
        .clone();
    let token = home.seats.lock().unwrap()[&bots[5].0]
        .reconnect_token
        .clone();
    home.disconnect(bots[5].0);
    home.heartbeat();
    let st = bots[5].1.reconnect(&token).unwrap();
    let a = st.assignment.expect("reconnect assignment");
    assert_eq!(a.instance_id, home.id);
    let seat = home.connect(&a.join_token).unwrap();
    assert_eq!(seat.role, Role::Racer);
    assert!(
        bots[5].1.reconnect(&token).is_err(),
        "reconnect token is one-time"
    );

    // ---- phase 7: moderation. Kick bot 6, ban bot 7.
    let admin = srv.admin();
    admin.admin_kick(bots[6].0, "afk").unwrap();
    admin.admin_ban(bots[7].0, "cheating", None).unwrap();
    let mut kicked = Vec::new();
    for i in &instances {
        kicked.extend(i.heartbeat().kicks);
    }
    assert_eq!(kicked.len(), 2);
    let reason_of = |id| &kicked.iter().find(|k| k.account_id == id).unwrap().reason;
    assert_eq!(reason_of(bots[6].0), "afk");
    assert!(reason_of(bots[7].0).starts_with("banned"));
    for i in &instances {
        assert!(i.heartbeat().kicks.is_empty(), "kicks are delivered once");
    }
    let err = bots[7].1.me().unwrap_err();
    assert_eq!(err.status(), Some(401), "ban revoked sessions");
    let err = srv
        .client()
        .login("bot07@bots.test", "bot-password-123")
        .unwrap_err();
    assert_eq!(err.code(), Some("banned"));

    let before = admin
        .admin_account(bots[8].0)
        .unwrap()
        .economy
        .unwrap()
        .cash;
    let after = admin
        .admin_adjust(bots[8].0, 2_500, "compensation for outage")
        .unwrap();
    assert_eq!(after.cash, before + 2_500);
    let audit = admin.admin_audit(10).unwrap();
    let actions: Vec<&str> = audit.iter().map(|a| a.action.as_str()).collect();
    assert_eq!(actions, ["economy_adjust", "ban", "kick"]);
    assert!(audit.iter().all(|a| a.actor == "test-admin"));
}

/// The instance-facing store refuses accounts that never joined that instance.
#[test]
fn instances_only_touch_hosted_accounts() {
    let srv = TestServer::start();
    let inst = FakeInstance::register(
        &srv,
        "solo",
        DamageMode::Arcade,
        "open",
        RoleCounts {
            racer: 4,
            ..Default::default()
        },
    );
    let (stranger, _) = player(&srv, "stranger");
    let store = RemoteAccountStore::new(inst.client.clone(), inst.id);
    assert!(store.load(PlayerId(stranger)).is_err());
    let (id, c) = player(&srv, "guest");
    matchmake(
        &c,
        Some(&prefs(Role::Racer, DamageMode::Arcade, "D")),
        &[Arc::clone(&inst)],
    );
    assert!(store.load(PlayerId(id)).unwrap().is_some());
}
