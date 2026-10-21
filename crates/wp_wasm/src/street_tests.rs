//! Native tests of the street racers (`crate::street`) inside a live city session.

use wp_contracts::{DamageMode, EntityId, VehicleInput, VehicleState};

use crate::scenario::free_drive;
use crate::session::Session;
use crate::street::{self, RacerMode, ROSTER_LEVELS};

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

fn city_session() -> Session {
    Session::new(free_drive("debug_sedan", DamageMode::Arcade, true), false).unwrap()
}

fn horn(on: bool) -> VehicleInput {
    VehicleInput {
        brake: 1.0,
        horn: on,
        ..Default::default()
    }
}

/// Put the player 9 m behind street racer `k` (same lane and heading), stopped.
fn park_behind(s: &mut Session, k: usize) -> EntityId {
    let racer = s.street().unwrap().entities()[k];
    let st = state(s, racer);
    let yaw = yaw_of(st.rot);
    let (sn, cs) = wp_math::sin_cos(yaw);
    let xy = [st.pos[0] + sn * 9.0, st.pos[1] - cs * 9.0];
    s.debug_sim_mut()
        .unwrap()
        .place_on_ground(PLAYER, xy, yaw, 0.0, Some(st.pos[2]))
        .unwrap();
    racer
}

/// Honk next to racer `k`: the offer opens for it.
fn challenge(s: &mut Session, k: usize) -> EntityId {
    let racer = park_behind(s, k);
    s.set_input(PLAYER, horn(true)).unwrap();
    s.run_ticks(2);
    s.set_input(PLAYER, horn(false)).unwrap();
    s.run_ticks(2);
    racer
}

/// A finish 400–900 m of road away that the planner accepts.
fn finish_for(s: &Session) -> [f32; 2] {
    let p = state(s, PLAYER).pos;
    for lane in &s.sim().world().network().lanes {
        let q = lane.polyline[lane.polyline.len() / 2];
        let d = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
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

#[test]
fn city_session_spawns_the_roster_and_racers_roam() {
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
    let ids = st.entities();
    let start: Vec<_> = ids.iter().map(|e| state(&s, *e).pos).collect();
    let mut violations = Vec::new();
    for _ in 0..40 {
        s.run_ticks(240);
        violations.extend(s.drain_events().into_iter().filter_map(|e| match e {
            wp_contracts::SimEvent::TrafficViolation { vehicle, kind, .. }
                if ids.contains(&vehicle) =>
            {
                Some(kind)
            }
            _ => None,
        }));
    }
    eprintln!("roaming violations: {violations:?}");
    assert!(
        violations.is_empty(),
        "roaming racers should not draw the police: {violations:?}"
    );
    let moved = ids
        .iter()
        .zip(&start)
        .filter(|(e, p0)| {
            let p = state(&s, **e).pos;
            ((p[0] - p0[0]).powi(2) + (p[1] - p0[1]).powi(2)).sqrt() > 50.0
        })
        .count();
    assert!(
        moved >= ids.len() - 1,
        "only {moved} of {} racers roamed",
        ids.len()
    );
    // Cruising, not racing: nobody far above city speeds.
    for e in &ids {
        assert!(state(&s, *e).speed_mps < 22.0, "racer {e:?} too fast");
    }
    let packed = s.street().unwrap().packed(s.sim());
    assert_eq!(packed.len(), 6 * ids.len());
}

#[test]
fn street_racers_can_be_turned_off() {
    crate::session::set_street_racers_enabled(false);
    let s = city_session();
    crate::session::set_street_racers_enabled(true);
    assert!(s.street().is_none());
    let flat = Session::new(free_drive("debug_sedan", DamageMode::Arcade, false), false).unwrap();
    assert!(
        flat.street().is_none(),
        "no street racers on the flat world"
    );
}

#[test]
fn honk_opens_a_challenge_and_bad_bets_are_refused() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    let racer = challenge(&mut s, 2);
    let offer = s.street().unwrap().offer(s.sim(), s.run()).expect("offer");
    assert_eq!(offer.entity, racer.0);
    assert_eq!(offer.level, ROSTER_LEVELS[2]);
    assert!(offer.affordable);
    // The racer pulls over and waits.
    s.run_ticks(240 * 3);
    assert!(state(&s, racer).speed_mps < 1.0);
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

#[test]
fn lost_race_keeps_the_stake_and_won_race_pays_even_money() {
    let mut s = city_session();
    s.run_ticks(240 * 2);
    // Race 1: the player stays on the brakes; the racer wins.
    let racer = challenge(&mut s, 0);
    let fin = finish_for(&s);
    let stake = street::level(ROSTER_LEVELS[0]).bet_min;
    let before = cash(&s);
    s.street_accept(fin, stake).unwrap();
    assert_eq!(cash(&s), before - stake, "stake paid up front");
    assert!(s.held(PLAYER), "countdown holds the player");
    s.run_ticks(street::COUNTDOWN_TICKS + 2);
    assert!(!s.held(PLAYER));
    let mut result = None;
    for _ in 0..240 * 180 {
        s.set_input(PLAYER, horn(false)).unwrap();
        s.run_ticks(1);
        if let Some(r) = s.street().unwrap().result() {
            result = Some(r.clone());
            break;
        }
    }
    let r = result.expect("race decided within 3 minutes");
    assert!(!r.won, "{r:?}");
    assert!(r.reason.contains("crossed the line"), "{r:?}");
    // 400–900 m at a plausible street pace (not teleported, not crawling).
    assert!((15.0..=120.0).contains(&r.time_s), "{r:?}");
    eprintln!("race 1: {r:?}");
    assert_eq!(r.net, -stake);
    assert_eq!(cash(&s), before - stake);
    let roster = s.street().unwrap().roster();
    assert_eq!((roster[0].wins, roster[0].losses), (1, 0));
    assert_eq!(roster[0].entity, racer.0);

    // Race 2 against another racer: the player is at B before it gets there.
    let racer = challenge(&mut s, 1);
    let fin = finish_for(&s);
    let before = cash(&s);
    let stake = street::level(ROSTER_LEVELS[1]).bet_min;
    s.street_accept(fin, stake).unwrap();
    s.run_ticks(street::COUNTDOWN_TICKS + 240);
    let z = state(&s, PLAYER).pos[2];
    let sim = s.debug_sim_mut().unwrap();
    let ground = sim.world().map().ground_height(fin, Some(z));
    sim.place_on_ground(PLAYER, fin, 0.0, 0.0, Some(ground))
        .unwrap();
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
    let _ = racer;
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
    let moved = ids
        .iter()
        .zip(&start)
        .filter(|(e, p0)| {
            let p = state(&s, **e).pos;
            ((p[0] - p0[0]).powi(2) + (p[1] - p0[1]).powi(2)).sqrt() > 50.0
        })
        .count();
    assert!(
        moved >= ids.len() - 1,
        "only {moved} of {} racers roamed",
        ids.len()
    );
    for e in &ids {
        let st = state(&s, *e);
        assert!(st.health > 0.5, "racer {e:?} wrecked while roaming");
    }
}

#[test]
fn racer_cars_are_known_before_the_first_tick() {
    let s = city_session();
    for e in s.street().unwrap().entities() {
        assert!(s.car(e).is_some(), "car of racer {e:?} resolved at tick 0");
    }
}

/// Perf: per-tick cost of the street racers in Port Vesper (release:
/// `cargo test --release -p wp_wasm --lib street_perf -- --ignored --nocapture`).
#[test]
#[ignore]
fn street_perf() {
    for on in [false, true] {
        crate::session::set_street_racers_enabled(on);
        let sc = crate::scenario::free_drive_in(
            "lindwurm_4t",
            DamageMode::Arcade,
            crate::scenario::city_spec("vesper_v2"),
        );
        let mut s = Session::new(sc, false).unwrap();
        s.run_ticks(240 * 5);
        let t0 = std::time::Instant::now();
        s.run_ticks(240 * 20);
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / (60.0 * 20.0);
        eprintln!("street racers {on}: {ms:.3} ms per 60 Hz frame (4 ticks)");
    }
    crate::session::set_street_racers_enabled(true);
}
