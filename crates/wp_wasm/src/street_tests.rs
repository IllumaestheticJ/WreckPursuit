//! Native tests of the street racers (`crate::street`) inside a live city session: roster and
//! calm roaming, challenge → race → result at noon and at night, the never-give-up rule in a
//! race (spin, crash, police), replays and crash replays of street sessions, and no racers
//! outside live free drive.

use wp_contracts::{DamageMode, EntityId, SimEvent, VehicleInput, VehicleState};

use crate::crash_replay::{CrashMark, ImpactSide};
use crate::scenario::free_drive;
use crate::session::Session;
use crate::street::{self, RaceResult, RacerMode, ROSTER_LEVELS};

const PLAYER: EntityId = EntityId(1);

fn state(s: &Session, id: EntityId) -> VehicleState {
    s.sim()
        .vehicle_states()
        .into_iter()
        .find(|(e, _)| *e == id)
        .map(|(_, st)| st)
        .unwrap()
}

fn yaw_of(q: [f32; 4]) -> f32 {
    let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
    let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    wp_math::atan2(-fx, fy)
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Free drive on the procedural grid at `hour` (12 = noon, 23 = the city chase's night).
fn city_session_at(hour: f32) -> Session {
    let mut sc = free_drive("debug_sedan", DamageMode::Arcade, true);
    sc.time_of_day_h = hour;
    Session::new(sc, false).unwrap()
}

fn city_session() -> Session {
    city_session_at(12.0)
}

/// The player's idle input: on the handbrake (a held brake pedal at a standstill reverses).
fn horn(on: bool) -> VehicleInput {
    VehicleInput {
        handbrake: 1.0,
        horn: on,
        headlights: true,
        ..Default::default()
    }
}

/// Put the player 9 m behind street racer `k` (same lane and heading), stopped. A logged
/// move, so recordings of the session replay it.
fn park_behind(s: &mut Session, k: usize) -> EntityId {
    let racer = s.street().unwrap().entities()[k];
    let st = state(s, racer);
    let yaw = yaw_of(st.rot);
    let (sn, cs) = wp_math::sin_cos(yaw);
    let xy = [st.pos[0] + sn * 9.0, st.pos[1] - cs * 9.0];
    s.place_logged(PLAYER, xy, yaw, st.pos[2]).unwrap();
    racer
}

/// Honk next to racer `k`: the offer opens for it.
fn challenge(s: &mut Session, k: usize) -> EntityId {
    let racer = park_behind(s, k);
    s.set_input(PLAYER, horn(true)).unwrap();
    s.run_ticks(2);
    s.set_input(PLAYER, horn(false)).unwrap();
    s.run_ticks(2);
    let offer = s.street().unwrap().offer(s.sim(), s.run()).expect("offer");
    assert_eq!(offer.entity, racer.0);
    racer
}

/// A finish 400–900 m of road away that the planner accepts.
fn finish_for(s: &Session) -> [f32; 2] {
    let p = state(s, PLAYER).pos;
    for lane in &s.sim().world().network().lanes {
        let q = lane.polyline[lane.polyline.len() / 2];
        let d = dist2(q, p);
        if !(300.0..=600.0).contains(&d) {
            continue;
        }
        if let Ok(pr) = s.street_preview([q[0], q[1]]) {
            if pr.ok && (400.0..=900.0).contains(&pr.length_m) {
                return pr.finish;
            }
        }
    }
    panic!("no finish found");
}

fn cash(s: &Session) -> i64 {
    s.run().unwrap().account().cash
}

/// Challenge racer `k`, accept the level's minimum stake to a 400–900 m finish and run the
/// countdown. Returns the racer.
fn start_race(s: &mut Session, k: usize) -> EntityId {
    let racer = challenge(s, k);
    let fin = finish_for(s);
    let lv = s.street().unwrap().roster()[k].level;
    s.street_accept(fin, street::level(lv).bet_min).unwrap();
    s.run_ticks(street::COUNTDOWN_TICKS + 2);
    racer
}

/// Run until the race is decided (the player stays on the handbrake), calling `each` every
/// tick with the seconds since the call.
fn race_to_result(
    s: &mut Session,
    max_s: u64,
    mut each: impl FnMut(&mut Session, f32),
) -> RaceResult {
    let seq = s.street().unwrap().result().map_or(0, |r| r.seq);
    for t in 0..240 * max_s {
        s.set_input(PLAYER, horn(false)).unwrap();
        s.run_ticks(1);
        each(s, t as f32 / 240.0);
        if let Some(r) = s.street().unwrap().result().filter(|r| r.seq > seq) {
            return r.clone();
        }
    }
    panic!("race not decided within {max_s} s");
}

/// Roaming check after a stretch of free drive: every racer (one spared) roamed more than
/// 50 m from its spawn or stands at a red light, and at least three in four actually roamed.
/// The grid's signals are fixed-time with 56–94 s cycles, so a careful racer that spawned
/// before a long red can still be waiting for its first green.
fn assert_roamed(s: &Session, ids: &[EntityId], start: &[[f32; 3]], what: &str) {
    let st = s.street().unwrap();
    let mut moved = 0;
    let mut at_red = 0;
    for (k, (e, p0)) in ids.iter().zip(start).enumerate() {
        if dist2(state(s, *e).pos, *p0) > 50.0 {
            moved += 1;
        } else if st.waiting_at_red(s.sim(), k) {
            at_red += 1;
        }
    }
    eprintln!(
        "{what}: {moved} of {} roamed, {at_red} waiting at a red",
        ids.len()
    );
    assert!(
        moved + at_red >= ids.len() - 1,
        "{what}: only {moved} of {} racers roamed ({at_red} at a red)",
        ids.len()
    );
    assert!(
        4 * moved >= 3 * ids.len(),
        "{what}: only {moved} of {} roamed",
        ids.len()
    );
}

#[test]
fn city_session_spawns_the_roster_and_racers_roam_calmly() {
    let mut s = city_session();
    let st = s.street().expect("street racers in a city session");
    let roster = st.roster();
    assert_eq!(roster.len(), ROSTER_LEVELS.len());
    for (r, lv) in roster.iter().zip(ROSTER_LEVELS) {
        assert_eq!(r.level, lv);
        assert_eq!(r.colour, street::level(lv).colour);
        assert!(street::level(lv).cars.contains(&r.car.as_str()));
    }
    // Distinct colours per level, in level order.
    let colours: Vec<_> = street::LEVELS.iter().map(|l| l.colour).collect();
    for (i, c) in colours.iter().enumerate() {
        assert!(!colours[i + 1..].contains(c));
    }
    // Scenario entities after the player, AI racers to the police and traffic.
    let ids = st.entities();
    assert_eq!(&s.entities()[1..], &ids[..]);
    for e in &ids {
        let v = s.vehicle(*e).unwrap();
        assert_eq!(v.kind, wp_sim::VehicleKind::AiRacer);
        assert!(street::is_street_entity(s.spec(*e).unwrap()));
    }
    let start: Vec<_> = ids.iter().map(|e| state(&s, *e).pos).collect();
    let mut violations = Vec::new();
    let (mut top, mut sum, mut n) = (0.0f32, 0.0f32, 0u32);
    for _ in 0..40 {
        s.run_ticks(240);
        violations.extend(s.drain_events().into_iter().filter_map(|e| match e {
            SimEvent::TrafficViolation { vehicle, kind, .. } if ids.contains(&vehicle) => {
                Some(kind)
            }
            _ => None,
        }));
        for e in &ids {
            let v = state(&s, *e).speed_mps;
            top = top.max(v);
            if v > 1.0 {
                sum += v;
                n += 1;
            }
        }
    }
    let mean = sum / n.max(1) as f32;
    eprintln!("roaming: top {top:.1} m/s, mean moving {mean:.1} m/s, violations {violations:?}");
    assert!(
        violations.is_empty(),
        "roaming racers should not draw the police: {violations:?}"
    );
    assert_roamed(&s, &ids, &start, "noon");
    // Cruising at traffic pace, not racing: under the 25 mph street's speeding threshold
    // (13.4 m/s) plus a little overshoot, and not crawling either.
    assert!(top < 15.0, "a roaming racer reached {top} m/s");
    assert!((5.0..=13.5).contains(&mean), "mean roaming speed {mean}");
    let packed = s.street().unwrap().packed(s.sim());
    assert_eq!(packed.len(), 6 * ids.len());
}

#[test]
fn street_racers_only_in_live_free_drive_and_can_be_turned_off() {
    crate::session::set_street_racers_enabled(false);
    let s = city_session();
    crate::session::set_street_racers_enabled(true);
    assert!(s.street().is_none());
    assert_eq!(s.entities().len(), 1, "no racer entities either");
    let flat = Session::new(free_drive("debug_sedan", DamageMode::Arcade, false), false).unwrap();
    assert!(
        flat.street().is_none(),
        "no street racers on the flat world"
    );
    // Scripted and simcli scenario files never get them (goldens and menu runs stay as they
    // are), even run live.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/simcli/scenarios/golden/city_chase.ron"
    );
    let sc = crate::scenario::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(!street::scenario_wants_racers(&sc));
    for scripted in [true, false] {
        let s = Session::new(sc.clone(), scripted).unwrap();
        assert!(s.street().is_none());
        assert_eq!(s.entities().len(), sc.entities.len());
    }
    // Nor a picker race.
    let def = wp_race::catalog::race("grid_highway_sprint").unwrap();
    let sc = crate::race::race_scenario(&def, "debug_sedan", DamageMode::Arcade, false).unwrap();
    assert!(!street::scenario_wants_racers(&sc));
    // A scenario saved from a street session keeps its racers and gets no second roster.
    let s = city_session();
    let again = Session::new(s.scenario().clone(), false).unwrap();
    assert_eq!(again.entities().len(), s.entities().len());
    assert_eq!(
        again.street().unwrap().entities(),
        s.street().unwrap().entities()
    );
}

#[test]
fn honk_opens_a_challenge_and_bad_bets_are_refused() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    let racer = challenge(&mut s, 2);
    let offer = s.street().unwrap().offer(s.sim(), s.run()).expect("offer");
    assert_eq!(offer.level, ROSTER_LEVELS[2]);
    assert!(offer.affordable);
    // The racer pulls over and waits (on the handbrake: it does not creep backwards).
    s.run_ticks(240 * 3);
    assert!(state(&s, racer).speed_mps.abs() < 1.0);
    let parked = state(&s, racer).pos;
    s.run_ticks(240 * 2);
    assert!(dist2(state(&s, racer).pos, parked) < 0.5, "waits in place");
    assert_eq!(s.street().unwrap().roster()[2].mode, RacerMode::Waiting);
    let fin = finish_for(&s);
    let before = cash(&s);
    assert!(
        s.street_accept(fin, offer.bet_min - 1).is_err(),
        "under the minimum"
    );
    assert!(
        s.street_accept(fin, before + 1).is_err(),
        "more than the cash"
    );
    let p = state(&s, PLAYER).pos;
    assert!(
        s.street_accept([p[0] + 1.0, p[1]], offer.bet_min).is_err(),
        "too short"
    );
    assert_eq!(cash(&s), before, "refused bets cost nothing");
    s.street_decline();
    assert!(s.street().unwrap().offer(s.sim(), s.run()).is_none());
    s.run_ticks(240 * 6);
    assert!(
        state(&s, racer).speed_mps > 2.0,
        "declined racer drives off"
    );
}

/// Challenge → race → result: the racer beats a player who stays put; a player at B first
/// wins even money. `hour` 12 (noon) or 23 (night: headlights on).
fn challenge_race_result(hour: f32) {
    let mut s = city_session_at(hour);
    s.run_ticks(240 * 2);
    let night = !(6.0..20.0).contains(&hour);
    // Race 1: the player stays on the handbrake; the racer wins.
    let racer = challenge(&mut s, 0);
    if night {
        assert!(s.input(racer).headlights, "waiting racer has its lights on");
    }
    let fin = finish_for(&s);
    let stake = street::level(ROSTER_LEVELS[0]).bet_min;
    let before = cash(&s);
    s.street_accept(fin, stake).unwrap();
    assert_eq!(cash(&s), before - stake, "stake paid up front");
    assert!(s.held(PLAYER), "countdown holds the player");
    let lined_up = state(&s, PLAYER).pos;
    assert!(
        dist2(state(&s, racer).pos, lined_up) < 12.0,
        "racer lined up beside the player"
    );
    s.run_ticks(street::COUNTDOWN_TICKS + 2);
    assert!(!s.held(PLAYER));
    assert!(
        dist2(state(&s, PLAYER).pos, lined_up) < 0.5,
        "the player waited the countdown in place"
    );
    let mut lights = true;
    let mut top = 0.0f32;
    let r = race_to_result(&mut s, 180, |s, _| {
        lights &= s.input(racer).headlights;
        top = top.max(state(s, racer).speed_mps);
    });
    eprintln!("hour {hour} race 1: {r:?}, racer top {top:.1} m/s");
    assert!(!r.won, "{r:?}");
    assert!(r.reason.contains("crossed the line"), "{r:?}");
    // 400–900 m at a plausible street pace (not teleported, not crawling, not cruising).
    assert!((15.0..=120.0).contains(&r.time_s), "{r:?}");
    assert!(top > 15.0, "the racer raced (top {top} m/s)");
    if night {
        assert!(lights, "headlights on through the night race");
    }
    assert_eq!(r.net, -stake);
    assert_eq!(cash(&s), before - stake);
    let roster = s.street().unwrap().roster();
    assert_eq!((roster[0].wins, roster[0].losses), (1, 0));
    assert_eq!(roster[0].entity, racer.0);
    assert_eq!(roster[0].mode, RacerMode::Roam, "back to roaming");

    // Race 2 against another racer: the player is at B before it gets there.
    challenge(&mut s, 1);
    let fin = finish_for(&s);
    let before = cash(&s);
    let stake = street::level(ROSTER_LEVELS[1]).bet_min;
    s.street_accept(fin, stake).unwrap();
    s.run_ticks(street::COUNTDOWN_TICKS + 240);
    let z = state(&s, PLAYER).pos[2];
    let ground = s.sim().world().map().ground_height(fin, Some(z));
    s.place_logged(PLAYER, fin, 0.0, ground).unwrap();
    s.run_ticks(4);
    let r = s.street().unwrap().result().cloned().expect("decided");
    assert_eq!(r.seq, 2);
    assert!(r.won, "{r:?}");
    assert_eq!(r.net, stake);
    assert_eq!(cash(&s), before + stake, "stake back plus theirs");
    let ledger = s.run().unwrap().ledger();
    assert!(ledger
        .iter()
        .any(|e| e.cash_delta == 2 * stake && e.cash_after == before + stake));
}

#[test]
fn challenge_race_result_at_noon() {
    challenge_race_result(12.0);
}

#[test]
fn challenge_race_result_at_night() {
    challenge_race_result(23.0);
}

#[test]
fn racers_roam_at_night_with_headlights() {
    let mut s = city_session_at(1.0);
    let ids = s.street().unwrap().entities();
    let start: Vec<_> = ids.iter().map(|e| state(&s, *e).pos).collect();
    s.run_ticks(240 * 30);
    assert_roamed(&s, &ids, &start, "1 am");
    for e in &ids {
        assert!(s.input(*e).headlights, "racer {e:?} drives without lights");
        assert!(state(&s, *e).speed_mps < 15.0);
    }
}

#[test]
fn a_racer_never_gives_up_after_a_spin_or_a_crash() {
    // No patrols (heat still counts): the shove off the road is a crash, not a police case.
    let mut sc = free_drive("debug_sedan", DamageMode::Arcade, true);
    sc.police_presence = Some(0.0);
    let mut s = Session::new(sc, false).unwrap();
    s.run_ticks(240 * 2);
    let racer = start_race(&mut s, 3);
    let mut spun = false;
    let mut crashed = false;
    let mut hits = 0;
    let r = race_to_result(&mut s, 240, |s, t| {
        hits += s
            .drain_events()
            .iter()
            .filter(|e| match e {
                SimEvent::Contact { a, b, .. } => *a == racer || *b == racer,
                _ => false,
            })
            .count();
        let st = state(s, racer);
        // 6 s in: a forced spin (turned round, standing).
        if !spun && t >= 6.0 {
            spun = true;
            let yaw = yaw_of(st.rot) + std::f32::consts::PI;
            s.place_logged(racer, [st.pos[0], st.pos[1]], yaw, st.pos[2])
                .unwrap();
        }
        // 16 s in: shoved sideways off the road at 15 m/s (into the kerb, a prop or a wall).
        if spun && !crashed && t >= 16.0 {
            crashed = true;
            let yaw = yaw_of(st.rot) + std::f32::consts::FRAC_PI_2;
            s.debug_sim_mut()
                .unwrap()
                .place_on_ground(racer, [st.pos[0], st.pos[1]], yaw, 15.0, Some(st.pos[2]))
                .unwrap();
        }
    });
    eprintln!("after a spin and a crash ({hits} contacts): {r:?}");
    assert!(spun && crashed, "decided before the incidents: {r:?}");
    assert!(!r.won, "the racer recovered and finished: {r:?}");
    assert!(r.reason.contains("crossed the line"), "{r:?}");
}

#[test]
fn a_wanted_racer_keeps_racing_until_caught_or_finished() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    let racer = start_race(&mut s, 4);
    // Wanted level 3 at the green light (the old rule: a careful racer yields from L2, any
    // from L3, and a pinned one pulls over).
    s.sim_mut().police_mut().set_heat(racer, 60.0);
    assert!(s.sim().police().wanted_level(racer).number() >= 3);
    let mut parked_s = 0.0f32;
    let mut worst = 0.0f32;
    let r = race_to_result(&mut s, 240, |s, _| {
        let v = state(s, racer).speed_mps;
        let chased = s.sim().police().wanted_level(racer).number() > 0;
        // Never stands still for long with the police on it, unless captured.
        parked_s = if v.abs() < 0.5 && chased {
            parked_s + 1.0 / 240.0
        } else {
            0.0
        };
        worst = worst.max(parked_s);
    });
    eprintln!("wanted racer: {r:?}, longest stop {worst:.1} s");
    assert!(
        r.reason.contains("crossed the line") || r.reason.contains("police caught"),
        "a racer never gives up: {r:?}"
    );
    assert!(!r.reason.contains("ran from"), "{r:?}");
}

#[test]
fn street_race_replays_without_drift_and_its_crash_window_too() {
    let mut s = city_session_at(23.0);
    s.record_start();
    s.run_ticks(240 * 2);
    let racer = start_race(&mut s, 0);
    let go = s.tick();
    let r = race_to_result(&mut s, 180, |_, _| {});
    assert!(r.reason.contains("crossed the line"), "{r:?}");
    s.run_ticks(240 * 2);
    // A crash-replay window mid-race (the racer racing, the others roaming) re-simulates
    // from the buffered snapshot with the logged inputs. The buffer keeps the last 10 s.
    let mark_tick = s.tick() - 240 * 4;
    let mark = CrashMark {
        seq: 1,
        tick: mark_tick,
        player: PLAYER.0,
        other: Some(racer.0),
        closing_mps: 0.0,
        delta_v_mps: 0.0,
        health_drop: 0.0,
        point: state(&s, PLAYER).pos,
        dir_local: [0.0, 1.0, 0.0],
        side: ImpactSide::Front,
    };
    let (mut cr, info) = s.crash_replay(&mark).unwrap();
    cr.replay_play(1.0).unwrap();
    cr.run_ticks(info.end_tick - cr.tick());
    let st = cr.replay_status().unwrap();
    assert_eq!(
        st.drift_count, 0,
        "crash window drifted at {:?}",
        st.drift_ticks
    );
    assert!(st.verified_ticks > 0);

    let replay = s.record_stop().unwrap();
    let ticks = replay.ticks;
    let per_s = replay.inputs.len() as f32 / (ticks as f32 / 240.0);
    eprintln!(
        "street session: {ticks} ticks, {} inputs ({per_s:.0}/s), {} ops, race from {go}",
        replay.inputs.len(),
        replay.ops.len()
    );
    let bytes = replay.encode();
    eprintln!("replay.bin: {} bytes", bytes.len());
    let mut p = Session::from_replay_bytes(&bytes).unwrap();
    assert!(p.street().is_none(), "no street AI in playback");
    assert_eq!(p.entities().len(), s.entities().len());
    assert_eq!(p.vehicle(racer).unwrap().kind, wp_sim::VehicleKind::AiRacer);
    p.run_ticks(ticks);
    let st = p.replay_status().unwrap();
    assert_eq!(st.drift_count, 0, "replay drifted at {:?}", st.drift_ticks);
    assert_eq!(st.verified_ticks, ticks);
}

#[test]
fn racers_roam_port_vesper_too() {
    let sc = crate::scenario::free_drive_in(
        "debug_sedan",
        DamageMode::Arcade,
        crate::scenario::city_spec("vesper_v2"),
    );
    let mut s = Session::new(sc, false).unwrap();
    let ids = s.street().expect("street racers in Port Vesper").entities();
    assert_eq!(ids.len(), ROSTER_LEVELS.len());
    let start: Vec<_> = ids.iter().map(|e| state(&s, *e).pos).collect();
    s.run_ticks(240 * 40);
    assert_roamed(&s, &ids, &start, "Port Vesper");
    for e in &ids {
        let st = state(&s, *e);
        assert!(st.health > 0.5, "racer {e:?} wrecked while roaming");
        assert!(st.speed_mps < 15.0, "racer {e:?} too fast while roaming");
    }
}

#[test]
fn sixteen_racers_four_per_level_spawn_apart_on_both_cities() {
    assert_eq!(ROSTER_LEVELS.len(), 16);
    for l in &street::LEVELS {
        assert_eq!(ROSTER_LEVELS.iter().filter(|x| **x == l.level).count(), 4);
    }
    assert!(
        street::LEVELS.iter().all(|l| l.level >= 2),
        "no Rookie level"
    );
    for city in ["procedural_12x12", "vesper_v2"] {
        let sc = crate::scenario::free_drive_in(
            "debug_sedan",
            DamageMode::Arcade,
            crate::scenario::city_spec(city),
        );
        let s = Session::new(sc, false).unwrap();
        let ids = s.street().unwrap().entities();
        assert_eq!(ids.len(), 16, "{city}");
        let mut pts = vec![state(&s, PLAYER).pos];
        pts.extend(ids.iter().map(|e| state(&s, *e).pos));
        let mut closest = f32::INFINITY;
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                closest = closest.min(dist2(pts[i], pts[j]));
            }
        }
        eprintln!("{city}: closest pair {closest:.0} m");
        assert!(
            closest >= street::SPAWN_SPACING_M - 0.01,
            "{city}: {closest} m"
        );
    }
}

#[test]
fn racer_cars_are_known_before_the_first_tick() {
    let s = city_session();
    for e in s.street().unwrap().entities() {
        assert!(s.car(e).is_some(), "car of racer {e:?} resolved at tick 0");
    }
}

/// Perf (desktop gate): sim cost per 60 Hz frame (4 ticks) of a city free drive with and
/// without the 16 street racers, both cities, native release, 30 s after a 5 s warm-up; the
/// street AI's own share (`street::perf`), and the cost per racer:
/// `cargo test --release -p wp_wasm --lib street_perf -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore]
fn street_perf() {
    const FRAMES: f64 = 60.0 * 30.0;
    for city in ["procedural_12x12", "vesper_v2"] {
        let mut off_ms = 0.0;
        for on in [false, true] {
            crate::session::set_street_racers_enabled(on);
            let sc = crate::scenario::free_drive_in(
                "lindwurm_4t",
                DamageMode::Arcade,
                crate::scenario::city_spec(city),
            );
            let t0 = std::time::Instant::now();
            let mut s = Session::new(sc, false).unwrap();
            let build = t0.elapsed();
            s.run_ticks(240 * 5);
            street::perf::take();
            let t0 = std::time::Instant::now();
            s.run_ticks(240 * 30);
            let ms = t0.elapsed().as_secs_f64() * 1000.0 / FRAMES;
            let (ai_ns, worst_ns) = street::perf::take();
            let ai_ms = ai_ns as f64 / 1e6 / FRAMES;
            let racers = s.street().map_or(0, |st| st.entities().len());
            eprintln!(
                "{city}: street racers {on}: {ms:.3} ms per 60 Hz frame, street AI {ai_ms:.3} ms (worst call {:.2} ms), \
                 {} vehicles ({racers} street racers), {} traffic agents, session build {build:?}",
                worst_ns as f64 / 1e6,
                s.sim().vehicles().len(),
                s.sim().world().agents().count()
            );
            if on {
                let per = (ms - off_ms) / racers.max(1) as f64;
                eprintln!(
                    "{city}: +{:.3} ms per frame for {racers} racers = {per:.3} ms per racer \
                     (AI {:.4} ms per racer)",
                    ms - off_ms,
                    ai_ms / racers.max(1) as f64
                );
            } else {
                off_ms = ms;
            }
        }
    }
    crate::session::set_street_racers_enabled(true);
}

/// Row 154 ("the game freezes when I start a street race"): a street race starts and runs 30 s
/// in the Port Vesper night session (seed 5) where, in 0.12.49, a roaming racer's next route
/// came back with nothing to drive and `Field::from_route` panicked a few seconds after the
/// start (in the browser the panic stopped the page). Challenge, a finish ~5 km of road away
/// (as a player picks on the map), accept, then 30 s of racing (the racer gets going).
#[test]
fn street_race_starts_and_runs_30_s() {
    let mut sc = crate::scenario::free_drive_in(
        "lindwurm_4t",
        DamageMode::Arcade,
        crate::scenario::city_spec("vesper_v2"),
    );
    sc.time_of_day_h = 23.0;
    sc.seed = 5;
    let mut s = Session::new(sc, false).unwrap();
    s.run_ticks(240);
    let racer = challenge(&mut s, 5);
    let p = state(&s, PLAYER).pos;
    let mut rng = wp_math::Rng::new(105);
    let mut fin = None;
    for _ in 0..40 {
        let a = rng.next_f32() * std::f32::consts::TAU;
        let d = 250.0 + rng.next_f32() * 4500.0;
        let q = [p[0] + a.cos() * d, p[1] + a.sin() * d];
        if let Ok(pr) = s.street_preview(q) {
            if pr.ok {
                fin = Some(q);
                break;
            }
        }
    }
    let fin = fin.expect("a raceable finish");
    let stake = street::level(s.street().unwrap().roster()[5].level).bet_min;
    s.street_accept(fin, stake).unwrap();
    // The test player does not drive: at the green light it steps out of the racer's way (it
    // may be lined up right behind on a one-lane street).
    s.run_ticks(street::COUNTDOWN_TICKS + 1);
    let off = [p[0] + 0.0, p[1] + 400.0];
    let z = s.sim().world().map().ground_height(off, None);
    s.place_logged(PLAYER, off, 0.0, z).unwrap();
    let mut elapsed = 0.0f32;
    let mut top = 0.0f32;
    for _ in 0..(30 * 240) {
        s.set_input(PLAYER, horn(false)).unwrap();
        s.run_ticks(1);
        match s.street().unwrap().race(s.sim()) {
            Some(r) => elapsed = r.elapsed_s,
            None => break,
        }
        top = top.max(state(&s, racer).speed_mps);
    }
    eprintln!("street race: {elapsed:.1} s raced, racer top {top:.1} m/s");
    assert!(
        elapsed > 25.0 || s.street().unwrap().result().is_some(),
        "raced {elapsed} s"
    );
    assert!(top > 10.0, "the racer raced (top {top} m/s)");
}
