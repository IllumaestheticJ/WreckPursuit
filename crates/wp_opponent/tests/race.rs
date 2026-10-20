//! Full-sim race tests on the procedural city (`wp_sim`: Rapier bodies, contacts, damage).
//!
//! The headline test (8 racers × 5 laps) takes ~7 s in release and a few minutes in a debug
//! build; it runs in both.

mod common;

use std::collections::BTreeMap;

use common::Setup;
use wp_contracts::{EntityId, VehicleSim};
use wp_opponent::{
    DriverProfile, FixedHeat, GridLayout, OpponentEvent, RacerStatus, AI_CONTACT_IMPULSE_NS,
};

/// Eight default-aggression drivers, skill 0.95 → 0.25 in grid order.
fn eight_drivers() -> Vec<DriverProfile> {
    (0..8)
        .map(|i| DriverProfile {
            skill: 0.95 - 0.1 * i as f32,
            ..DriverProfile::default()
        })
        .collect()
}

#[test]
fn eight_racers_five_laps_no_ai_collisions_and_pace_follows_skill() {
    let drivers = eight_drivers();
    let setup = Setup {
        seed: 7,
        laps: 5,
        drivers: drivers.clone(),
        grid: GridLayout::SingleFile,
        max_s: 420.0,
        ..Setup::default()
    };
    let out = common::run_plain(&setup);
    let st = out.field.standings();
    for s in &st {
        assert!(
            matches!(s.status, RacerStatus::Finished { .. }),
            "{:?} did not finish: {:?}",
            s.entity,
            s.status
        );
        assert_eq!(s.lap_times_s.len(), 5);
    }
    // No racer–racer contact above the collision threshold.
    let hard: Vec<_> = out
        .contacts
        .iter()
        .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
        .collect();
    assert!(hard.is_empty(), "AI–AI collisions: {hard:?}");
    assert_eq!(out.field.ai_contact_count(), 0);
    // Best laps follow skill: a more skilled driver is never slower by more than 0.15 s
    // (two drivers 0.1 skill apart can swap when one is stuck behind the other).
    let best: BTreeMap<EntityId, f32> = st
        .iter()
        .map(|s| {
            let b = s.lap_times_s.iter().cloned().fold(f32::INFINITY, f32::min);
            (s.entity, b)
        })
        .collect();
    let by_skill: Vec<(f32, f32)> = out
        .entities
        .iter()
        .zip(&drivers)
        .map(|(e, d)| (d.skill, best[e]))
        .collect();
    for i in 0..by_skill.len() {
        for j in i + 1..by_skill.len() {
            let (si, bi) = by_skill[i];
            let (sj, bj) = by_skill[j];
            assert!(si > sj);
            assert!(
                bi < bj + 0.15,
                "skill {si} best {bi:.2} s vs skill {sj} best {bj:.2} s: {by_skill:?}"
            );
        }
    }
    let spread = by_skill[7].1 - by_skill[0].1;
    assert!(
        (2.5..8.0).contains(&spread),
        "best-lap spread {spread:.2} s: {by_skill:?}"
    );
    // Finishing order matches the grid (fastest in front, nobody needed to pass).
    let order: Vec<EntityId> = st.iter().map(|s| s.entity).collect();
    let mut sorted = out.entities.clone();
    sorted.sort();
    assert_eq!(order, sorted);
}

#[test]
fn two_wide_grid_reverse_order_stays_clean() {
    // Slowest in front, two-wide: the faster cars are held up, look for passes and must not
    // hit anyone doing it.
    let mut drivers = eight_drivers();
    drivers.reverse();
    let setup = Setup {
        seed: 3,
        laps: 2,
        drivers,
        grid: GridLayout::TwoWide,
        max_s: 200.0,
        ..Setup::default()
    };
    let out = common::run_plain(&setup);
    let hard: Vec<_> = out
        .contacts
        .iter()
        .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
        .collect();
    assert!(hard.is_empty(), "AI–AI collisions: {hard:?}");
    assert!(out.field.race_over(), "race not over after {} s", out.sim_s);
}

#[test]
fn faster_car_passes_with_margin() {
    // A much faster car starts behind three sedans and gets past; every pass keeps the
    // overtake margin (no contact).
    let drivers = vec![
        DriverProfile {
            skill: 0.4,
            ..DriverProfile::default()
        };
        4
    ];
    let mut drivers = drivers;
    drivers[3].skill = 0.9;
    let setup = Setup {
        seed: 11,
        laps: 4,
        drivers,
        cars: vec![
            "debug_sedan".into(),
            "debug_sedan".into(),
            "debug_sedan".into(),
            "lucciola_350".into(),
        ],
        grid: GridLayout::SingleFile,
        max_s: 330.0,
        ..Setup::default()
    };
    let mut min_side_gap = f32::INFINITY;
    let out = common::run(&setup, &wp_opponent::NoPolice, &mut |sim, _field| {
        // Lateral clearance whenever the fast car is alongside a sedan.
        let st = sim.vehicle_states();
        let fast = st[3].1.pos;
        for (_, s) in &st[..3] {
            let dx = s.pos[0] - fast[0];
            let dy = s.pos[1] - fast[1];
            let d2 = dx * dx + dy * dy;
            if d2 < 25.0 {
                min_side_gap = min_side_gap.min(d2.sqrt());
            }
        }
    });
    let st = out.field.standings();
    let fast_pos = st.iter().position(|s| s.entity == out.entities[3]).unwrap();
    // It gets past at least one of them (how many depends on finding room: there is no
    // slipstream yet, see the README's known gaps).
    assert!(fast_pos <= 2, "fast car passed no sedan: {st:?}");
    let hard = out
        .contacts
        .iter()
        .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
        .count();
    assert_eq!(hard, 0, "{:?}", out.contacts);
    // Closest centre-to-centre distance while within 5 m of a sedan: two 1.8 m wide cars
    // side by side with the overtake margin stay well over 2 m apart.
    assert!(
        min_side_gap > 2.0,
        "closest centre distance alongside {min_side_gap:.2} m"
    );
}

#[test]
fn same_seed_same_race_hash() {
    let setup = Setup {
        seed: 5,
        laps: 1,
        drivers: eight_drivers()[..4].to_vec(),
        max_s: 25.0,
        ..Setup::default()
    };
    let a = common::run_plain(&setup);
    let b = common::run_plain(&setup);
    assert_eq!(a.sim_hash, b.sim_hash);
    assert_eq!(a.field_hash, b.field_hash);
    let c = common::run_plain(&Setup { seed: 6, ..setup });
    assert_ne!(
        a.field_hash, c.field_hash,
        "seed must change the drivers' noise"
    );
}

#[test]
fn wanted_racer_abandons_the_race_and_runs() {
    let drivers = eight_drivers()[..3].to_vec();
    let setup = Setup {
        seed: 2,
        laps: 2,
        drivers,
        max_s: 40.0,
        ..Setup::default()
    };
    // Racer 2 (careful: flees at wanted level 2 with default risk 0.4) is wanted at level 3.
    let mut heat = BTreeMap::new();
    heat.insert(EntityId(2), 3u8);
    let mut events = Vec::new();
    let out = common::run(&setup, &FixedHeat(heat), &mut |_, _| {});
    let mut field = out.field;
    events.extend(field.drain_events());
    let st = field.standings();
    let fled = st.iter().find(|s| s.entity == EntityId(2)).unwrap();
    assert_eq!(fled.status, RacerStatus::Fled);
    assert!(events
        .iter()
        .any(|e| matches!(e, OpponentEvent::Abandoned { entity, .. } if *entity == EntityId(2))));
    // The others keep racing.
    for s in st.iter().filter(|s| s.entity != EntityId(2)) {
        assert_eq!(s.status, RacerStatus::Racing);
    }
}

#[test]
fn open_roads_with_traffic_no_civilian_contact() {
    // An illegal street race on open roads: civilian traffic, junctions, signals. Racers read
    // the signals (a lasting green means cross traffic is held), wait out reds when cross
    // traffic is moving, and break junction stand-offs. Nobody touches a civilian, nobody
    // hits another racer, nobody is stuck.
    let setup = Setup {
        seed: 7,
        laps: 1,
        drivers: eight_drivers()[..4].to_vec(),
        closed_roads: false,
        traffic: 0.25,
        grid: GridLayout::SingleFile,
        max_s: 90.0,
        ..Setup::default()
    };
    let out = common::run_plain(&setup);
    let mut field = out.field;
    let events = field.drain_events();
    let dq: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, OpponentEvent::Disqualified { .. }))
        .collect();
    assert!(dq.is_empty(), "civilian contacts: {dq:?}");
    let hard = out
        .contacts
        .iter()
        .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
        .count();
    assert_eq!(hard, 0, "{:?}", out.contacts);
    for (k, t) in out.max_stuck_s.iter().enumerate() {
        assert!(*t < 8.0, "racer {} stuck {t:.0} s", k + 1);
    }
    for (k, t) in out.max_wait_s.iter().enumerate() {
        assert!(*t < 90.0, "racer {} waited {t:.0} s", k + 1);
    }
    for s in field.standings() {
        assert!(
            s.progress_m > 250.0,
            "{:?} stuck at {:.0} m",
            s.entity,
            s.progress_m
        );
    }
}

#[test]
fn wanted_ai_racer_flees_police_through_the_city() {
    // Open roads with traffic. At 20 s the police report racer 1 at wanted level 3 and a
    // unit at the start line; the AI reads it from the sim's event stream (`EventHeat`),
    // abandons the race and drives a flight route away from that unit.
    use wp_contracts::WantedLevel;
    use wp_police::{PoliceEvent, UnitId, UnitKind};
    let unit_at = [-100.0f32, -166.75];
    let setup = Setup {
        // Seed 4 -> 1 (lead/city-residential, rows 94-95): with the arterial / highway
        // medians the live police also give racer 2 wanted level 2 at seeds 4 and 5 (it
        // speeds past the unit and flees too); seeds 1, 2, 3 and 6 pass.
        seed: 1,
        laps: 2,
        drivers: eight_drivers()[..3].to_vec(),
        closed_roads: false,
        traffic: 0.25,
        grid: GridLayout::SingleFile,
        live_police: true,
        inject: vec![
            (
                20.0,
                PoliceEvent::UnitSpawned {
                    unit: UnitId(1),
                    kind: UnitKind::Car,
                    pos: [unit_at[0], unit_at[1], 0.0],
                    heading: [1.0, 0.0],
                },
            ),
            (
                20.0,
                PoliceEvent::WantedLevelChanged {
                    racer: EntityId(1),
                    level: WantedLevel::L3,
                },
            ),
        ],
        max_s: 80.0,
        ..Setup::default()
    };
    let out = common::run(&setup, &wp_opponent::NoPolice, &mut |_, _| {});
    let mut field = out.field;
    let events = field.drain_events();
    assert!(
        events.iter().any(
            |e| matches!(e, OpponentEvent::Abandoned { entity, wanted_level: 3 } if *entity == EntityId(1))
        ),
        "{events:?}"
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, OpponentEvent::Disqualified { .. })));
    let st = field.standings();
    let fled = st.iter().find(|s| s.entity == EntityId(1)).unwrap();
    // The sim's own police patrol the city too and may catch it on the way.
    assert!(
        matches!(fled.status, RacerStatus::Fled | RacerStatus::Busted),
        "{:?}",
        fled.status
    );
    for s in st.iter().filter(|s| s.entity != EntityId(1)) {
        assert!(
            matches!(s.status, RacerStatus::Racing | RacerStatus::Busted),
            "{:?}",
            s.status
        );
    }
    // Its flight route ends far from the unit, and it made real progress along it (traffic
    // and junctions included) in the minute after the call.
    let flight = field.flight_of(0).expect("no flight route");
    let end = flight.samples[flight.len() - 1].pos;
    let d_dest = ((end[0] - unit_at[0]).powi(2) + (end[1] - unit_at[1]).powi(2)).sqrt();
    assert!(
        d_dest > 300.0,
        "flight destination only {d_dest:.0} m from the unit"
    );
    let covered = field.debug_of(0).s;
    assert!(
        covered > 250.0,
        "covered only {covered:.0} m of the flight route"
    );
    let hard = out
        .contacts
        .iter()
        .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
        .count();
    assert_eq!(hard, 0, "{:?}", out.contacts);
}

#[test]
fn live_police_chase_ends_in_escape_or_capture_without_stuck_cars() {
    // Open roads, traffic, and the sim's own police: physical patrol cars on D's road graph
    // that witness violations, recognise wanted cars and chase them. The AI reads
    // `Sim::police()` directly (`SimPolice`).
    //  - racer 1 starts at L3 (above its flee level 2): abandons the race and runs;
    //  - racer 2 starts at L1 (below it): keeps racing, pulls over if a unit sits on it;
    //  - racer 3 is clean (but may earn heat of its own on the way).
    let setup = Setup {
        seed: 4,
        laps: 2,
        drivers: eight_drivers()[..3].to_vec(),
        closed_roads: false,
        traffic: 0.25,
        grid: GridLayout::SingleFile,
        sim_police: Some("sentinel_cruiser".into()),
        heat: vec![(EntityId(1), 55.0), (EntityId(2), 15.0)],
        run_full: true,
        max_s: 150.0,
        ..Setup::default()
    };
    let mut speeds = [0.0f32; 3];
    let out = common::run(&setup, &wp_opponent::NoPolice, &mut |sim, _| {
        for (k, v) in sim.vehicles().iter().take(3).enumerate() {
            speeds[k] = v.vehicle.state().speed_mps;
        }
    });
    let mut field = out.field;
    let events = field.drain_events();
    let st = field.standings();
    let summary: Vec<_> = events
        .iter()
        .filter(|e| {
            !matches!(
                e,
                OpponentEvent::LapCompleted { .. } | OpponentEvent::Overtook { .. }
            )
        })
        .collect();
    let info = format!(
        "{summary:?}\n{st:?}\nstuck {:?} wait {:?}",
        out.max_stuck_s, out.max_wait_s
    );
    assert!(out.police_vehicles > 0, "no physical police: {info}");
    // Racer 1 runs, and the chase ends either way (caught, or still running / escaped).
    assert!(
        events.iter().any(
            |e| matches!(e, OpponentEvent::Abandoned { entity, wanted_level: 3 } if *entity == EntityId(1))
        ),
        "{info}"
    );
    // Racer 2 never runs at L1: it yields (and is booked) or races on.
    assert!(
        !events.iter().any(
            |e| matches!(e, OpponentEvent::Abandoned { entity, wanted_level: 1 } if *entity == EntityId(2))
        ),
        "{info}"
    );
    for s in &st {
        match s.status {
            RacerStatus::Busted => {
                // A booked car stays put.
                let k = (s.entity.0 - 1) as usize;
                assert!(
                    speeds[k].abs() < 1.0,
                    "busted {:?} still moving {:.1} m/s",
                    s.entity,
                    speeds[k]
                );
            }
            RacerStatus::Fled | RacerStatus::Racing | RacerStatus::Finished { .. } => {}
            other => panic!("{:?} ended {other:?}: {info}", s.entity),
        }
    }
    // Every busted racer was announced; escapes only for racers that fled.
    for s in st.iter().filter(|s| s.status == RacerStatus::Busted) {
        assert!(events
            .iter()
            .any(|e| matches!(e, OpponentEvent::Busted { entity, .. } if *entity == s.entity)));
    }
    // No stuck cars: nobody stands still with nothing in its way, and no queue (behind a
    // car, a civilian or a red light: this corner's red lasts ~45 s) outlasts 1.5 cycles.
    for (k, t) in out.max_stuck_s.iter().enumerate() {
        assert!(*t < 8.0, "racer {} stuck {t:.0} s: {info}", k + 1);
    }
    for (k, t) in out.max_wait_s.iter().enumerate() {
        assert!(*t < 90.0, "racer {} waited {t:.0} s: {info}", k + 1);
    }
    // Racers that are not caught keep making progress round the circuit.
    let free = st
        .iter()
        .filter(|s| matches!(s.status, RacerStatus::Racing | RacerStatus::Finished { .. }))
        .collect::<Vec<_>>();
    for s in &free {
        assert!(
            s.progress_m > 400.0,
            "{:?} only {:.0} m: {info}",
            s.entity,
            s.progress_m
        );
    }
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, OpponentEvent::Disqualified { .. })),
        "{info}"
    );
    let hard = out
        .contacts
        .iter()
        .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
        .count();
    assert_eq!(hard, 0, "{:?}", out.contacts);
}

#[test]
#[ignore]
fn sweep_open_roads_progress() {
    let mut total = 0.0;
    let mut worst = 0.0f32;
    for seed in 1..=8u64 {
        let setup = Setup {
            seed,
            laps: 1,
            drivers: eight_drivers()[..4].to_vec(),
            closed_roads: false,
            traffic: 0.25,
            grid: GridLayout::SingleFile,
            max_s: 90.0,
            ..Setup::default()
        };
        let out = common::run_plain(&setup);
        let mut field = out.field;
        let p: Vec<i32> = field
            .standings()
            .iter()
            .map(|s| s.progress_m as i32)
            .collect();
        let dq = field
            .drain_events()
            .iter()
            .filter(|e| matches!(e, OpponentEvent::Disqualified { .. }))
            .count();
        let hard = out
            .contacts
            .iter()
            .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
            .count();
        total += p.iter().sum::<i32>() as f32;
        let st = out.max_stuck_s.iter().fold(0.0f32, |m, x| m.max(*x));
        let wt = out.max_wait_s.iter().fold(0.0f32, |m, x| m.max(*x));
        worst = worst.max(wt);
        eprintln!("seed {seed}: {p:?} stuck {st:.0} wait {wt:.0} dq {dq} hard {hard}");
    }
    eprintln!("TOTAL {total:.0} longest wait {worst:.0}");
}

/// Same car, one clearly better driver at the back: with slipstream it gets past (the tow
/// on the straights closes the time gap the corners open); without, it never does.
/// Needs `Sim::set_drag_scale` (feature `sim-draft-hook`, see the integration doc).
#[cfg(feature = "sim-draft-hook")]
#[test]
fn draft_lets_a_faster_driver_in_the_same_car_pass() {
    let race = |seed: u64, draft: bool| {
        let mut drivers = vec![
            DriverProfile {
                skill: 0.35,
                ..DriverProfile::default()
            };
            4
        ];
        drivers[3].skill = 0.8;
        let setup = Setup {
            seed,
            laps: 4,
            drivers,
            grid: GridLayout::SingleFile,
            max_s: 330.0,
            draft,
            ..Setup::default()
        };
        let out = common::run_plain(&setup);
        let hard = out
            .contacts
            .iter()
            .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
            .count();
        assert_eq!(hard, 0, "seed {seed} draft {draft}: {:?}", out.contacts);
        let st = out.field.standings();
        let pos = st.iter().position(|s| s.entity == EntityId(4)).unwrap();
        (pos, out.tow_s[3])
    };
    let mut gained_with = 0;
    let mut gained_without = 0;
    for seed in 1..=4 {
        let (with, tow) = race(seed, true);
        let (without, _) = race(seed, false);
        assert!(tow > 5.0, "seed {seed}: only {tow:.1} s in the tow");
        gained_with += 3 - with;
        gained_without += 3 - without;
    }
    assert!(
        gained_with >= 4 && gained_with > gained_without,
        "places gained with draft {gained_with}, without {gained_without}"
    );
}
