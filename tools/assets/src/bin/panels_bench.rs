//! `panels-bench`: cost of panels on top of the lattice, 24 cars (release build):
//! `cargo run --release -p car-assets --bin panels-bench`.
//!
//! Scenarios per car (debug_sedan), each measured with and without panels attached:
//! idle (asleep), a 40 km/h side impact (lattice awake ~0.4 s) followed by 2 s of the door
//! swinging under braking, and the swing-only phase (lattice asleep, panels moving).

use std::time::Instant;

use car_assets::{load_car, panel_path, repo_root};
use wp_contracts::{BodyRegion, ContactImpulse, DamageSim, PHYSICS_DT};
use wp_damage::barrier::{barrier_contacts, ImpactKind};
use wp_damage::{DamageModel, PanelSpec};

const CARS: usize = 24;

fn main() {
    let root = repo_root();
    let car = load_car(&root, "debug_sedan").unwrap();
    let spec = PanelSpec::load(&panel_path(&root, "debug_sedan")).unwrap();
    let mode = wp_contracts::load_mode_table(root.join("data/modes/extreme.toml")).unwrap();
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    let (c0, c1) = barrier_contacts(
        lo,
        hi,
        ImpactKind::Side,
        40.0 / 3.6,
        car.def.mass_kg,
        1400.0,
    );
    let frames_per_s = 60.0;
    for with_panels in [false, true] {
        let mut models: Vec<DamageModel> = (0..CARS)
            .map(|_| {
                let mut m = DamageModel::new(&car.lattice, &car.materials);
                if with_panels {
                    m.attach_panels(&spec).unwrap();
                }
                m
            })
            .collect();
        let mut out = Vec::new();
        // Idle.
        let steps = 240;
        let t = Instant::now();
        for _ in 0..steps {
            for m in &mut models {
                m.apply_contacts(&[], &mode, PHYSICS_DT, &mut out);
            }
        }
        let idle = t.elapsed().as_secs_f64() / steps as f64 / CARS as f64;
        // Crash + swing: 2.4 s.
        let steps = (2.4 / PHYSICS_DT) as usize;
        let t = Instant::now();
        let mut swing_start = 0.0;
        let mut swing_steps = 0;
        let mut awake_steps = 0usize;
        for s in 0..steps {
            let c: &[ContactImpulse] = match s {
                0 => &c0,
                1 => &c1,
                _ => &[],
            };
            if s == 96 {
                swing_start = t.elapsed().as_secs_f64();
            }
            for m in &mut models {
                if s == 96 && with_panels {
                    m.unlatch_panel(BodyRegion::LeftDoorF);
                    m.set_panel_feel([0.0, 8.0, -9.81], [0.0, -20.0, 0.0]);
                }
                m.apply_contacts(c, &mode, PHYSICS_DT, &mut out);
            }
            if s >= 96 {
                swing_steps += 1;
                awake_steps += models.iter().filter(|m| m.is_awake()).count();
            }
            out.clear();
        }
        let total = t.elapsed().as_secs_f64();
        let crash = total / steps as f64 / CARS as f64;
        let swing = (total - swing_start) / swing_steps as f64 / CARS as f64;
        let awake = models.iter().filter(|m| m.is_awake()).count();
        println!(
            "awake lattices at the end: {awake}; awake car-steps in swing phase: {awake_steps}"
        );
        let open = models
            .iter()
            .flat_map(|m| m.panel_states())
            .filter(|p| p.state != wp_damage::PanelState::Attached)
            .count();
        let per_frame = |per_step: f64| per_step * (1.0 / PHYSICS_DT as f64) / frames_per_s * 1e3;
        println!(
            "panels={with_panels:5}  idle {:.2} µs/step ({:.4} ms/frame)  crash+swing {:.1} µs/step ({:.3} ms/frame)  swing-only {:.2} µs/step ({:.4} ms/frame)  panels moving: {open}",
            idle * 1e6,
            per_frame(idle),
            crash * 1e6,
            per_frame(crash),
            swing * 1e6,
            per_frame(swing),
        );
    }
}
