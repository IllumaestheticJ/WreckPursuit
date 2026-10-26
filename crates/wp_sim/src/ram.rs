//! Battering-ram mods in car-on-car crashes (owner, 2026-10-26: "we want to make the cars feel
//! like a battering ram").
//!
//! A car's [`RamProfile`] (bull bars, push bars, armour: `wp_economy::ModEffect::Ram`, set with
//! [`crate::Sim::set_ram_profile`]) gives each face of the car three multipliers:
//!
//! - **Effective mass.** A hit on a face with `mass_scale = k` plays out as if the car weighed
//!   `k` times as much. Rapier solves the contact with the true masses; afterwards each car of
//!   the pair gets a correcting impulse so the exchanged momentum is the one of cars of mass
//!   `k_a·m_a` and `k_b·m_b`. The relative velocity change the solver produced is kept exactly
//!   (so the contact stays resolved): the rammer is slowed less, the car it hits is thrown
//!   harder. Only while the pair is closing at [`RAM_MIN_CLOSING_MPS`] or more, so slowly
//!   pushing a car along gains no free thrust.
//! - **Damage dealt / taken.** The contacts the damage model sees are scaled: the other car's
//!   by my `dealt_scale` (and by how much harder it was thrown), mine by my `taken_scale` (and
//!   by how much less I was slowed).
//!
//! A hit only counts for a face by the share of its impulse whose normal points into that
//! face ([`RamZone::of_normal`]), so a bull bar does nothing for a T-bone on the doors.
//! Contacts with props, parked cars and the static world are not touched (no ram there yet).
//! The same in every damage mode (owner rule: physics never branches on `DamageMode`).
//!
//! Deterministic: pairs are visited in vehicle slot order, contacts in collection order.

use rapier3d::prelude::{RigidBodyHandle, RigidBodySet, Vector};
use wp_contracts::{ContactImpulse, EntityId};
use wp_economy::{RamFace, RamProfile};

/// Least closing speed of a pair for the effective-mass correction, m/s. Below this the
/// cars are leaning on each other, not crashing.
pub const RAM_MIN_CLOSING_MPS: f32 = 1.0;

/// What the ram pass needs to know about one vehicle.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RamCar {
    /// Entity id.
    pub id: EntityId,
    /// Chassis rigid body.
    pub body: RigidBodyHandle,
    /// Its ram faces.
    pub profile: RamProfile,
}

/// Impulse-weighted [`RamFace`] of the contacts in `contacts` that came from `other`, the
/// summed world impulse into the car, the impulse-weighted world point and the pair's
/// largest closing speed.
struct Side {
    face: RamFace,
    impulse_world: Vector,
    point_world: Vector,
    closing: f32,
}

fn side(
    contacts: &[ContactImpulse],
    other: EntityId,
    profile: &RamProfile,
    bodies: &RigidBodySet,
    body: RigidBodyHandle,
) -> Option<Side> {
    let pose = *bodies.get(body)?.position();
    let (mut sum, mut mass, mut dealt, mut taken) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut impulse_world = Vector::ZERO;
    let mut point = Vector::ZERO;
    let mut closing = 0.0f32;
    for c in contacts.iter().filter(|c| c.other == Some(other)) {
        let j = c.impulse_ns;
        if j <= 0.0 {
            continue;
        }
        let f = profile.face_for_normal(c.normal_local);
        sum += j;
        mass += f.mass_scale * j;
        dealt += f.dealt_scale * j;
        taken += f.taken_scale * j;
        impulse_world += pose.transform_vector(Vector::from_array(c.normal_local)) * j;
        point += pose.transform_point(Vector::from_array(c.point_local_m)) * j;
        closing = closing.max(c.rel_speed_mps);
    }
    (sum > 0.0).then(|| Side {
        face: RamFace {
            mass_scale: mass / sum,
            dealt_scale: dealt / sum,
            taken_scale: taken / sum,
        },
        impulse_world,
        point_world: point / sum,
        closing,
    })
}

/// Apply ram profiles to this tick's car-on-car contacts: correcting impulses on the bodies
/// (effective mass) and scaled impulses in `per_vehicle` (damage). `cars[i]` owns
/// `per_vehicle[i]`. No-op when every car is stock.
pub(crate) fn apply(
    cars: &[RamCar],
    per_vehicle: &mut [Vec<ContactImpulse>],
    bodies: &mut RigidBodySet,
) {
    if cars.iter().all(|c| c.profile.is_stock()) {
        return;
    }
    // (car index, other entity, damage multiplier for the car's contacts from other).
    let mut scales: Vec<(usize, EntityId, f32)> = Vec::new();
    for (i, a) in cars.iter().enumerate() {
        if a.profile.is_stock() {
            continue;
        }
        let mut others: Vec<EntityId> = per_vehicle[i].iter().filter_map(|c| c.other).collect();
        others.sort();
        others.dedup();
        for o in others {
            let Some(j) = cars.iter().position(|c| c.id == o) else {
                continue;
            };
            let b = &cars[j];
            // A pair of ram cars is handled once, by the lower slot.
            if !b.profile.is_stock() && j < i {
                continue;
            }
            let Some(sa) = side(&per_vehicle[i], b.id, &a.profile, bodies, a.body) else {
                continue;
            };
            let sb = side(&per_vehicle[j], a.id, &b.profile, bodies, b.body);
            let fb = sb.as_ref().map_or(RamFace::STOCK, |s| s.face);
            let (Some(ra), Some(rb)) = (bodies.get(a.body), bodies.get(b.body)) else {
                continue;
            };
            if !(ra.is_dynamic() && rb.is_dynamic()) {
                continue;
            }
            let (ma, mb) = (ra.mass(), rb.mass());
            let closing = sa.closing.max(sb.as_ref().map_or(0.0, |s| s.closing));
            // g = μ'/μ: how much more momentum the pair exchanges with the scaled masses.
            let (ka, kb) = (sa.face.mass_scale, fb.mass_scale);
            let g = if closing >= RAM_MIN_CLOSING_MPS && ma > 0.0 && mb > 0.0 {
                let mu = ma * mb / (ma + mb);
                let mu2 = (ka * ma) * (kb * mb) / (ka * ma + kb * mb);
                mu2 / mu
            } else {
                1.0
            };
            if g != 1.0 {
                // `impulse_world` is into a; b received the opposite. Keep a's and b's
                // velocity changes those of masses ka·ma and kb·mb under impulse g·J.
                let jw = sa.impulse_world;
                let p = sa.point_world;
                bodies[a.body].apply_impulse_at_point(jw * (g / ka - 1.0), p, true);
                bodies[b.body].apply_impulse_at_point(-jw * (g / kb - 1.0), p, true);
            }
            let (ga, gb) = if g != 1.0 {
                (g / ka, g / kb)
            } else {
                (1.0, 1.0)
            };
            scales.push((i, b.id, ga * sa.face.taken_scale * fb.dealt_scale));
            scales.push((j, a.id, gb * fb.taken_scale * sa.face.dealt_scale));
        }
    }
    for (i, other, s) in scales {
        if s == 1.0 {
            continue;
        }
        for c in per_vehicle[i].iter_mut().filter(|c| c.other == Some(other)) {
            c.impulse_ns *= s;
            for t in &mut c.tangent_impulse_local_ns {
                *t *= s;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Sim, SimConfig, VehicleKind};
    use std::f32::consts::FRAC_PI_2;
    use wp_contracts::{EntityId, LoadedCar, TimeOfDay, VehicleSim, VehicleState};
    use wp_economy::RamProfile;

    const CAR: &str = "tansy_13";

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    fn car(id: &str) -> LoadedCar {
        wp_contracts::load_car(data(&format!("cars/{id}.json"))).unwrap()
    }

    /// The ram faces of real catalog mods.
    fn profile(mods: &[&str]) -> RamProfile {
        let econ = wp_economy::Economy::load(std::path::Path::new(&data(""))).unwrap();
        let set: Vec<_> = mods.iter().map(|m| econ.catalog.get(m).unwrap()).collect();
        RamProfile::of_mods(&set)
    }

    fn state(sim: &Sim, id: EntityId) -> VehicleState {
        sim.vehicles()
            .iter()
            .find(|v| v.id == id)
            .unwrap()
            .vehicle
            .state()
    }

    /// What one crash did: each car's speed change 1 s after launch and plastic energy.
    struct Crash {
        dv_striker: f32,
        dv_struck: f32,
        crush_striker_j: f32,
        crush_struck_j: f32,
    }

    /// Two identical cars: the striker at 15 m/s along +Y into the struck car standing 12 m
    /// ahead at heading `yaw`, each with its ram faces.
    fn crash(mode: &str, striker: RamProfile, struck: RamProfile, yaw: f32) -> Crash {
        let c = car(CAR);
        let mode = wp_contracts::load_mode_table(data(&format!("modes/{mode}.toml"))).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 5,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        let a = sim.spawn_vehicle(&c, [0.0; 3], 0.0).unwrap();
        let b = sim.spawn_vehicle(&c, [0.0, 12.0, 0.0], yaw).unwrap();
        sim.set_vehicle_kind(b, VehicleKind::Traffic).unwrap();
        sim.set_ram_profile(a, striker).unwrap();
        sim.set_ram_profile(b, struck).unwrap();
        for _ in 0..240 {
            sim.step_fixed();
        }
        let za = state(&sim, a).pos[2];
        let zb = state(&sim, b).pos[2];
        sim.place_vehicle(a, [0.0, 0.0, za], 0.0, 15.0).unwrap();
        sim.place_vehicle(b, [0.0, 12.0, zb], yaw, 0.0).unwrap();
        let (va0, vb0) = (state(&sim, a).lin_vel, state(&sim, b).lin_vel);
        for _ in 0..240 {
            sim.step_fixed();
        }
        let (va1, vb1) = (state(&sim, a).lin_vel, state(&sim, b).lin_vel);
        let dv = |v0: [f32; 3], v1: [f32; 3]| (v1[0] - v0[0]).hypot(v1[1] - v0[1]);
        let crush = |id| {
            sim.vehicles()
                .iter()
                .find(|v| v.id == id)
                .unwrap()
                .damage
                .total_plastic_energy_j()
        };
        Crash {
            dv_striker: dv(va0, va1),
            dv_struck: dv(vb0, vb1),
            crush_striker_j: crush(a),
            crush_struck_j: crush(b),
        }
    }

    #[test]
    fn ram_bar_shoves_harder_and_crushes_less() {
        let ram = profile(&["prot_bull_bar_ram"]);
        for mode in ["arcade", "sim"] {
            for (label, yaw) in [("rear-end", 0.0), ("T-bone", FRAC_PI_2)] {
                let stock = crash(mode, RamProfile::default(), RamProfile::default(), yaw);
                let rammed = crash(mode, ram, RamProfile::default(), yaw);
                let l = format!("{mode} {label}");
                // Equal cars: stock trades speed about 1:1; the ram bar keeps more of its own
                // speed and throws the other car harder.
                assert!(
                    rammed.dv_struck > stock.dv_struck * 1.15,
                    "{l}: struck dv {:.2} vs stock {:.2}",
                    rammed.dv_struck,
                    stock.dv_struck
                );
                assert!(
                    rammed.dv_striker < stock.dv_striker * 0.85,
                    "{l}: striker dv {:.2} vs stock {:.2}",
                    rammed.dv_striker,
                    stock.dv_striker
                );
                assert!(
                    rammed.crush_struck_j > stock.crush_struck_j * 1.3,
                    "{l}: struck crush {:.0} J vs stock {:.0} J",
                    rammed.crush_struck_j,
                    stock.crush_struck_j
                );
                assert!(
                    rammed.crush_striker_j < stock.crush_striker_j * 0.7,
                    "{l}: striker crush {:.0} J vs stock {:.0} J",
                    rammed.crush_striker_j,
                    stock.crush_striker_j
                );
            }
        }
    }

    #[test]
    fn a_bull_bar_does_nothing_for_a_hit_on_the_doors() {
        // The struck car's ram bar is on its nose; it is hit in the side.
        let ram = profile(&["prot_bull_bar_ram"]);
        let stock = crash(
            "arcade",
            RamProfile::default(),
            RamProfile::default(),
            FRAC_PI_2,
        );
        let barred = crash("arcade", RamProfile::default(), ram, FRAC_PI_2);
        assert!(
            (barred.dv_struck / stock.dv_struck - 1.0).abs() < 0.05,
            "struck dv {:.2} vs stock {:.2}",
            barred.dv_struck,
            stock.dv_struck
        );
        assert!(
            (barred.crush_struck_j / stock.crush_struck_j - 1.0).abs() < 0.1,
            "struck crush {:.0} J vs stock {:.0} J",
            barred.crush_struck_j,
            stock.crush_struck_j
        );
    }
}
