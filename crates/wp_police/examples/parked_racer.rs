//! Probe for the economy "parked test" (lead round 8): a racer that never moves, made wanted
//! at 03:00 by an unwitnessed red light + speeding report and heat set directly. Prints, every
//! 5 s, the racer's status, the nearest units and the open incidents.
//!
//! `cargo run --release -p wp_police --example parked_racer`

use wp_contracts::{DamageMode, EntityId, SimEvent, TimeOfDay, ViolationKind, PHYSICS_DT};
use wp_police::graph::dist;
use wp_police::scenario::ScenarioWorld;
use wp_police::{PoliceConfig, PoliceEvent, PoliceSetup, PoliceSystem, RacerView};

const RACER: EntityId = EntityId(7);

fn main() {
    let (graph, _map) = wp_police::city::procedural(&Default::default());
    let start = graph.segments[0];
    let p = graph.node(start.a).pos;
    let setup = PoliceSetup {
        seed: 8,
        mode: DamageMode::Arcade,
        internal_driving: true,
        hazards: false,
    };
    let mut police = PoliceSystem::with_city(PoliceConfig::builtin(), graph, setup);
    let mut world = ScenarioWorld::clock(TimeOfDay::from_hours(3.0), 0.0);
    let view = RacerView {
        entity: RACER,
        pos: [p[0], p[1], 0.0],
        vel: [0.0; 3],
    };
    let viol = |kind| SimEvent::TrafficViolation {
        vehicle: RACER,
        kind,
        pos: [p[0], p[1], 0.0],
    };
    println!("racer parked at ({:.0}, {:.0})", p[0], p[1]);
    for tick in 0..240 * 180 {
        world.time = world.time.advanced(PHYSICS_DT * world.ratio);
        let mut inputs = Vec::new();
        match tick {
            1 => inputs.push(viol(ViolationKind::RedLight)),
            2 => inputs.push(viol(ViolationKind::Speeding { over_mps: 12.0 })),
            3 => police.set_heat(RACER, 30.0),
            _ => {}
        }
        let mut out = Vec::new();
        police.step(&world, &[view], &inputs, PHYSICS_DT, &mut out);
        for ev in &out {
            if let Some(e) = PoliceEvent::from_sim_event(ev) {
                if !matches!(e, PoliceEvent::Radio { .. }) {
                    println!("  t={:6.2}  {e:?}", tick as f32 * PHYSICS_DT);
                }
            }
        }
        if tick % (240 * 5) == 0 {
            let mut near: Vec<_> = police
                .units()
                .iter()
                .filter(|u| !u.wrecked)
                .map(|u| (dist(u.body.pos, p), u.id.0, u.assignment, u.body.speed))
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            println!(
                "t={:5.0}s level={:?} status={:?} incidents={:?}",
                tick as f32 * PHYSICS_DT,
                police.wanted_level(RACER),
                police.racer(RACER).map(|r| r.status),
                police
                    .incidents()
                    .iter()
                    .map(|i| (i.id.0, i.unit.map(|u| u.0)))
                    .collect::<Vec<_>>()
            );
            for (d, id, a, v) in near.iter().take(3) {
                println!("    unit {id:3} {d:7.1} m  {v:5.1} m/s  {a:?}");
            }
        }
        if police.wanted_level(RACER) == wp_contracts::WantedLevel::None && tick > 10 {
            println!("ended at t={:.1}s", tick as f32 * PHYSICS_DT);
            break;
        }
    }
}
