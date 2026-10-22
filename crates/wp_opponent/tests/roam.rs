//! Free-roam street racers in the full `wp_sim` loop: the procedural city with civilian
//! traffic, a handful of roamers from the stock roster on spawn slots, `Roamers` driving them.

mod common;

use common::{capability, car, world, Setup};
use wp_contracts::{
    AssistProfile, DamageSim, EntityId, LoadedCar, SimEvent, TimeOfDay, VehicleSim,
};
use wp_opponent::{
    spawn_slots, stock_roster, Haunt, HauntMap, HauntParams, NoPolice, RoamConfig, RoamEvent,
    RoamPhase, RoamerSpec, Roamers, VehicleObs, AI_CONTACT_IMPULSE_NS,
};
use wp_sim::{Sim, SimConfig, VehicleKind};

struct RoamOutcome {
    roamers: Roamers,
    events: Vec<RoamEvent>,
    /// Roamer–roamer contacts at or above `AI_CONTACT_IMPULSE_NS`.
    hard_contacts: u32,
    /// Longest continuous time each roamer stood still (< 0.5 m/s), s.
    max_still_s: Vec<f32>,
    /// Roamers put back on the road after being stranded for 30 s: (time s, entity, where).
    rescues: Vec<(f32, EntityId, [f32; 3])>,
    /// Wall time inside `Roamers::tick` per 60 Hz frame (4 physics ticks), ms.
    frame_ms: f64,
    sim_hash: u64,
}

fn roam(seed: u64, n: usize, traffic: f32, seconds: f32) -> RoamOutcome {
    let setup = Setup {
        seed,
        traffic,
        closed_roads: false,
        ..Setup::default()
    };
    let w = world(&setup);
    let haunts = HauntMap::classify(&w, &HauntParams::default());
    let roster = stock_roster(n, &haunts.present(), seed);
    let wanted: Vec<Haunt> = roster.iter().map(|e| e.haunt).collect();
    let slots = spawn_slots(&w, &haunts, &wanted, &[], 60.0, seed);
    assert_eq!(slots.len(), n, "room for every roamer");
    if std::env::var("WP_ROAM_TRACE").is_ok() {
        for sl in &slots {
            let l = &w.network().lanes[sl.lane as usize];
            eprintln!(
                "slot {:?} lane {} {:?} road {} limit {:.0} pts {:?}",
                sl.pos,
                sl.lane,
                l.kind,
                l.road,
                l.speed_limit_mps,
                &l.polyline[..l.polyline.len().min(4)]
            );
        }
    }
    let mode = wp_contracts::load_mode_table(common::data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.set_world(w);
    sim.set_systems(wp_sim::Systems {
        police: false,
        ..wp_sim::Systems::ALL
    });
    let mut specs = Vec::new();
    for (e, slot) in roster.iter().zip(&slots) {
        let c: LoadedCar = car(e.car);
        let id = sim.spawn_vehicle(&c, slot.pos, slot.yaw).unwrap();
        sim.set_vehicle_kind(id, VehicleKind::AiRacer).unwrap();
        sim.set_assists(id, AssistProfile::ai()).unwrap();
        specs.push(RoamerSpec {
            entity: id,
            profile: e.profile,
            capability: capability(&c, true),
            haunt: e.haunt,
        });
    }
    let entities: Vec<EntityId> = specs.iter().map(|s| s.entity).collect();
    let haunts_copy = haunts.clone();
    let mut rescues = Vec::new();
    let mut roamers = Roamers::new(
        haunts,
        &specs,
        RoamConfig {
            seed,
            ..RoamConfig::default()
        },
    );
    let mut events: Vec<SimEvent> = Vec::new();
    let mut log = Vec::new();
    let mut hard_contacts = 0;
    let mut tick_wall_s = 0.0f64;
    let mut still = vec![0.0f32; n];
    let mut max_still_s = vec![0.0f32; n];
    let ticks = (seconds * 240.0) as u64;
    while sim.tick() < ticks {
        let obs: Vec<VehicleObs> = sim
            .vehicles()
            .iter()
            .map(|v| VehicleObs {
                entity: v.id,
                state: v.vehicle.state(),
                effects: v.damage.handling_effects(),
                size_m: [
                    v.vehicle.def().body_size_m[0],
                    v.vehicle.def().body_size_m[1],
                ],
            })
            .collect();
        let t0 = std::time::Instant::now();
        let inputs = roamers.tick(sim.world(), &obs, &events, &NoPolice).to_vec();
        tick_wall_s += t0.elapsed().as_secs_f64();
        for (e, i) in inputs {
            sim.set_input(e, i).unwrap();
        }
        let fresh = roamers.drain_events();
        if std::env::var("WP_ROAM_TRACE").is_ok() {
            for e in &fresh {
                eprintln!("t={:.1} {e:?}", sim.tick() as f32 / 240.0);
                if let RoamEvent::LegStarted { entity, .. } = e {
                    let k = entities.iter().position(|x| x == entity).unwrap();
                    let c = roamers.field_of(k).unwrap().corridor();
                    let pts: Vec<_> = c.samples.iter().step_by(5).take(8).map(|s| s.pos).collect();
                    eprintln!("   corridor {pts:?}");
                }
            }
            if sim.tick().is_multiple_of(480) {
                let only: Option<usize> = std::env::var("WP_ROAM_ONLY")
                    .ok()
                    .and_then(|v| v.parse().ok());
                for (k, e) in entities.iter().enumerate() {
                    if only.is_some_and(|o| o != k) {
                        continue;
                    }
                    let st = obs.iter().find(|o| o.entity == *e).unwrap();
                    let (lim, tac, s) = roamers
                        .field_of(k)
                        .map(|f| {
                            let d = f.debug_of(0);
                            (
                                format!("{:?} vt={:.1}", d.limit, d.v_target),
                                format!("{:?}", f.tactic_of(0)),
                                d.s,
                            )
                        })
                        .unwrap_or_default();
                    eprintln!(
                        "t={:.0} #{k} {:?} legs={} d={:.0} pos=({:.0},{:.0},{:.1}) v={:.1} s={s:.0} {lim} {tac}",
                        sim.tick() as f32 / 240.0,
                        roamers.phase_of(k),
                        roamers.legs_of(k),
                        roamers.distance_of(k),
                        st.state.pos[0],
                        st.state.pos[1],
                        st.state.pos[2],
                        st.state.speed_mps
                    );
                }
            }
        }
        log.extend(fresh);
        for e in roamers.stranded(30.0) {
            let here: Vec<[f32; 2]> = obs
                .iter()
                .map(|o| [o.state.pos[0], o.state.pos[1]])
                .collect();
            let k = entities.iter().position(|x| *x == e).unwrap();
            let slot = spawn_slots(
                sim.world(),
                &haunts_copy,
                &[roster[k].haunt],
                &here,
                60.0,
                sim.tick(),
            );
            if let Some(sl) = slot.first() {
                let at = obs.iter().find(|o| o.entity == e).unwrap().state.pos;
                sim.place_on_ground(e, [sl.pos[0], sl.pos[1]], sl.yaw, 0.0, Some(sl.pos[2]))
                    .unwrap();
                sim.repair_vehicle(e).unwrap();
                roamers.relocated(e);
                rescues.push((sim.tick() as f32 / 240.0, e, at));
            }
        }
        sim.step_fixed();
        events = sim.drain_events();
        for ev in &events {
            if let SimEvent::Contact {
                a,
                b,
                impulse_ns,
                point,
                ..
            } = ev
            {
                if std::env::var("WP_ROAM_CONTACTS").is_ok()
                    && (entities.contains(a) || entities.contains(b))
                    && *impulse_ns > 50.0
                {
                    eprintln!(
                        "contact t={:.2} {a:?}-{b:?} {impulse_ns:.0} at {point:?}",
                        sim.tick() as f32 / 240.0
                    );
                }
                if entities.contains(a)
                    && entities.contains(b)
                    && *impulse_ns >= AI_CONTACT_IMPULSE_NS
                {
                    hard_contacts += 1;
                }
            }
        }
        for (k, e) in entities.iter().enumerate() {
            let v = sim.vehicles().iter().find(|v| v.id == *e).unwrap();
            if v.vehicle.state().speed_mps.abs() < 0.5 {
                still[k] += 1.0 / 240.0;
                max_still_s[k] = max_still_s[k].max(still[k]);
            } else {
                still[k] = 0.0;
            }
        }
    }
    RoamOutcome {
        roamers,
        events: log,
        hard_contacts,
        max_still_s,
        rescues,
        frame_ms: tick_wall_s * 1000.0 / (ticks as f64 / 4.0),
        sim_hash: sim.state_hash(),
    }
}

#[test]
fn roamers_keep_driving_through_city_traffic() {
    // Six roamers (highway, downtown and rural haunts on the grid city) for two minutes with
    // 25 % civilian traffic: everyone keeps moving, chains legs, never hits a civilian or
    // another roamer.
    let out = roam(3, 6, 0.25, 120.0);
    let hits: Vec<_> = out
        .events
        .iter()
        .filter(|e| matches!(e, RoamEvent::HitCivilian { .. }))
        .collect();
    assert!(hits.is_empty(), "civilian hits: {hits:?}");
    assert_eq!(out.hard_contacts, 0, "roamer collisions");
    // The grid city's highway diverge ramps run under the mainline's paved edge at about
    // (±15, ±310) and wedge any physical car (world bug, docs/integration/roam.md): rescues
    // there are expected until the ramp is fixed. Anywhere else at most one.
    let elsewhere: Vec<_> = out
        .rescues
        .iter()
        .filter(|r| !((r.2[0].abs() - 15.0).abs() < 5.0 && (r.2[1].abs() - 310.0).abs() < 15.0))
        .collect();
    assert!(elsewhere.len() <= 1, "rescues: {:?}", out.rescues);
    for k in 0..out.roamers.len() {
        let d = out.roamers.distance_of(k);
        assert!(d > 600.0, "roamer {k} drove only {d:.0} m");
        assert!(out.roamers.legs_of(k) >= 1);
        assert_ne!(out.roamers.phase_of(k), RoamPhase::Wrecked, "roamer {k}");
    }
    let legs: u32 = (0..out.roamers.len()).map(|k| out.roamers.legs_of(k)).sum();
    assert!(legs > 6, "roamers chain legs ({legs})");
}

#[test]
fn roaming_is_deterministic() {
    let a = roam(5, 3, 0.15, 25.0);
    let b = roam(5, 3, 0.15, 25.0);
    assert_eq!(a.sim_hash, b.sim_hash);
    assert_eq!(a.roamers.state_hash(), b.roamers.state_hash());
    let c = roam(6, 3, 0.15, 25.0);
    assert_ne!(a.roamers.state_hash(), c.roamers.state_hash());
}

/// Ignored sweep: `cargo test --release -p wp_opponent --test roam -- --ignored --nocapture`.
#[test]
#[ignore]
fn sweep_roam() {
    let seeds: Vec<u64> = match std::env::var("WP_SWEEP_SEED") {
        Ok(v) => vec![v.parse().unwrap()],
        Err(_) => (1..=6).collect(),
    };
    for seed in seeds {
        let out = roam(seed, 8, 0.25, 180.0);
        let hits = out
            .events
            .iter()
            .filter(|e| matches!(e, RoamEvent::HitCivilian { .. }))
            .count();
        let km: f32 = (0..out.roamers.len())
            .map(|k| out.roamers.distance_of(k))
            .sum::<f32>()
            / 1000.0;
        let legs: u32 = (0..out.roamers.len()).map(|k| out.roamers.legs_of(k)).sum();
        let still = out.max_still_s.iter().fold(0.0f32, |m, x| m.max(*x));
        eprintln!(
            "seed {seed}: {km:.1} km, {legs} legs, {hits} civilian hits, {} roamer contacts, longest stand {still:.0} s, {:.3} ms/frame, rescues {:?}",
            out.hard_contacts, out.frame_ms, out.rescues
        );
    }
}
