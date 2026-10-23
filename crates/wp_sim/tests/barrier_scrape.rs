//! Row 164: Port Vesper's median barriers are smooth. A car glancing into one at 30 m/s and
//! 10° scrapes along it: it stays within ~1.5 m of the barrier face, is not thrown away from
//! it (lateral speed away ≤ 3 m/s) and is not spun (yaw rate ≤ 60°/s). Checked on a straight
//! stretch and on curves bending each way (the barrier's convex face, where per-segment boxes
//! left corners at every joint, and its concave face).

use std::sync::OnceLock;

use wp_contracts::{load_car, load_mode_table, LoadedCar, SimEvent, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_world::geom::{self, V3};
use wp_world::network::RoadNetwork;
use wp_world::vesper::{generate, VesperConfig};
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn net() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| generate(&VesperConfig::default()))
}

fn sim() -> Sim {
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 4,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.set_world(World::new(
        net().clone(),
        WorldConfig {
            traffic: TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            parked_cars: false,
            ..Default::default()
        },
    ));
    sim
}

/// Barrier half width, m (the drawn and solid barrier is 0.5 m wide).
const BARRIER_HALF_M: f32 = 0.25;
/// Arc length a test stretch needs ahead of the start, m.
const STRETCH_M: f32 = 110.0;
/// Approach speed, m/s, and angle into the barrier.
const SPEED: f32 = 30.0;
const ANGLE_DEG: f32 = 10.0;

/// The test car's wheelbase, m, and full steering lock, rad (`debug_sedan.json`).
const WHEELBASE_M: f32 = 2.7;
const MAX_STEER_RAD: f32 = 0.62;
/// The first impact: 0.25 s after the first contact. The car's box chassis takes a rigid
/// corner hit that turns it parallel to the barrier; for this car and a 5.2 m/s closing speed
/// (30 m/s at 10°) that alone is ≈ 95°/s (Δω = m_eff · v · arm / I_z with the front corner
/// 2.2 m ahead of the centre of mass), so the scrape's yaw limit starts after it.
const IMPACT_STEPS: usize = 60;

/// A barrier line and the arc length a scrape starts at.
#[derive(Debug, Clone, Copy)]
struct Site {
    line: usize,
    s: f32,
    /// Heading change over the stretch, rad (+ = bends left, away from the car's side).
    turn: f32,
}

fn heading(line: &[V3], cum: &[f32], s: f32) -> f32 {
    geom::yaw_of(geom::point_at(line, cum, s).1)
}

fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// Candidate stretches on every barrier line, 10 m apart, with their total bend.
fn sites() -> Vec<Site> {
    let mut out = Vec::new();
    for (li, line) in wp_world::surface::median_barriers(net()).iter().enumerate() {
        let cum = geom::cumulative(line);
        let total = *cum.last().unwrap();
        let mut s = 20.0;
        while s + STRETCH_M + 20.0 < total {
            let turn = wrap(heading(line, &cum, s + STRETCH_M) - heading(line, &cum, s));
            out.push(Site { line: li, s, turn });
            s += 10.0;
        }
    }
    out
}

/// Worst readings of one scrape, after the first barrier contact.
#[derive(Debug, Default)]
struct Scrape {
    contacted: bool,
    /// Largest gap between the car's side and the barrier face, m.
    max_gap_m: f32,
    /// Largest speed away from the barrier, m/s.
    max_away_mps: f32,
    /// Largest yaw rate during the first impact ([`IMPACT_STEPS`]), °/s.
    impact_yaw_dps: f32,
    /// Largest yaw rate after it, while scraping, °/s.
    max_yaw_dps: f32,
    /// Largest heading off the barrier's tangent, °.
    max_heading_off_deg: f32,
    /// Speed at the end, m/s.
    end_speed: f32,
}

/// Launch the car `SPEED` m/s at `ANGLE_DEG` into the barrier from 2.6 m right of its line,
/// then hold it lightly against the barrier for 2 s after the first contact.
fn scrape(car: &LoadedCar, site: Site) -> Scrape {
    let lines = wp_world::surface::median_barriers(net());
    let line = &lines[site.line];
    let cum = geom::cumulative(line);
    let half_w = car.def.body_size_m[0] * 0.5;
    let (p, d) = geom::point_at(line, &cum, site.s);
    let r = geom::right_of(d);
    let start = [p[0] + r[0] * 2.6, p[1] + r[1] * 2.6];
    let yaw = geom::yaw_of(d) + ANGLE_DEG.to_radians();
    let mut s = sim();
    let id = s
        .spawn_vehicle(car, [start[0], start[1], 0.0], yaw)
        .unwrap();
    s.place_on_ground(id, start, yaw, SPEED, Some(p[2]))
        .unwrap();
    let mut out = Scrape::default();
    let mut after = 0usize;
    for _ in 0..(4.0 * 240.0) as usize {
        let st = s.vehicle_states()[0].1.clone();
        let q = [st.pos[0], st.pos[1]];
        let pr = geom::project(line, &cum, q);
        let (_, dir) = geom::point_at(line, &cum, pr.s);
        let right = geom::right_of(dir);
        if out.contacted {
            // Hold lightly against the barrier, as a player scraping along would: follow its
            // bend, aim the car's side 5 cm into the face and keep the heading parallel.
            let car_yaw = yaw_of_rot(st.rot);
            let bend = wrap(heading(line, &cum, pr.s + 10.0) - heading(line, &cum, pr.s - 10.0));
            let kappa = bend / 20.0;
            let target = BARRIER_HALF_M + half_w - 0.05;
            let psi = wrap(car_yaw - geom::yaw_of(dir));
            // Right-steer angle, rad: bend feed-forward, lateral and heading feedback.
            let delta = -WHEELBASE_M * kappa - 0.05 * (pr.lateral - target) + 0.6 * psi;
            let steer = (delta / MAX_STEER_RAD).clamp(-1.0, 1.0);
            let throttle = ((SPEED - st.speed_mps) * 0.5).clamp(0.0, 1.0);
            s.set_input(
                id,
                VehicleInput {
                    steer,
                    throttle,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        s.step_fixed();
        let hit = s.drain_events().iter().any(|e| {
            matches!(e, SimEvent::Contact { a, b, .. }
                if (*a == id && b.0 == u32::MAX) || (*b == id && a.0 == u32::MAX))
        });
        if hit && !out.contacted {
            out.contacted = true;
        }
        if out.contacted {
            after += 1;
            // Let the first impact settle for 0.1 s before judging the gap.
            let st = &s.vehicle_states()[0].1;
            let pr = geom::project(line, &cum, [st.pos[0], st.pos[1]]);
            if after > 24 {
                out.max_gap_m = out.max_gap_m.max(pr.lateral - BARRIER_HALF_M - half_w);
            }
            let away = st.lin_vel[0] * right[0] + st.lin_vel[1] * right[1];
            out.max_away_mps = out.max_away_mps.max(away);
            let yaw_dps = st.ang_vel[2].abs().to_degrees();
            if after <= IMPACT_STEPS {
                out.impact_yaw_dps = out.impact_yaw_dps.max(yaw_dps);
            } else {
                out.max_yaw_dps = out.max_yaw_dps.max(yaw_dps);
            }
            let psi = wrap(yaw_of_rot(st.rot) - geom::yaw_of(dir)).to_degrees();
            out.max_heading_off_deg = out.max_heading_off_deg.max(psi.abs());
            out.end_speed = st.speed_mps;
            if after >= 2 * 240 {
                break;
            }
        }
    }
    out
}

/// Heading (yaw about +Z, 0 = +Y) of a quaternion `[x, y, z, w]`.
fn yaw_of_rot(q: [f32; 4]) -> f32 {
    let [x, y, z, w] = q;
    // The rotated +Y axis.
    let fx = 2.0 * (x * y - w * z);
    let fy = 1.0 - 2.0 * (x * x + z * z);
    geom::yaw_of([fx, fy])
}

fn check(name: &str, site: Site) {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let r = scrape(&car, site);
    eprintln!("{name}: {site:?} → {r:?}");
    assert!(r.contacted, "{name}: never touched the barrier");
    assert!(r.max_gap_m < 1.5, "{name}: thrown off the barrier: {r:?}");
    assert!(r.max_away_mps < 3.0, "{name}: kicked away: {r:?}");
    assert!(r.max_yaw_dps < 60.0, "{name}: yaw kick: {r:?}");
    // The first impact may only align the car (no more than a rigid corner hit gives) and
    // must not spin it.
    assert!(r.impact_yaw_dps < 110.0, "{name}: impact spin: {r:?}");
    assert!(
        r.max_heading_off_deg < 15.0,
        "{name}: turned off the barrier: {r:?}"
    );
    assert!(r.end_speed > 15.0, "{name}: snagged: {r:?}");
}

#[test]
fn a_glancing_scrape_slides_along_a_straight_barrier() {
    let site = sites()
        .into_iter()
        .filter(|s| s.turn.abs() < 0.3f32.to_radians())
        .min_by(|a, b| a.turn.abs().total_cmp(&b.turn.abs()))
        .expect("a straight barrier stretch");
    check("straight", site);
}

#[test]
fn a_glancing_scrape_slides_along_a_barrier_bending_away() {
    // Bends left (≈ 16° over the stretch): the car scrapes the barrier's convex face.
    let site = sites()
        .into_iter()
        .max_by(|a, b| a.turn.total_cmp(&b.turn))
        .unwrap();
    assert!(site.turn > 10f32.to_radians(), "{site:?}");
    check("convex", site);
}

#[test]
fn a_glancing_scrape_slides_along_a_barrier_bending_in() {
    // Bends right: the car scrapes the barrier's concave face.
    let site = sites()
        .into_iter()
        .min_by(|a, b| a.turn.total_cmp(&b.turn))
        .unwrap();
    // Port Vesper's right-hand barrier bends are gentle (≈ 4° over the stretch).
    assert!(site.turn < -3f32.to_radians(), "{site:?}");
    check("concave", site);
}
