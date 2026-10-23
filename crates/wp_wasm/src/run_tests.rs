//! Native tests of the end-of-run flow (`crate::run` inside a live session).

use wp_contracts::{DamageMode, EntityId, VehicleInput, VehicleState};

use wp_contracts::WantedLevel;
use wp_economy::CarStatus;
use wp_police::unit::Assignment;

use simcli::replay::OpKind;

use crate::run::RunOutcome;
use crate::scenario::{night_run_in, procedural_city};
use crate::session::Session;

const PLAYER: EntityId = EntityId(1);

/// Half pedal: holds the stopped car. Above 0.5, held at a standstill, the gearbox
/// auto-reverses (wp_vehicle drivetrain): on main's grid the "parked" racer backed away down a
/// diagonal avenue at 17 m/s and was never caught (see `career_tests::brake_until_settled`).
fn brake() -> VehicleInput {
    VehicleInput {
        brake: 0.5,
        ..Default::default()
    }
}

fn state(s: &Session, id: EntityId) -> VehicleState {
    s.sim()
        .vehicle_states()
        .into_iter()
        .find(|(e, _)| *e == id)
        .map(|(_, st)| st)
        .unwrap()
}

/// A stationary racer parked `ahead_m` in front of the first police car, at `heat`.
fn parked_near_police(ahead_m: f32, heat: f32) -> Session {
    let mut sc = night_run_in("debug_sedan", DamageMode::Arcade, procedural_city());
    sc.entities[0].speed_mps = 0.0;
    let mut s = Session::new(sc, false).unwrap();
    for _ in 0..240 * 5 {
        s.run_ticks(1);
        if s.debug_park_near_police(PLAYER, ahead_m).is_ok() {
            s.debug_sim_mut()
                .unwrap()
                .police_mut()
                .set_heat(PLAYER, heat);
            return s;
        }
    }
    panic!("no police car spawned");
}

fn speed(s: &Session, id: EntityId) -> f32 {
    let v = state(s, id).lin_vel;
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// Brake in place until the run settles something (at most `max_s` of sim time).
fn wait_for_report(s: &mut Session, max_s: u64) -> crate::run::RunReport {
    for _ in 0..240 * max_s {
        s.set_input(PLAYER, brake()).unwrap();
        s.run_ticks(1);
        if let Some(r) = s.run().and_then(|r| r.report()) {
            return r.clone();
        }
    }
    panic!("nothing settled in {max_s} s");
}

#[test]
fn capture_bills_holds_the_car_and_repair_continue_restores_it() {
    let mut s = parked_near_police(14.0, 30.0);
    let cash0 = s.run().unwrap().account().cash;
    let r = wait_for_report(&mut s, 60);
    assert_eq!(r.outcome, RunOutcome::Captured, "{r:?}");
    assert!(r.run_over);
    // Started at L2 and held still: arrested where it stands.
    assert!(r.level >= WantedLevel::L2, "{:?}", r.level);
    assert_eq!(r.mode, DamageMode::Arcade);
    // Fine (L2, Arcade-scaled) and impound; repair quoted, not paid.
    assert!(r.fines > 0, "{r:?}");
    assert_eq!(r.car_status, CarStatus::Impounded);
    let fee = r.impound_fee.expect("impound fee");
    assert!(fee > 0 && r.impound_fee_per_day.unwrap() > 0);
    assert_eq!(r.cash_before, cash0);
    assert_eq!(r.cash_after, cash0 - r.fines - r.bail);
    assert_eq!(r.bounty_payout, 0);
    assert_eq!(r.continue_cost, fee + r.repair_cost);
    assert!(r.can_continue);
    let reasons: Vec<&str> = r.ledger.iter().map(|l| l.reason.as_str()).collect();
    assert!(
        reasons.contains(&"Fine") && reasons.contains(&"CarImpounded"),
        "{reasons:?}"
    );

    // The run is over: live input is ignored and the car stays braked to a halt.
    assert!(s.held(PLAYER));
    let full = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    for _ in 0..240 * 2 {
        assert!(!s.set_input(PLAYER, full).unwrap());
        s.run_ticks(1);
    }
    assert!(speed(&s, PLAYER) < 1.0, "{}", speed(&s, PLAYER));
    assert_eq!(s.input(PLAYER).brake, 1.0);

    let health_before = s
        .sim()
        .vehicles()
        .iter()
        .find(|v| v.id == PLAYER)
        .unwrap()
        .damage
        .health();
    let cash1 = s.run().unwrap().account().cash;
    let c = s.repair_and_continue().unwrap();
    assert_eq!(c.impound_paid, fee);
    assert_eq!(c.repair_paid, r.repair_cost);
    assert_eq!(c.cash_after, cash1 - fee - r.repair_cost);
    assert!(c.ledger.iter().any(|l| l.reason == "ImpoundRetrieved"));
    s.run_ticks(1);
    let after = s.sim().vehicles().iter().find(|v| v.id == PLAYER).unwrap();
    assert_eq!(after.damage.health(), 1.0, "repaired from {health_before}");
    assert!(!after.damage.is_total_loss());
    // Control is back.
    assert!(!s.held(PLAYER));
    assert!(s.set_input(PLAYER, full).unwrap());
    // A second "Repair & continue" is refused: the run is not over.
    assert!(s.repair_and_continue().is_err());
    // Whole-session ledger: gap-free and ends at the account's cash.
    let ledger = s.run().unwrap().ledger();
    assert!(ledger.windows(2).all(|w| w[1].seq == w[0].seq + 1));
    assert_eq!(ledger.last().unwrap().cash_after, c.cash_after);
    // The repair is a replay op (simcli `OpKind::Repair`) and survives the binary format.
    let rp = s.record_stop().unwrap();
    assert!(rp.ops.iter().any(|o| o.op == OpKind::Repair));
    assert_eq!(simcli::replay::Replay::decode(&rp.encode()).unwrap(), rp);
}

#[test]
fn escape_pays_bounty_and_the_run_goes_on() {
    let mut s = parked_near_police(14.0, 30.0);
    // Wait for the pursuit, then vanish: far from every unit, out of sight.
    for _ in 0..240 * 20 {
        s.set_input(PLAYER, brake()).unwrap();
        s.run_ticks(1);
        if crate::police::status(s.sim().police(), PLAYER).status == "Pursuit" {
            break;
        }
    }
    assert_eq!(
        crate::police::status(s.sim().police(), PLAYER).status,
        "Pursuit"
    );
    let units: Vec<[f32; 2]> = s
        .sim()
        .police()
        .units()
        .iter()
        .map(|u| u.body.pos)
        .collect();
    let pois: Vec<[f32; 2]> = s
        .sim()
        .world()
        .network()
        .lanes
        .iter()
        .flat_map(|l| l.polyline.iter().map(|p| [p[0], p[1]]))
        .collect();
    let far = pois
        .iter()
        .copied()
        .max_by(|a, b| {
            let d = |p: &[f32; 2]| {
                units
                    .iter()
                    .map(|u| (u[0] - p[0]).hypot(u[1] - p[1]))
                    .fold(f32::MAX, f32::min)
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    s.debug_sim_mut()
        .unwrap()
        .place_on_ground(PLAYER, far, 0.0, 0.0, None)
        .unwrap();
    let r = wait_for_report(&mut s, 90);
    assert_eq!(r.outcome, RunOutcome::Escaped, "{r:?}");
    assert!(!r.run_over);
    assert!(r.bounty_payout > 0, "{r:?}");
    assert_eq!(r.fines, 0);
    assert_eq!(r.car_status, CarStatus::Garage);
    assert_eq!(r.impound_fee, None);
    // Escapes pay bounty and cash (`stakes.toml [bounty.escape_cash]`, machine G).
    assert!(r.escape_cash > 0, "{r:?}");
    assert_eq!(r.cash_after, r.cash_before + r.escape_cash);
    assert!(r.ledger.iter().any(|l| l.reason == "Escape"));
    assert!(!s.held(PLAYER));
    assert!(s.repair_and_continue().is_err(), "nothing to continue from");
}

#[test]
fn police_sirens_follow_pursuit_state() {
    let mut s = parked_near_police(14.0, 0.0);
    let police: Vec<EntityId> = s
        .sim()
        .vehicles()
        .iter()
        .filter(|v| v.kind == wp_sim::VehicleKind::Police)
        .map(|v| v.id)
        .collect();
    let on = |s: &Session, id: EntityId| {
        let v = s.sim().vehicles().iter().find(|v| v.id == id).unwrap();
        crate::police::siren_on(s.sim(), v)
    };
    // Nobody is wanted: patrols run dark.
    for id in &police {
        assert!(!on(&s, *id), "patrol car {id:?} has its siren on");
    }
    s.debug_sim_mut()
        .unwrap()
        .police_mut()
        .set_heat(PLAYER, 30.0);
    let mut seen = false;
    for _ in 0..240 * 20 {
        s.set_input(PLAYER, brake()).unwrap();
        s.run_ticks(1);
        let pursuing: Vec<EntityId> = s
            .sim()
            .police()
            .units()
            .iter()
            .filter(|u| matches!(u.assignment, Assignment::Pursue { .. }))
            .filter_map(|u| u.entity)
            .collect();
        for id in &pursuing {
            assert!(on(&s, *id));
            seen = true;
        }
        if seen {
            break;
        }
    }
    assert!(seen, "a unit pursues");
    // vehicle_meta's flag 1 is the same rule.
    let patrols = s
        .sim()
        .police()
        .units()
        .iter()
        .filter(|u| matches!(u.assignment, Assignment::Patrol { .. }))
        .filter_map(|u| u.entity)
        .count();
    let dark = s
        .sim()
        .vehicles()
        .iter()
        .filter(|v| v.kind == wp_sim::VehicleKind::Police && !crate::police::siren_on(s.sim(), v))
        .count();
    assert!(dark >= patrols);
}

#[test]
fn restart_keeps_the_account() {
    let mut s = parked_near_police(14.0, 30.0);
    let r = wait_for_report(&mut s, 60);
    assert_eq!(r.outcome, RunOutcome::Captured);
    let prev = s.take_run().unwrap();
    let cash = prev.account().cash;
    let n = prev.ledger().len();
    let mut sc = night_run_in("debug_sedan", DamageMode::Arcade, procedural_city());
    sc.entities[0].speed_mps = 0.0;
    let mut next = Session::new(sc, false).unwrap();
    next.adopt_account(prev);
    let run = next.run().unwrap();
    assert_eq!(run.account().cash, cash);
    assert_eq!(run.ledger().len(), n);
    assert!(!run.is_over() && run.report().is_none());
    // The impounded car stays in the pound; the restart drives a car from the garage.
    let a = run.account();
    assert_eq!(a.garage.len(), 2);
    let active = a.car(a.active_car.unwrap()).unwrap();
    assert_eq!(active.location, wp_economy::CarLocation::Garage);
}

#[test]
fn replays_and_scripted_sessions_have_no_economy_run() {
    let mut sc = night_run_in("debug_sedan", DamageMode::Arcade, procedural_city());
    sc.duration_s = 1.0;
    assert!(Session::new(sc.clone(), true).unwrap().run().is_none());
    let mut live = Session::new(sc, false).unwrap();
    assert!(live.run().is_some());
    live.run_ticks(10);
    let replay = live.record_stop().unwrap();
    assert!(Session::from_replay(replay).unwrap().run().is_none());
}

/// P9: a helicopter put up over a pursued racer on Port Vesper tracks it while it drives off
/// (the browser e2e's flow, natively).
#[test]
fn helicopter_tracks_a_racer_driving_off_in_port_vesper() {
    let mut sc = night_run_in(
        "debug_sedan",
        DamageMode::Arcade,
        crate::scenario::city_spec("vesper_v2"),
    );
    sc.entities[0].speed_mps = 0.0;
    let mut s = Session::new(sc, false).unwrap();
    let mut parked = false;
    for _ in 0..240 * 10 {
        s.run_ticks(1);
        if s.debug_park_near_police(PLAYER, 14.0).is_ok() {
            parked = true;
            break;
        }
    }
    assert!(parked, "no police car spawned");
    s.debug_sim_mut()
        .unwrap()
        .police_mut()
        .set_heat(PLAYER, 60.0);
    let mut unit = None;
    for _ in 0..240 * 30 {
        s.run_ticks(1);
        unit = s
            .debug_sim_mut()
            .unwrap()
            .police_mut()
            .debug_launch_helicopter(PLAYER, 120.0);
        if unit.is_some() {
            break;
        }
    }
    assert!(unit.is_some(), "pursuit never started");
    let throttle = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    let mut visual = 0;
    for k in 0..240 * 20 {
        s.set_input(PLAYER, throttle).unwrap();
        s.run_ticks(1);
        if k % 24 == 0 {
            let h = crate::police::helicopters(s.sim().police(), &|p, z| {
                s.sim().world().map().ground_height(p, z)
            });
            if h.len() == crate::police::HELO_STRIDE && h[7] == 1.0 {
                visual += 1;
            }
        }
    }
    assert!(visual > 100, "visual in {visual} of 200 samples");
}

// ----- garage visits (`crate::garage_visit`) ------------------------------------------------

/// Free drive in the procedural city, one tick in (the car has left any garage it spawned in).
fn free_drive_city() -> Session {
    let sc = crate::scenario::free_drive_in("debug_sedan", DamageMode::Arcade, procedural_city());
    let mut s = Session::new(sc, false).unwrap();
    assert!(
        s.run().is_some(),
        "free drive in the city has an economy run"
    );
    assert!(!s.garages().is_empty(), "the procedural city has garages");
    s.run_ticks(1);
    s
}

/// Drive the player into a static box at `speed_mps` until it is damaged, then clear its heat.
fn crash(s: &mut Session, speed_mps: f32) {
    let st = state(s, PLAYER);
    let sim = s.debug_sim_mut().unwrap();
    // A wall 25 m ahead along the car's +Y (yaw 0 = +Y).
    sim.place_on_ground(
        PLAYER,
        [st.pos[0], st.pos[1]],
        0.0,
        speed_mps,
        Some(st.pos[2]),
    )
    .unwrap();
    sim.add_static_box(
        [st.pos[0], st.pos[1] + 25.0, st.pos[2] + 1.0],
        [8.0, 1.0, 3.0],
        0.0,
    );
    for _ in 0..240 * 4 {
        s.run_ticks(1);
    }
    let health = health(s);
    assert!(health < 1.0, "the crash did no damage ({health})");
    s.debug_sim_mut()
        .unwrap()
        .police_mut()
        .set_heat(PLAYER, 0.0);
}

fn health(s: &Session) -> f32 {
    s.sim()
        .vehicles()
        .iter()
        .find(|v| v.id == PLAYER)
        .unwrap()
        .damage
        .health()
}

/// Put the player in garage `i` at `speed_mps` and run `ticks` (a teleport: for the
/// pursuit test, where the drive itself does not matter).
fn teleport_into(s: &mut Session, i: usize, speed_mps: f32, ticks: u64) {
    let g = s.garages()[i].clone();
    s.debug_sim_mut()
        .unwrap()
        .place_on_ground(
            PLAYER,
            [g.pos[0], g.pos[1]],
            g.heading_rad,
            speed_mps,
            Some(g.pos[2]),
        )
        .unwrap();
    s.run_ticks(ticks);
}

/// Drive into garage `i` along its lane: start `GARAGE_APPROACH_M` short of it at
/// `cruise_mps`, steer for the shop, and (with `stop`) brake inside the ring. Returns once a
/// visit opens or after 20 s; the car's closest approach in m.
pub(crate) fn drive_into(
    s: &mut Session,
    id: EntityId,
    i: usize,
    cruise_mps: f32,
    stop: bool,
) -> f32 {
    const GARAGE_APPROACH_M: f32 = 35.0;
    let g = s.garages()[i].clone();
    let (sn, cs) = wp_math::sin_cos(g.heading_rad);
    let fwd = [-sn, cs];
    let start = [
        g.pos[0] - fwd[0] * GARAGE_APPROACH_M,
        g.pos[1] - fwd[1] * GARAGE_APPROACH_M,
    ];
    s.debug_sim_mut()
        .unwrap()
        .place_on_ground(id, start, g.heading_rad, cruise_mps, Some(g.pos[2]))
        .unwrap();
    // Aim 20 m past the shop on its lane line.
    let aim = [g.pos[0] + fwd[0] * 20.0, g.pos[1] + fwd[1] * 20.0];
    let mut closest = f32::INFINITY;
    for _ in 0..240 * 20 {
        let st = state(s, id);
        let d = (st.pos[0] - g.pos[0]).hypot(st.pos[1] - g.pos[1]);
        closest = closest.min(d);
        let speed = st.lin_vel[0].hypot(st.lin_vel[1]);
        let want = wp_math::atan2(-(aim[0] - st.pos[0]), aim[1] - st.pos[1]);
        let fwd_now = wp_math::atan2(-st.lin_vel[0], st.lin_vel[1]);
        let heading = if speed > 0.5 { fwd_now } else { g.heading_rad };
        let err = wp_math::wrap_angle(want - heading);
        let braking = stop && d < g.radius_m - 4.0;
        let input = VehicleInput {
            steer: (-2.0 * err).clamp(-1.0, 1.0),
            throttle: if braking || speed > cruise_mps {
                0.0
            } else {
                0.5
            },
            brake: if braking { 0.5 } else { 0.0 },
            ..Default::default()
        };
        // The garage holds the car once the visit opens.
        if s.garage_visit().is_some() || s.held(id) {
            break;
        }
        s.set_input(id, input).unwrap();
        s.run_ticks(1);
        if !stop && d > g.radius_m + 5.0 && closest < g.radius_m {
            break;
        }
    }
    closest
}

/// Put the player 200 m away from every garage and run a tick.
fn drive_away(s: &mut Session) {
    let g = s.garages().to_vec();
    let far = s
        .sim()
        .world()
        .network()
        .lanes
        .iter()
        .flat_map(|l| l.polyline.iter().map(|p| [p[0], p[1], p[2]]))
        .find(|p| {
            g.iter()
                .all(|g| (p[0] - g.pos[0]).hypot(p[1] - g.pos[1]) > 200.0)
        })
        .unwrap();
    s.debug_sim_mut()
        .unwrap()
        .place_on_ground(PLAYER, [far[0], far[1]], 0.0, 0.0, Some(far[2]))
        .unwrap();
    s.run_ticks(1);
}

#[test]
fn stopping_in_a_garage_opens_repairs_and_repair_restores_the_car() {
    let mut s = free_drive_city();
    crash(&mut s, 18.0);
    drive_away(&mut s);
    assert!(s.garage_visit().is_none());
    let cash0 = s.run().unwrap().account().cash;

    drive_into(&mut s, PLAYER, 0, 6.0, true);
    let v = s.garage_visit().expect("a visit opened").clone();
    assert_eq!(v.seq, 1);
    assert_eq!(v.site.index, 0);
    assert_eq!(v.wanted, WantedLevel::None);
    assert!(v.health < 1.0);
    let q = v.quote.clone().expect("damage is quoted");
    assert!(q.player_pays > 0, "{q:?}");
    assert!(!q.reports_to_police);
    assert!(v.can_repair);
    assert_eq!(v.cash, cash0);
    // The car is held while the visit is open.
    assert!(s.held(PLAYER));
    let full = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    assert!(!s.set_input(PLAYER, full).unwrap());
    s.run_ticks(240);
    assert!(speed(&s, PLAYER) < 1.0);

    let r = s.garage_repair().unwrap();
    assert_eq!(r.paid, q.player_pays);
    assert_eq!(r.cash_after, cash0 - q.player_pays);
    assert!(r.ledger.iter().any(|l| l.reason == "RepairPaid"));
    s.run_ticks(1);
    assert_eq!(health(&s), 1.0);
    assert!(s.garage_visit().is_none());
    assert!(!s.held(PLAYER));
    assert!(s.set_input(PLAYER, full).unwrap());
    assert_eq!(s.run().unwrap().account().cash, r.cash_after);
    // A second repair is refused: no visit.
    assert!(s.garage_repair().is_err());
    // The repair is a replay op.
    let rp = s.record_stop().unwrap();
    assert!(rp.ops.iter().any(|o| o.op == OpKind::Repair));
    // Whole-session ledger: gap-free and ends at the account's cash.
    let ledger = s.run().unwrap().ledger();
    assert!(ledger.windows(2).all(|w| w[1].seq == w[0].seq + 1));
    assert_eq!(ledger.last().unwrap().cash_after, r.cash_after);
}

#[test]
fn garage_visit_needs_a_stop_and_a_fresh_entry() {
    let mut s = free_drive_city();
    // Driving through at speed does not stop the car.
    let closest = drive_into(&mut s, PLAYER, 0, 12.0, false);
    assert!(closest < 5.0, "drove through the shop ({closest} m)");
    assert!(s.garage_visit().is_none());
    drive_away(&mut s);

    // A pristine car: the visit opens with nothing to repair; "Repair" is refused.
    drive_into(&mut s, PLAYER, 0, 6.0, true);
    let v = s.garage_visit().expect("a visit opened").clone();
    assert!(v.quote.is_none());
    assert_eq!(v.note.as_deref(), Some("nothing to repair"));
    assert!(!v.can_repair);
    assert!(s.garage_repair().is_err());
    assert!(
        s.garage_visit().is_some(),
        "a refused repair keeps the visit open"
    );

    // "Leave" gives control back; staying inside does not reopen it.
    s.garage_leave();
    assert!(!s.held(PLAYER));
    s.run_ticks(240);
    assert!(s.garage_visit().is_none());

    // Out and back in: a new visit.
    drive_away(&mut s);
    drive_into(&mut s, PLAYER, 0, 6.0, true);
    assert_eq!(s.garage_visit().map(|v| v.seq), Some(2));
}

#[test]
fn a_pursuit_keeps_the_garage_shut() {
    let mut s = parked_near_police(14.0, 30.0);
    for _ in 0..240 * 20 {
        s.set_input(PLAYER, brake()).unwrap();
        s.run_ticks(1);
        if crate::police::status(s.sim().police(), PLAYER).status == "Pursuit" {
            break;
        }
    }
    assert_eq!(
        crate::police::status(s.sim().police(), PLAYER).status,
        "Pursuit"
    );
    teleport_into(&mut s, 0, 0.0, 2);
    assert!(
        matches!(
            crate::police::status(s.sim().police(), PLAYER).status,
            "Pursuit" | "Searching"
        ),
        "still chased"
    );
    assert!(s.garage_visit().is_none(), "no visit mid-chase");
}

/// Every shop of the grid city and Port Vesper can be driven into from its lane and stopped
/// in (the grid's block POIs are 18 m off the road or under the viaduct; their shops sit on
/// a street lane).
#[test]
fn every_garage_can_be_driven_into() {
    for city in [procedural_city(), crate::scenario::city_spec("vesper_v2")] {
        let sc = crate::scenario::free_drive_in("debug_sedan", DamageMode::Arcade, city);
        let mut s = Session::new(sc, false).unwrap();
        s.run_ticks(1);
        let n = s.garages().len();
        assert!(n > 0);
        for i in 0..n {
            drive_away(&mut s);
            let closest = drive_into(&mut s, PLAYER, i, 6.0, true);
            let name = s.garages()[i].name.clone();
            let v = s
                .garage_visit()
                .unwrap_or_else(|| panic!("{name}: no visit (closest {closest:.1} m)"));
            assert_eq!(v.site.index, i as u32, "{name}");
            assert!(!v.career);
            s.garage_leave();
        }
    }
}
