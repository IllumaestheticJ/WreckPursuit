//! Full-sim race tests on the procedural city (`wp_sim`: Rapier bodies, contacts, damage).
//!
//! The headline test (8 racers × 5 laps) takes ~7 s in release and a few minutes in a debug
//! build; it runs in both.

mod common;

use std::collections::BTreeMap;

use common::Setup;
use wp_contracts::EntityId;
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
    // A much faster car starts behind three sedans and has to get past them; every pass keeps
    // the overtake margin (no contact).
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
        laps: 3,
        drivers,
        cars: vec![
            "debug_sedan".into(),
            "debug_sedan".into(),
            "debug_sedan".into(),
            "lucciola_350".into(),
        ],
        grid: GridLayout::SingleFile,
        max_s: 260.0,
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
    assert!(
        fast_pos <= 1,
        "fast car passed fewer than 2 of 3 sedans: {st:?}"
    );
    let hard = out
        .contacts
        .iter()
        .filter(|c| c.3 >= AI_CONTACT_IMPULSE_NS)
        .count();
    assert_eq!(hard, 0, "{:?}", out.contacts);
    assert!(out.field.passes_by(3) >= 1);
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
    // An illegal street race on open roads: civilian traffic, junctions, signals. Pace is
    // slow (racers yield at busy junctions); what matters is that nobody touches a civilian
    // and nobody hits another racer.
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
    for s in field.standings() {
        assert!(
            s.progress_m > 250.0,
            "{:?} stuck at {:.0} m",
            s.entity,
            s.progress_m
        );
    }
}
