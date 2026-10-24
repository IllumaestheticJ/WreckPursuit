//! Native tests of the street racers (`crate::street`) inside a live city session: roster and
//! calm roaming, challenge → race → result at noon and at night, the never-give-up rule in a
//! race (spin, crash, police), replays and crash replays of street sessions, and no racers
//! outside live free drive.

use wp_contracts::{DamageMode, EntityId, SimEvent, VehicleInput, VehicleState};

use crate::crash_replay::{CrashMark, ImpactSide};
use crate::scenario::free_drive;
use crate::session::Session;
use crate::street::{self, RaceResult, RacerMode, Shift, ROSTER};

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

/// Free drive on the procedural grid at `hour` (12 = noon, 23 = the night run's night).
fn city_session_at(hour: f32) -> Session {
    let mut sc = free_drive("debug_sedan", DamageMode::Arcade, true);
    sc.time_of_day_h = hour;
    Session::new(sc, false).unwrap()
}

/// Racers on shift now (not in the lockup).
fn on_duty(s: &Session) -> Vec<EntityId> {
    s.street()
        .unwrap()
        .roster()
        .into_iter()
        .filter(|r| r.mode != RacerMode::OffDuty)
        .map(|r| EntityId(r.entity))
        .collect()
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

/// Honk right behind racer `k`: the offer opens for it.
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
        if let Ok(pr) = s.street_preview([q[0], q[1]], &[]) {
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
    s.street_accept(fin, &[], street::level(lv).bet_min)
        .unwrap();
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
    assert_eq!(roster.len(), ROSTER.len());
    for (r, (lv, shift)) in roster.iter().zip(ROSTER) {
        assert_eq!(r.level, lv);
        assert_eq!(r.shift, shift);
        // Noon: only the all-day racers are out.
        assert_eq!(r.mode == RacerMode::OffDuty, shift != Shift::AllDay);
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
    let ids = on_duty(&s);
    assert_eq!(ids.len(), 8, "8 racers by day (row 152)");
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

/// A honk only challenges a racer the player is right behind: alongside, in front or facing
/// the other way does nothing (owner request 2026-10-23).
#[test]
fn honk_challenges_only_from_right_behind() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    let racer = s.street().unwrap().entities()[2];
    let st = state(&s, racer);
    let yaw = yaw_of(st.rot);
    let (sn, cs) = wp_math::sin_cos(yaw);
    // Racer forward = (-sin, cos); right = (cos, sin).
    for (what, along, side, turn) in [
        ("alongside", 0.0, 5.0, 0.0),
        ("in front", 12.0, 0.0, 0.0),
        (
            "behind, facing the other way",
            -9.0,
            0.0,
            std::f32::consts::PI,
        ),
        ("too far behind", -40.0, 0.0, 0.0),
    ] {
        let xy = [
            st.pos[0] - sn * along + cs * side,
            st.pos[1] + cs * along + sn * side,
        ];
        s.place_logged(PLAYER, xy, yaw + turn, st.pos[2]).unwrap();
        assert!(
            s.street().unwrap().challengeable(s.sim(), PLAYER).is_none(),
            "no hint: {what}"
        );
        s.set_input(PLAYER, horn(true)).unwrap();
        s.run_ticks(2);
        s.set_input(PLAYER, horn(false)).unwrap();
        s.run_ticks(2);
        assert!(
            s.street().unwrap().offer(s.sim(), s.run()).is_none(),
            "a honk opens no challenge: {what}"
        );
    }
    challenge(&mut s, 2);
}

#[test]
fn honk_opens_a_challenge_and_bad_bets_are_refused() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    let racer = challenge(&mut s, 2);
    let offer = s.street().unwrap().offer(s.sim(), s.run()).expect("offer");
    assert_eq!(offer.level, ROSTER[2].0);
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
        s.street_accept(fin, &[], offer.bet_min - 1).is_err(),
        "under the minimum"
    );
    assert!(
        s.street_accept(fin, &[], before + 1).is_err(),
        "more than the cash"
    );
    // 30 m ahead on the player's lane (a pick beside or behind the car may route round the
    // block and back: it covers the whole road).
    let p = state(&s, PLAYER).pos;
    let (sn, cs) = wp_math::sin_cos(yaw_of(state(&s, PLAYER).rot));
    assert!(
        s.street_accept([p[0] - sn * 30.0, p[1] + cs * 30.0], &[], offer.bet_min)
            .is_err(),
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
    let stake = street::level(ROSTER[0].0).bet_min;
    let before = cash(&s);
    s.street_accept(fin, &[], stake).unwrap();
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
    let rep_before = s.run().unwrap().account().rep;
    let stake = street::level(ROSTER[1].0).bet_min;
    s.street_accept(fin, &[], stake).unwrap();
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
    // The win climbs the career's rep ladder by the level's win_rep (a loss costs none).
    let win_rep = street::level(ROSTER[1].0).win_rep;
    assert_eq!(r.rep, win_rep);
    assert_eq!(s.run().unwrap().account().rep, rep_before + win_rep);
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
    assert!(s.street().is_some(), "street racers in Port Vesper");
    let ids = on_duty(&s);
    assert_eq!(ids.len(), 8, "noon: the 8 all-day racers");
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
fn racers_by_shift_spawn_apart_on_both_cities() {
    // Row 152: 8 by day (two per level), 13 at night, 4 of them on Loop 9.
    let day: Vec<_> = ROSTER.iter().filter(|r| r.1 == Shift::AllDay).collect();
    assert_eq!(day.len(), 8);
    for l in &street::LEVELS {
        assert_eq!(day.iter().filter(|r| r.0 == l.level).count(), 2);
    }
    assert_eq!(ROSTER.len(), 13);
    assert_eq!(ROSTER.iter().filter(|r| r.1 == Shift::Loop).count(), 4);
    assert!(
        street::LEVELS.iter().all(|l| l.level >= 2),
        "no Rookie level"
    );
    for (city, hour) in [
        ("procedural_12x12", 12.0),
        ("vesper_v2", 12.0),
        ("vesper_v2", 23.0),
    ] {
        let mut sc = crate::scenario::free_drive_in(
            "debug_sedan",
            DamageMode::Arcade,
            crate::scenario::city_spec(city),
        );
        sc.time_of_day_h = hour;
        let s = Session::new(sc, false).unwrap();
        assert_eq!(s.street().unwrap().entities().len(), 13, "{city}");
        let ids = on_duty(&s);
        assert_eq!(ids.len(), if hour > 20.0 { 13 } else { 8 }, "{city} {hour}");
        let mut pts = vec![state(&s, PLAYER).pos];
        pts.extend(ids.iter().map(|e| state(&s, *e).pos));
        let mut closest = f32::INFINITY;
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                closest = closest.min(dist2(pts[i], pts[j]));
            }
        }
        eprintln!("{city}: closest pair {closest:.0} m");
        if hour < 20.0 {
            // The night shift waits in the lockup: parked on drawn ground, at rest, undamaged,
            // asleep (no vehicle physics).
            let mut s = s;
            s.run_ticks(240 * 3);
            for r in s.street().unwrap().roster() {
                if r.mode != RacerMode::OffDuty {
                    continue;
                }
                let st = state(&s, EntityId(r.entity));
                assert!(
                    st.speed_mps.abs() < 0.5 && st.health > 0.99 && st.pos[2] > -50.0,
                    "{city}: {} in the lockup moves or fell: {st:?}",
                    r.name
                );
                assert_eq!(
                    s.sim().is_dormant(EntityId(r.entity)),
                    Some(true),
                    "{city}: {} sleeps in the lockup (SR-3)",
                    r.name
                );
            }
        }
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
/// without the street racers (13: noon has 8 out, 5 in the lockup), both cities, native release, 30 s after a 5 s warm-up; the
/// street AI's own share (`street::perf`), and the cost per racer:
/// `cargo test --release -p wp_wasm --lib street_perf -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore]
fn street_perf() {
    const FRAMES: f64 = 60.0 * 30.0;
    for (city, hour) in [
        ("procedural_12x12", 12.0),
        ("procedural_12x12", 23.0),
        ("vesper_v2", 12.0),
        ("vesper_v2", 23.0),
    ] {
        if std::env::var("PERF_ONLY").is_ok_and(|o| o != format!("{city}:{hour}")) {
            continue;
        }
        let mut off_ms = 0.0;
        for on in [false, true] {
            crate::session::set_street_racers_enabled(on);
            let mut sc = crate::scenario::free_drive_in(
                "lindwurm_4t",
                DamageMode::Arcade,
                crate::scenario::city_spec(city),
            );
            sc.time_of_day_h = hour;
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
            let lights = s.street().map_or(0, |st| {
                st.entities()
                    .iter()
                    .filter(|e| s.sim().is_light(**e) == Some(true))
                    .count()
            });
            let dorm = s.street().map_or(0, |st| {
                st.entities()
                    .iter()
                    .filter(|e| s.sim().is_dormant(**e) == Some(true))
                    .count()
            });
            eprintln!("  light {lights}, dormant {dorm}");
            eprintln!(
                "{city} {hour}h: street racers {on}: {ms:.3} ms per 60 Hz frame, street AI {ai_ms:.3} ms (worst call {:.2} ms), \
                 {} vehicles ({racers} street racers), {} traffic agents, session build {build:?}",
                worst_ns as f64 / 1e6,
                s.sim().vehicles().len(),
                s.sim().world().agents().count()
            );
            if on {
                let per = (ms - off_ms) / racers.max(1) as f64;
                eprintln!(
                    "{city} {hour}h: +{:.3} ms per frame for {racers} racers = {per:.3} ms per racer \
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

/// Row 152: the night shift (5 racers) comes out of the lockup when night falls, out of the
/// player's sight and onto lanes in the city; at dawn it goes back. 8 out by day, 13 at night.
#[test]
fn night_shift_comes_out_at_dusk_and_goes_home_at_dawn() {
    // 19:57 at a minute per second: night (20:00) after 3 s.
    let mut sc = free_drive("debug_sedan", DamageMode::Arcade, true);
    sc.time_of_day_h = 19.95;
    sc.time_ratio = 60.0;
    let mut s = Session::new(sc, false).unwrap();
    assert_eq!(on_duty(&s).len(), 8, "dusk: 8 out");
    let night: Vec<EntityId> = s
        .street()
        .unwrap()
        .roster()
        .into_iter()
        .filter(|r| r.shift != Shift::AllDay)
        .map(|r| EntityId(r.entity))
        .collect();
    assert_eq!(night.len(), 5);
    let locked: Vec<_> = night.iter().map(|e| state(&s, *e).pos).collect();
    let packed = s.street().unwrap().packed(s.sim());
    assert_eq!(packed.len(), 6 * 8, "the lockup is not on the map");
    s.run_ticks(240 * 8);
    assert!(street::is_night(s.sim()));
    assert_eq!(on_duty(&s).len(), 13, "night: 13 out");
    let p = state(&s, PLAYER).pos;
    for (e, was) in night.iter().zip(&locked) {
        let now = state(&s, *e).pos;
        assert!(dist2(now, *was) > 300.0, "{e:?} left the lockup");
        assert!(
            dist2(now, p) >= 140.0,
            "{e:?} came out in sight of the player"
        );
        let lane = s.sim().world().map().locate(now, None, 3.0, true, true);
        assert!(lane.is_some(), "{e:?} is on the road ({now:?})");
    }
    assert_eq!(s.street().unwrap().packed(s.sim()).len(), 6 * 13);

    // 04:57: back to the lockup after 05:00 (each once out of the player's sight).
    let mut sc = free_drive("debug_sedan", DamageMode::Arcade, true);
    sc.time_of_day_h = 4.95;
    sc.time_ratio = 60.0;
    let mut s = Session::new(sc, false).unwrap();
    assert_eq!(on_duty(&s).len(), 13, "before dawn: 13 out");
    s.run_ticks(240 * 30);
    assert!(!street::is_night(s.sim()));
    assert_eq!(on_duty(&s).len(), 8, "after dawn: 8 out");
}

/// Row 152: in Port Vesper at night four racers lap Loop 9 at highway pace without drawing
/// the police; the other nine roam the city calmly.
#[test]
fn four_racers_lap_loop9_at_night_in_port_vesper() {
    let mut sc = crate::scenario::free_drive_in(
        "debug_sedan",
        DamageMode::Arcade,
        crate::scenario::city_spec("vesper_v2"),
    );
    sc.time_of_day_h = 23.0;
    let mut s = Session::new(sc, false).unwrap();
    assert_eq!(on_duty(&s).len(), 13);
    let roster = s.street().unwrap().roster();
    let lap: Vec<EntityId> = roster
        .iter()
        .filter(|r| r.shift == Shift::Loop)
        .map(|r| EntityId(r.entity))
        .collect();
    let city: Vec<EntityId> = roster
        .iter()
        .filter(|r| r.shift != Shift::Loop)
        .map(|r| EntityId(r.entity))
        .collect();
    assert_eq!(lap.len(), 4);
    let highway = |s: &Session, e: EntityId| {
        let st = state(s, e);
        let map = s.sim().world().map();
        map.locate(st.pos, None, 4.0, true, false).is_some_and(|l| {
            matches!(map.seg(l.seg).kind, wp_world::map::SegKind::Lane { lane }
                if s.sim().world().network().lanes[lane as usize].kind
                    == wp_world::network::LaneKind::Highway)
        })
    };
    for e in &lap {
        assert!(highway(&s, *e), "{e:?} starts on Loop 9");
    }
    let mut violations = Vec::new();
    let (mut sum, mut n, mut on_hw, mut samples) = (0.0f32, 0u32, 0u32, 0u32);
    let mut city_top = 0.0f32;
    for _ in 0..60 {
        s.run_ticks(240);
        violations.extend(s.drain_events().into_iter().filter_map(|e| match e {
            SimEvent::TrafficViolation { vehicle, kind, .. } if lap.contains(&vehicle) => {
                Some(kind)
            }
            _ => None,
        }));
        for e in &lap {
            sum += state(&s, *e).speed_mps;
            n += 1;
            samples += 1;
            on_hw += highway(&s, *e) as u32;
        }
        for e in &city {
            city_top = city_top.max(state(&s, *e).speed_mps);
        }
    }
    let mean = sum / n as f32;
    eprintln!(
        "loop racers: mean {mean:.1} m/s, on the highway {on_hw}/{samples}, violations \
         {violations:?}; city racers top {city_top:.1} m/s"
    );
    assert!(mean > 18.0, "loop racers lap at highway pace: {mean} m/s");
    assert!(
        4 * on_hw >= 3 * samples,
        "and stay on the highway: {on_hw}/{samples}"
    );
    assert!(violations.is_empty(), "{violations:?}");
    assert!(city_top < 15.0, "city racers keep to city pace: {city_top}");
}

/// Row 150: no challenge while the player is wanted (a honk does nothing, an open offer lapses,
/// accepting is refused); accepting resets both cars side by side at the start, at rest, facing
/// along the route, as logged moves (the recording replays them).
#[test]
fn no_challenge_while_wanted_and_both_cars_start_side_by_side() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    s.debug_sim_mut()
        .unwrap()
        .police_mut()
        .set_heat(PLAYER, 40.0);
    s.run_ticks(2);
    assert!(s.sim().police().is_wanted_or_pursued(PLAYER));
    park_behind(&mut s, 0);
    assert!(
        s.street().unwrap().challengeable(s.sim(), PLAYER).is_none(),
        "no hint while wanted"
    );
    assert!(s.street().unwrap().in_reach(s.sim(), PLAYER).is_some());
    s.set_input(PLAYER, horn(true)).unwrap();
    s.run_ticks(2);
    s.set_input(PLAYER, horn(false)).unwrap();
    s.run_ticks(2);
    assert!(
        s.street().unwrap().offer(s.sim(), s.run()).is_none(),
        "a honk opens no challenge while wanted"
    );
    // Clean again: the challenge opens; wanted again: it lapses.
    s.debug_sim_mut()
        .unwrap()
        .police_mut()
        .set_heat(PLAYER, 0.0);
    s.run_ticks(2);
    assert!(!s.sim().police().is_wanted_or_pursued(PLAYER));
    let racer = challenge(&mut s, 0);
    s.debug_sim_mut()
        .unwrap()
        .police_mut()
        .set_heat(PLAYER, 40.0);
    s.run_ticks(2);
    assert!(
        s.street().unwrap().offer(s.sim(), s.run()).is_none(),
        "the offer lapsed"
    );
    s.debug_sim_mut()
        .unwrap()
        .police_mut()
        .set_heat(PLAYER, 0.0);
    s.run_ticks(240 * 21);
    // Rolling a little and turned off the lane's heading before the accept.
    challenge(&mut s, 1);
    let fin = finish_for(&s);
    let before = state(&s, PLAYER);
    let wobble = [before.pos[0] + 0.6, before.pos[1] + 0.4];
    s.place_logged(PLAYER, wobble, yaw_of(before.rot) + 0.35, before.pos[2])
        .unwrap();
    let racer1 = s.street().unwrap().entities()[1];
    let stake = street::level(ROSTER[1].0).bet_min;
    s.street_accept(fin, &[], stake).unwrap();
    s.run_ticks(1);
    let (a, b) = (state(&s, PLAYER), state(&s, racer1));
    let view = s.street().unwrap().race(s.sim()).unwrap();
    let route_dir = {
        let p = &view.polyline;
        let d = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
        d[1].atan2(d[0])
    };
    let heading = |q: [f32; 4]| {
        let yaw = yaw_of(q);
        // yaw 0 = +Y.
        (yaw.cos()).atan2(-yaw.sin())
    };
    let ang = |x: f32, y: f32| {
        ((x - y + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI)
            .abs()
    };
    eprintln!(
        "start: player {:?} {:.2} m/s, racer {:?} {:.2} m/s, gap {:.2} m, route dir {route_dir:.2}, \
         player heading {:.2}, racer heading {:.2}",
        a.pos,
        a.speed_mps,
        b.pos,
        b.speed_mps,
        dist2(a.pos, b.pos),
        heading(a.rot),
        heading(b.rot)
    );
    assert!(
        a.speed_mps.abs() < 0.3 && b.speed_mps.abs() < 0.3,
        "both at rest"
    );
    assert!((2.5..=7.5).contains(&dist2(a.pos, b.pos)), "side by side");
    assert!(
        ang(heading(a.rot), route_dir) < 0.2,
        "player faces the route"
    );
    assert!(
        ang(heading(b.rot), route_dir) < 0.2,
        "racer faces the route"
    );
    // The forward offset between them is small (beside, not behind).
    let f = [route_dir.cos(), route_dir.sin()];
    let along = (b.pos[0] - a.pos[0]) * f[0] + (b.pos[1] - a.pos[1]) * f[1];
    assert!(
        along.abs() < 1.5,
        "beside, not ahead or behind ({along:.2} m)"
    );
    let _ = racer;
}

/// Row 151: a route through waypoints runs through them in order; the gates sit on the route
/// and B counts only after the last gate.
#[test]
fn waypoints_shape_the_route_and_gates_count_in_order() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    challenge(&mut s, 0);
    // A finish 400–2500 m of road away and a waypoint well off the direct route to it: a lane point
    // 120–400 m from the route's middle that makes it at least 100 m longer. (Not every lane
    // of the procedural grid reaches every other, so some pairs have no route.)
    let p = state(&s, PLAYER).pos;
    let lanes: Vec<[f32; 3]> = s
        .sim()
        .world()
        .network()
        .lanes
        .iter()
        .map(|l| l.polyline[l.polyline.len() / 2])
        .collect();
    let mut found = None;
    'search: for f in &lanes {
        if !(300.0..=900.0).contains(&dist2(*f, p)) {
            continue;
        }
        let Ok(direct) = s.street_preview([f[0], f[1]], &[]) else {
            continue;
        };
        if !(direct.ok && (400.0..=2500.0).contains(&direct.length_m)) {
            continue;
        }
        let mid = direct.polyline[direct.polyline.len() / 2];
        for q in &lanes {
            let off = ((q[0] - mid[0]).powi(2) + (q[1] - mid[1]).powi(2)).sqrt();
            if !(120.0..=400.0).contains(&off) || dist2(*q, p) < 100.0 {
                continue;
            }
            let r = s.street_preview(direct.finish, &[[q[0], q[1]]]);
            if let Ok(pr) = r {
                if pr.ok && pr.length_m > direct.length_m + 100.0 {
                    found = Some((direct, [q[0], q[1]], pr));
                    break 'search;
                }
            }
        }
    }
    let (direct, w, pr) = found.expect("a finish and a waypoint off the direct route");
    let fin = direct.finish;
    eprintln!(
        "direct {:.0} m, via {:?}: {:.0} m, gate {:?}",
        direct.length_m, w, pr.length_m, pr.gates
    );
    assert_eq!(pr.gates.len(), 1);
    let g = pr.gates[0];
    assert!(
        ((g[0] - w[0]).powi(2) + (g[1] - w[1]).powi(2)).sqrt() < 30.0,
        "gate at the waypoint"
    );
    // Too many waypoints are refused.
    assert!(s
        .street_preview(fin, &[[w[0], w[1]]; street::MAX_WAYPOINTS + 1])
        .is_err());
    // Race with the waypoint: teleporting to B before the gate does not win.
    let stake = street::level(ROSTER[0].0).bet_min;
    s.street_accept(fin, &[w], stake).unwrap();
    s.run_ticks(street::COUNTDOWN_TICKS + 240);
    let view = s.street().unwrap().race(s.sim()).unwrap();
    assert_eq!(view.gates.len(), 1);
    assert_eq!(view.next_gate, 0);
    let course = s.street().unwrap().course().to_vec();
    assert_eq!(course[0], 2.0, "course: the gate, then B");
    let z = state(&s, PLAYER).pos[2];
    let ground = s.sim().world().map().ground_height(fin, Some(z));
    s.place_logged(PLAYER, fin, 0.0, ground).unwrap();
    s.run_ticks(8);
    assert!(
        s.street().unwrap().result().is_none(),
        "B before the gate does not count"
    );
    // Through the gate, then B: the player wins.
    let gz = s.sim().world().map().ground_height(g, Some(z));
    s.place_logged(PLAYER, g, 0.0, gz).unwrap();
    s.run_ticks(4);
    assert_eq!(s.street().unwrap().race(s.sim()).unwrap().next_gate, 1);
    s.place_logged(PLAYER, fin, 0.0, ground).unwrap();
    s.run_ticks(4);
    let r = s.street().unwrap().result().cloned().expect("decided");
    assert!(r.won, "{r:?}");
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
        if let Ok(pr) = s.street_preview(q, &[]) {
            if pr.ok {
                fin = Some(q);
                break;
            }
        }
    }
    let fin = fin.expect("a raceable finish");
    let stake = street::level(s.street().unwrap().roster()[5].level).bet_min;
    s.street_accept(fin, &[], stake).unwrap();
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

/// SR-3: roaming racers far from the player drive the sim's light model and stay on the roads,
/// moving; the racer the player is racing never goes light, however far away it gets.
#[test]
fn far_racers_drive_light_and_the_race_opponent_never_does() {
    let mut sc = crate::scenario::free_drive_in(
        "debug_sedan",
        DamageMode::Arcade,
        crate::scenario::city_spec("vesper_v2"),
    );
    sc.time_of_day_h = 23.0;
    let mut s = Session::new(sc, false).unwrap();
    let ids = on_duty(&s);
    let start: Vec<_> = ids.iter().map(|e| state(&s, *e).pos).collect();
    let (mut light_seen, mut off_road) = (0usize, Vec::new());
    for _ in 0..40 {
        s.run_ticks(240);
        let p = state(&s, PLAYER).pos;
        for e in &ids {
            if s.sim().is_light(*e) != Some(true) {
                continue;
            }
            light_seen += 1;
            let st = state(&s, *e);
            assert!(
                dist2(st.pos, p) > wp_sim::LIGHT_OFF_M,
                "light only far away"
            );
            if s.sim()
                .world()
                .map()
                .locate(st.pos, None, 4.0, true, true)
                .is_none()
            {
                off_road.push((*e, st.pos));
            }
        }
    }
    let moved = ids
        .iter()
        .zip(&start)
        .filter(|(e, p0)| dist2(state(&s, **e).pos, **p0) > 200.0)
        .count();
    eprintln!(
        "light samples {light_seen}, off road {off_road:?}, moved {moved}/{}",
        ids.len()
    );
    assert!(
        light_seen > 40,
        "far racers drive light ({light_seen} samples)"
    );
    assert!(
        off_road.len() * 20 <= light_seen,
        "light racers stay on the roads: {off_road:?}"
    );
    assert!(
        moved * 4 >= ids.len() * 3,
        "and keep moving ({moved}/{})",
        ids.len()
    );

    // A race: the opponent stays on full physics with the player 600 m away.
    let racer = start_race(&mut s, 0);
    let p = state(&s, PLAYER).pos;
    let off = [p[0] + 600.0, p[1]];
    let z = s.sim().world().map().ground_height(off, None);
    s.place_logged(PLAYER, off, 0.0, z).unwrap();
    let mut light = false;
    for _ in 0..(240 * 8) {
        s.run_ticks(1);
        light |= s.sim().is_light(racer) == Some(true);
        if s.street().unwrap().race(s.sim()).is_none() {
            break;
        }
    }
    assert!(!light, "the race opponent never drives light");
}

/// main's career time progression (PR #45, `wp_world::progression`) composes with the street
/// racers: every moment of a night shift's budget is `DayPhase::Night`, which is what turns the
/// night-shift street racers on ([`street::is_night`]), and every moment of a day shift is
/// `DayPhase::Day`. The 18→20 / 04→08 transitions are the only stretches outside the windows.
#[test]
fn career_time_shifts_line_up_with_the_street_racers_day_and_night() {
    use wp_contracts::DayPhase;
    use wp_world::{Shift as CareerShift, TimeProgression, TimeRules};
    for (shift, want) in [
        (CareerShift::Day, DayPhase::Day),
        (CareerShift::Night, DayPhase::Night),
    ] {
        let mut t = TimeProgression::starting_at(TimeRules::default(), 1, shift);
        let mut events = Vec::new();
        let mut samples = 0;
        while t.shift() == shift && !t.is_transitioning() {
            assert_eq!(
                t.time_of_day().phase(),
                want,
                "{shift:?} at {:.2} h",
                t.time_of_day().hours()
            );
            t.step(1.0, &mut events);
            samples += 1;
        }
        assert!(samples > 100, "{shift:?}: {samples} samples");
    }
}

/// Owner, 2026-10-23: "in betting mode for free roam it should pause both cars, and clear the
/// area out for race start". While the offer is open the player is held at a halt beside the
/// waiting racer (a honk calls it off); accepting clears civilian cars around the start.
#[test]
fn betting_holds_both_cars_and_the_start_is_cleared() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    let racer = challenge(&mut s, 2);
    assert!(s.held(PLAYER), "betting holds the player");
    // The player's own throttle is overridden while the map is up.
    s.set_input(
        PLAYER,
        VehicleInput {
            throttle: 1.0,
            headlights: true,
            ..Default::default()
        },
    )
    .unwrap();
    s.run_ticks(240 * 3);
    assert!(state(&s, PLAYER).speed_mps.abs() < 0.5, "player held");
    assert!(state(&s, racer).speed_mps.abs() < 1.0, "racer waits");
    // A honk calls the offer off and frees the player.
    s.set_input(PLAYER, horn(true)).unwrap();
    s.run_ticks(2);
    s.set_input(PLAYER, horn(false)).unwrap();
    s.run_ticks(2);
    assert!(s.street().unwrap().offer(s.sim(), s.run()).is_none());
    assert!(!s.held(PLAYER));
    // Accepting clears the start of traffic before both cars go onto it.
    let mut s = city_session();
    s.run_ticks(240 * 2);
    challenge(&mut s, 2);
    let fin = finish_for(&s);
    let lv = s.street().unwrap().roster()[2].level;
    s.street_accept(fin, &[], street::level(lv).bet_min)
        .unwrap();
    let p = state(&s, PLAYER).pos;
    let near = s
        .sim()
        .world()
        .agents()
        .filter(|a| dist2(a.pos, p) < street::START_CLEAR_M - 25.0)
        .count();
    assert_eq!(near, 0, "no civilian car near the start");
    assert!(s.held(PLAYER), "countdown holds the player");
}
