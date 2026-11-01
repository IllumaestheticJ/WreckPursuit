//! Slipstream (drafting) through the full game loop (`wp_sim::draft_link`, owner approval
//! 2026-10-31): a car close behind a fast leader on a straight meets less air and pulls more
//! speed than the same car alone; side by side or behind a slow car it gets nothing. Physics,
//! so identical in every damage mode and for every vehicle kind (CLAUDE.md owner rule).

use wp_contracts::{load_car, load_mode_table, EntityId, LoadedCar, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig, VehicleKind, DRAFT_PARAMS};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

const CAR: &str = "kestrel_ts";

/// Where the other car is relative to the measured one.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    /// No other car.
    Solo,
    /// A leader `gap_m` bumper to bumper ahead, same lane.
    Behind { gap_m: f32 },
    /// Alongside, one lane (3.5 m) to the right.
    Beside,
}

struct Run {
    /// Measured car's speed at the end, m/s.
    speed_mps: f32,
    /// Mean `Sim::drafting` of the measured car over the run.
    mean_draft: f32,
    /// Bumper-to-bumper gap at the end (`Behind` only), m.
    gap_end_m: f32,
    hash: u64,
}

/// Both cars start at `speed_mps` heading +Y and hold full throttle (`gas`) or coast for
/// `secs`. Both are `kind` (AI racers and police also burn AI nitrous at full throttle, so
/// the kind comparison coasts).
fn run(
    mode_file: &str,
    layout: Layout,
    speed_mps: f32,
    secs: f32,
    kind: VehicleKind,
    gas: bool,
) -> Run {
    let car: LoadedCar = load_car(data(&format!("cars/{CAR}.json"))).unwrap();
    let len = car.def.body_size_m[1];
    let mode = load_mode_table(data(mode_file)).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 11,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
    });
    let me = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
    sim.set_vehicle_kind(me, kind).unwrap();
    let other_pos = match layout {
        Layout::Solo => None,
        Layout::Behind { gap_m } => Some([0.0, len + gap_m, 0.0]),
        Layout::Beside => Some([3.5, 0.0, 0.0]),
    };
    let other = other_pos.map(|p| {
        let id = sim.spawn_vehicle(&car, p, 0.0).unwrap();
        sim.set_vehicle_kind(id, kind).unwrap();
        id
    });
    // Settle on the springs, then launch both at speed.
    for _ in 0..240 {
        sim.step_fixed();
    }
    let z = |sim: &Sim, id: EntityId| {
        sim.vehicle_states()
            .iter()
            .find(|(e, _)| *e == id)
            .unwrap()
            .1
            .pos[2]
    };
    let zm = z(&sim, me);
    sim.place_vehicle(me, [0.0, 0.0, zm], 0.0, speed_mps)
        .unwrap();
    if let (Some(id), Some(p)) = (other, other_pos) {
        let zo = z(&sim, id);
        sim.place_vehicle(id, [p[0], p[1], zo], 0.0, speed_mps)
            .unwrap();
    }
    let input = VehicleInput {
        throttle: if gas { 1.0 } else { 0.0 },
        ..Default::default()
    };
    sim.set_input(me, input).unwrap();
    if let Some(id) = other {
        sim.set_input(id, input).unwrap();
    }
    let ticks = (secs * 240.0) as u32;
    let mut draft_sum = 0.0f64;
    for _ in 0..ticks {
        sim.step_fixed();
        draft_sum += f64::from(sim.drafting(me).unwrap());
    }
    let states = sim.vehicle_states();
    let mine = &states.iter().find(|(e, _)| *e == me).unwrap().1;
    let gap_end_m = other.map_or(0.0, |id| {
        let o = &states.iter().find(|(e, _)| *e == id).unwrap().1;
        o.pos[1] - mine.pos[1] - len
    });
    Run {
        speed_mps: mine.speed_mps,
        mean_draft: (draft_sum / f64::from(ticks)) as f32,
        gap_end_m,
        hash: sim.state_hash(),
    }
}

const MODES: [&str; 2] = ["modes/arcade.toml", "modes/sim.toml"];

fn tow(mode: &str, secs: f32, kind: VehicleKind, gas: bool) -> Run {
    run(mode, Layout::Behind { gap_m: 5.0 }, 40.0, secs, kind, gas)
}

#[test]
fn tow_five_metres_behind_at_40_mps_gains_speed() {
    for mode in MODES {
        let solo = run(mode, Layout::Solo, 40.0, 4.0, VehicleKind::Player, true);
        let towed = tow(mode, 4.0, VehicleKind::Player, true);
        let gain = towed.speed_mps - solo.speed_mps;
        eprintln!(
            "{mode}: 4 s full throttle from 40 m/s: solo {:.3} m/s, towed {:.3} m/s (+{gain:.3}), \
             mean drag cut {:.1} %, gap 5.0 -> {:.2} m",
            solo.speed_mps,
            towed.speed_mps,
            towed.mean_draft * 100.0,
            towed.gap_end_m
        );
        assert_eq!(solo.mean_draft, 0.0);
        // 5 m behind: 35 % × (1 − 5/25) = 28 % at the start, more as the gap closes.
        assert!(
            towed.mean_draft > 0.25 && towed.mean_draft <= DRAFT_PARAMS.max_reduction,
            "{mode}: mean drag cut {}",
            towed.mean_draft
        );
        assert!(gain > 0.3, "{mode}: tow gained only {gain:.3} m/s");
        // The tow closes the gap, and the follower is still behind (no contact).
        assert!(
            towed.gap_end_m > 0.5 && towed.gap_end_m < 5.0,
            "{mode}: gap {:.2}",
            towed.gap_end_m
        );
    }
}

#[test]
fn coasting_in_the_tow_loses_less_speed() {
    for mode in MODES {
        let solo = run(mode, Layout::Solo, 40.0, 4.0, VehicleKind::Traffic, false);
        let towed = tow(mode, 4.0, VehicleKind::Traffic, false);
        eprintln!(
            "{mode}: 4 s coasting from 40 m/s: solo {:.3}, towed {:.3} m/s",
            solo.speed_mps, towed.speed_mps
        );
        assert!(towed.speed_mps > solo.speed_mps + 0.3, "{mode}");
    }
}

#[test]
fn no_tow_side_by_side_or_behind_a_slow_car() {
    for mode in MODES {
        let solo = run(mode, Layout::Solo, 40.0, 2.0, VehicleKind::Player, true);
        let beside = run(mode, Layout::Beside, 40.0, 2.0, VehicleKind::Player, true);
        assert_eq!(beside.mean_draft, 0.0, "{mode}: side by side");
        assert!(
            (beside.speed_mps - solo.speed_mps).abs() < 0.02,
            "{mode}: beside {} vs solo {}",
            beside.speed_mps,
            solo.speed_mps
        );
        // Below `min_speed_mps` (10 m/s) a car makes no wake (town traffic). Coasting, so the
        // leader stays under it.
        let slow = DRAFT_PARAMS.min_speed_mps * 0.8;
        let solo_slow = run(mode, Layout::Solo, slow, 2.0, VehicleKind::Traffic, false);
        let behind_slow = run(
            mode,
            Layout::Behind { gap_m: 5.0 },
            slow,
            2.0,
            VehicleKind::Traffic,
            false,
        );
        assert_eq!(behind_slow.mean_draft, 0.0, "{mode}: slow leader");
        assert!(
            (behind_slow.speed_mps - solo_slow.speed_mps).abs() < 0.02,
            "{mode}: slow {} vs {}",
            behind_slow.speed_mps,
            solo_slow.speed_mps
        );
    }
}

#[test]
fn same_tow_in_both_modes_and_for_every_leader_kind() {
    // Coasting: no AI nitrous, so every kind drives the same car the same way.
    let base = tow(MODES[0], 2.0, VehicleKind::Player, false);
    assert!(base.mean_draft > 0.25);
    for mode in MODES {
        for kind in [
            VehicleKind::Player,
            VehicleKind::AiRacer,
            VehicleKind::Police,
            VehicleKind::Traffic,
        ] {
            let r = tow(mode, 2.0, kind, false);
            assert_eq!(r.mean_draft, base.mean_draft, "{mode} {kind:?}");
            assert_eq!(r.speed_mps, base.speed_mps, "{mode} {kind:?}");
        }
    }
}

#[test]
fn drafting_is_deterministic() {
    let a = tow(MODES[0], 2.0, VehicleKind::AiRacer, true);
    let b = tow(MODES[0], 2.0, VehicleKind::AiRacer, true);
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.speed_mps, b.speed_mps);
}
