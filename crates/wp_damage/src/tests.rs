//! Unit tests for the damage model: beam material maths, brittle/polymer failure, impacts,
//! containment, components → handling effects, determinism, sleeping, repair and debris.

use std::collections::BTreeMap;

use wp_contracts::{
    BodyRegion, Component, ContactImpulse, DamageMode, DamageSim, HandlingEffects, LatticeBeam,
    LatticeDef, LatticeNode, LoadedCar, MaterialDef, ModeTable, SimEvent, PHYSICS_DT,
};

use crate::barrier::{run_barrier, ImpactKind};
use crate::debris::DetachedKind;
use crate::{default_price_table, DamageModel};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn sim_mode() -> ModeTable {
    wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap()
}

fn arcade_mode() -> ModeTable {
    wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap()
}

fn sedan() -> LoadedCar {
    wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap()
}

fn material(id: &str) -> MaterialDef {
    wp_contracts::load_material_def(data(&format!("materials/{id}.json"))).unwrap()
}

/// Two nodes 1 m apart along +Y, node 0 pinned, one beam of `material` / `area`.
fn one_beam(material_id: &str, area: f32) -> (DamageModel, MaterialDef) {
    let node = |id: u16, y: f32, pinned: bool| LatticeNode {
        id,
        pos_m: [0.0, y, 0.5],
        mass_kg: 1.0,
        pinned_to_chassis: pinned,
        region: BodyRegion::FrameFront,
    };
    let def = LatticeDef {
        id: "beam".into(),
        nodes: vec![node(0, 0.0, true), node(1, 1.0, false)],
        beams: vec![LatticeBeam {
            a: 0,
            b: 1,
            material_id: material_id.into(),
            area_m2: area,
            breakable: true,
            detach_group: None,
        }],
        detachables: vec![],
        components: vec![],
        skin_binding: vec![],
    };
    let m = material(material_id);
    let mut mats = BTreeMap::new();
    mats.insert(m.id.clone(), m.clone());
    let mut model = DamageModel::new(&def, &mats);
    // Give the free node room to move (bounds come from the rest pose).
    model.bounds_lo = [-1.0, -1.0, -1.0];
    model.bounds_hi = [1.0, 3.0, 2.0];
    (model, m)
}

fn run(model: &mut DamageModel, mode: &ModeTable, seconds: f32) -> Vec<SimEvent> {
    let mut out = Vec::new();
    for _ in 0..(seconds / PHYSICS_DT).round() as usize {
        model.apply_contacts(&[], mode, PHYSICS_DT, &mut out);
    }
    out
}

fn beam_len(model: &DamageModel) -> f32 {
    let p = model.node_positions();
    crate::vec3::length(crate::vec3::sub(p[1], p[0]))
}

#[test]
fn elastic_below_yield_no_permanent_set() {
    let area = 1.0e-4;
    let (mut m, mat) = one_beam("hss", area);
    let fy = mat.yield_mpa * 1e6 * area;
    let k = mat.young_mpa * 1e6 * area / 1.0;
    m.set_node_force(1, [0.0, 0.5 * fy, 0.0]);
    run(&mut m, &sim_mode(), 0.5);
    let stretch = beam_len(&m) - 1.0;
    let expected = 0.5 * fy / k;
    assert!(
        (stretch - expected).abs() < 0.15 * expected,
        "elastic stretch {stretch} vs F/k {expected}"
    );
    assert_eq!(
        m.hot[0].rest, m.beams[0].rest0,
        "no plastic flow below yield"
    );
    assert_eq!(m.total_plastic_energy_j(), 0.0);
    m.set_node_force(1, [0.0; 3]);
    run(&mut m, &sim_mode(), 0.5);
    assert!((beam_len(&m) - 1.0).abs() < 1e-4, "springs back fully");
}

#[test]
fn yield_caps_force_and_leaves_plastic_set_with_hardening() {
    let area = 1.0e-4;
    let (mut m, mat) = one_beam("mild_steel", area);
    let fy = mat.yield_mpa * 1e6 * area;
    let et_a = mat.plastic_modulus_mpa * 1e6 * area;
    m.set_node_force(1, [0.0, 1.5 * fy, 0.0]);
    run(&mut m, &sim_mode(), 0.1);
    m.set_node_force(1, [0.0; 3]);
    run(&mut m, &sim_mode(), 0.5);
    let (b, hb) = (&m.beams[0], &m.hot[0]);
    let set = hb.rest - b.rest0;
    assert!(set > 0.01, "permanent set {set}");
    // Permanent set is what remains after unloading (springback ≤ elastic strain at yield).
    assert!((beam_len(&m) - hb.rest).abs() < 2e-3);
    // Hardening: yield force grew with plastic strain exactly per material maths.
    let eps_p = b.plastic_strain;
    assert!((eps_p - set / b.rest0).abs() < 1e-5);
    assert!((hb.lim_t - (fy + et_a * eps_p)).abs() < 1e-3 * fy);
    // Plastic work = ∫ F_yield d(rest), between the initial and the hardened yield force.
    let e = m.total_plastic_energy_j();
    assert!(
        e >= fy * set * 0.999 && e <= (fy + et_a * eps_p) * set * 1.001,
        "work {e}"
    );
}

#[test]
fn structural_beam_buckles_in_compression() {
    let area = 1.0e-4;
    let (mut m, mat) = one_beam("hss", area);
    let fy = mat.yield_mpa * 1e6 * area;
    m.set_node_force(1, [0.0, -2.0 * fy, 0.0]);
    run(&mut m, &sim_mode(), 0.05);
    let (b, hb) = (&m.beams[0], &m.hot[0]);
    assert!(
        b.buckled,
        "compressive yield past buckle strain folds the member"
    );
    let t = &m.tuning;
    let densify = if hb.rest < t.densify_ratio * b.rest0 {
        t.densify_factor
    } else {
        1.0
    };
    assert!(
        (hb.lim_c - hb.lim_t * t.structural_buckle_factor * densify).abs() < 1e-3 * hb.lim_t,
        "buckled member is weaker in compression (until it densifies)"
    );
    assert!(hb.rest >= m.tuning.min_length_ratio * b.rest0 - 1e-6);
    assert!(!hb.broken, "compression folds, it does not tear");
}

#[test]
fn glass_is_brittle_no_plastic_range() {
    let area = 0.6e-4;
    let (mut m, mat) = one_beam("glass", area);
    let fy = mat.yield_mpa * 1e6 * area;
    m.set_node_force(1, [0.0, 0.5 * fy, 0.0]);
    run(&mut m, &sim_mode(), 0.2);
    assert!(!m.hot[0].broken);
    m.set_node_force(1, [0.0, 3.0 * fy, 0.0]);
    run(&mut m, &sim_mode(), 0.2);
    assert!(m.hot[0].broken, "glass breaks instead of yielding");
    assert_eq!(m.total_plastic_energy_j(), 0.0, "no plastic work in glass");
    assert_eq!(m.hot[0].rest, m.beams[0].rest0);
}

#[test]
fn plastic_cover_cracks_when_crushed_or_stretched() {
    let area = 1.0e-4;
    let (mut m, mat) = one_beam("pp_bumper", area);
    let fy = mat.yield_mpa * 1e6 * area;
    m.set_node_force(1, [0.0, -6.0 * fy, 0.0]);
    run(&mut m, &sim_mode(), 0.3);
    assert!(m.hot[0].broken, "crushed polymer cracks");
    let (mut m, _) = one_beam("pp_bumper", area);
    m.set_node_force(1, [0.0, 6.0 * fy, 0.0]);
    run(&mut m, &sim_mode(), 0.3);
    assert!(m.hot[0].broken, "stretched polymer tears");
    assert!(
        m.total_plastic_energy_j() > 0.0,
        "polymer yields before it tears"
    );
}

#[test]
fn beam_break_scale_toughens_beams() {
    let area = 1.0e-4;
    let mat = material("mild_steel");
    let fy = mat.yield_mpa * 1e6 * area;
    let mut tough = sim_mode();
    tough.beam_break_scale = 3.0;
    let (mut a, _) = one_beam("mild_steel", area);
    let (mut b, _) = one_beam("mild_steel", area);
    for m in [&mut a, &mut b] {
        m.set_node_force(1, [0.0, 3.0 * fy, 0.0]);
    }
    let mut out = Vec::new();
    let mut a_broke = None;
    let mut b_broke = None;
    for step in 0..2400 {
        a.apply_contacts(&[], &sim_mode(), PHYSICS_DT, &mut out);
        b.apply_contacts(&[], &tough, PHYSICS_DT, &mut out);
        if a_broke.is_none() && a.hot[0].broken {
            a_broke = Some(step);
        }
        if b_broke.is_none() && b.hot[0].broken {
            b_broke = Some(step);
        }
    }
    let a_broke = a_broke.expect("mild steel tears at its failure strain");
    assert!(b_broke.is_none_or(|s| s > a_broke));
}

#[test]
fn no_contacts_is_neutral_and_cheap() {
    let car = sedan();
    let mut m = DamageModel::new(&car.lattice, &car.materials);
    let ev = run(&mut m, &sim_mode(), 1.0);
    assert!(ev.is_empty());
    assert_eq!(m.handling_effects(), HandlingEffects::NEUTRAL);
    assert_eq!(m.health(), 1.0);
    assert!(!m.is_awake());
    let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
    assert_eq!(m.node_positions(), &rest[..]);
    assert!(m
        .repair_estimate(&sim_mode(), &default_price_table())
        .lines
        .is_empty());
}

#[test]
fn slow_contacts_do_not_deform() {
    let car = sedan();
    let mut m = DamageModel::new(&car.lattice, &car.materials);
    let resting = ContactImpulse {
        point_local_m: [0.0, 2.35, 0.5],
        normal_local: [0.0, -1.0, 0.0],
        impulse_ns: 50.0,
        tangent_impulse_local_ns: [0.0; 3],
        rel_speed_mps: 0.2,
        other: None,
        other_mass_kg: 0.0,
    };
    let mut out = Vec::new();
    for _ in 0..240 {
        m.apply_contacts(&[resting], &sim_mode(), PHYSICS_DT, &mut out);
    }
    assert_eq!(m.total_plastic_energy_j(), 0.0);
    assert_eq!(m.health(), 1.0);
}

#[test]
fn identical_runs_identical_hashes_and_seeds_only_change_pull() {
    let car = sedan();
    let (a, ra) = run_barrier(&car, &sim_mode(), ImpactKind::Offset, 50.0, 0.0, 0.5, 7);
    let (_, rb) = run_barrier(&car, &sim_mode(), ImpactKind::Offset, 50.0, 0.0, 0.5, 7);
    assert_eq!(ra.state_hash, rb.state_hash);
    assert_eq!(ra.metrics, rb.metrics);
    let (c, _) = run_barrier(&car, &sim_mode(), ImpactKind::Offset, 50.0, 0.0, 0.5, 8);
    // The seed only picks the bent rack's pull direction/magnitude, never the deformation.
    assert_eq!(a.node_positions(), c.node_positions());
}

#[test]
fn nodes_stay_inside_chassis_bounds() {
    let car = sedan();
    for mode in [sim_mode(), arcade_mode()] {
        for kind in [
            ImpactKind::Frontal,
            ImpactKind::Offset,
            ImpactKind::Side,
            ImpactKind::Rear,
        ] {
            let (m, r) = run_barrier(&car, &mode, kind, 120.0, 0.0, 0.4, 1);
            assert!(r.contained, "{kind:?} {:?}", mode.mode);
            assert!(m
                .node_positions()
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite())));
        }
    }
}

#[test]
fn left_front_hit_bends_left_front_corner_only() {
    let car = sedan();
    let (m, _) = run_barrier(&car, &sim_mode(), ImpactKind::Side, 40.0, 1400.0, 0.5, 1);
    let e = m.handling_effects();
    let fl = wp_contracts::WHEEL_FL;
    let fr = wp_contracts::WHEEL_FR;
    assert!(m.component_failed(Component::SuspensionFL));
    assert!(
        e.camber_delta[fl].abs() + e.toe_delta[fl].abs() > 0.01,
        "struck corner is knocked out of alignment: {e:?}"
    );
    let far = e.camber_delta[fr].abs() + e.toe_delta[fr].abs();
    let near = e.camber_delta[fl].abs() + e.toe_delta[fl].abs();
    assert!(
        far < 0.3 * near,
        "far corner barely moves: near {near} far {far} {e:?}"
    );
    assert!(e.grip_scale[fl] < e.grip_scale[fr]);
}

#[test]
fn frontal_crash_effects_drive_damaged() {
    let car = sedan();
    let (m, _) = run_barrier(&car, &sim_mode(), ImpactKind::Frontal, 80.0, 0.0, 0.5, 1);
    let e = m.handling_effects();
    assert!(m.is_total_loss());
    assert!(e.engine_dead);
    assert_eq!(e.power_scale, 0.0);
    assert!(e.lights_broken);
    assert!(e.drag_scale > 1.0);
    assert!(e.steer_ratio_scale < 1.0);
}

#[test]
fn radiator_failure_overheats_over_time() {
    let car = sedan();
    let (mut m, _) = run_barrier(&car, &sim_mode(), ImpactKind::Frontal, 60.0, 0.0, 0.3, 1);
    assert!(m.component_failed(Component::Radiator));
    let before = m.handling_effects().overheating;
    run(&mut m, &sim_mode(), 10.0);
    let after = m.handling_effects().overheating;
    assert!(after > before && after <= 1.0, "{before} -> {after}");
}

#[test]
fn arcade_is_more_forgiving_than_sim() {
    let car = sedan();
    for kind in [ImpactKind::Frontal, ImpactKind::Side] {
        let (s, rs) = run_barrier(&car, &sim_mode(), kind, 56.0, 1400.0, 0.5, 1);
        let (a, ra) = run_barrier(&car, &arcade_mode(), kind, 56.0, 1400.0, 0.5, 1);
        assert!(a.health() > s.health(), "{kind:?}");
        assert!(ra.metrics["crush_m"] < rs.metrics["crush_m"]);
        assert!(
            ra.metrics["repair_total"] < rs.metrics["repair_total"],
            "arcade repairs are cheaper"
        );
    }
}

#[test]
fn events_carry_entity_and_are_emitted_once() {
    let car = sedan();
    let mut m = DamageModel::new(&car.lattice, &car.materials);
    m.set_entity(wp_contracts::EntityId(42));
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    let (c0, c1) =
        crate::barrier::barrier_contacts(lo, hi, ImpactKind::Frontal, 80.0 / 3.6, 1400.0, 0.0);
    let mut out = Vec::new();
    m.apply_contacts(&c0, &sim_mode(), PHYSICS_DT, &mut out);
    m.apply_contacts(&c1, &sim_mode(), PHYSICS_DT, &mut out);
    for _ in 0..240 {
        m.apply_contacts(&[], &sim_mode(), PHYSICS_DT, &mut out);
    }
    let mut total_loss = 0;
    let mut failed = BTreeMap::new();
    for e in &out {
        match e {
            SimEvent::TotalLoss { vehicle } => {
                assert_eq!(*vehicle, wp_contracts::EntityId(42));
                total_loss += 1;
            }
            SimEvent::ComponentFailed { vehicle, component } => {
                assert_eq!(*vehicle, wp_contracts::EntityId(42));
                *failed.entry(*component).or_insert(0) += 1;
            }
            SimEvent::WheelDetached { vehicle, .. } | SimEvent::PartDetached { vehicle, .. } => {
                assert_eq!(*vehicle, wp_contracts::EntityId(42));
            }
            SimEvent::ComponentDamaged { health, .. } => assert!((0.0..=1.0).contains(health)),
            _ => {}
        }
    }
    assert_eq!(total_loss, 1);
    assert!(failed.values().all(|&n| n == 1));
    assert!(failed.contains_key(&Component::Engine));
}

#[test]
fn lattice_sleeps_after_a_crash() {
    let car = sedan();
    let (mut m, _) = run_barrier(&car, &sim_mode(), ImpactKind::Frontal, 56.0, 0.0, 0.5, 1);
    run(&mut m, &sim_mode(), 1.5);
    assert!(!m.is_awake(), "a settled lattice stops solving");
    let h = m.state_hash();
    run(&mut m, &sim_mode(), 1.0);
    let steady = m.state_hash();
    // Only the call counter changes while asleep.
    assert_ne!(h, steady);
    let p = m.node_positions().to_vec();
    run(&mut m, &sim_mode(), 1.0);
    assert_eq!(p, m.node_positions());
}

#[test]
fn repair_estimate_grows_with_damage_and_scales_with_mode() {
    let car = sedan();
    let prices = default_price_table();
    let mut last = 0.0;
    for speed in [15.0, 30.0, 50.0, 70.0] {
        let (m, _) = run_barrier(&car, &sim_mode(), ImpactKind::Frontal, speed, 0.0, 0.5, 1);
        let est = m.repair_estimate(&sim_mode(), &prices);
        assert!(est.total > last, "{speed} km/h: {} <= {last}", est.total);
        let sum: f32 = est.lines.iter().map(|l| l.total).sum();
        assert!((sum - est.total).abs() < 1e-2 * est.total.max(1.0));
        for l in &est.lines {
            assert!((0.0..=1.0).contains(&l.severity));
            assert!(l.total >= 0.0);
        }
        let mut cheap = sim_mode();
        cheap.repair_cost_scale = 0.5;
        let half = m.repair_estimate(&cheap, &prices).total;
        assert!((half - 0.5 * est.total).abs() < 1e-2 * est.total);
        last = est.total;
    }
}

#[test]
fn reset_restores_pristine_state() {
    let car = sedan();
    let (mut m, _) = run_barrier(&car, &sim_mode(), ImpactKind::Frontal, 56.0, 0.0, 0.3, 1);
    m.set_entity(wp_contracts::EntityId(3));
    m.reset();
    let fresh = DamageModel::new(&car.lattice, &car.materials);
    assert_eq!(m.node_positions(), fresh.node_positions());
    assert_eq!(m.health(), 1.0);
    assert_eq!(m.entity(), wp_contracts::EntityId(3));
    assert_eq!(m.handling_effects(), HandlingEffects::NEUTRAL);
}

#[test]
fn hull_updates_are_throttled_and_follow_deformation() {
    let car = sedan();
    let mut m = DamageModel::new(&car.lattice, &car.materials);
    let v0 = m.hull_version();
    assert!(m.convex_hull_outer().len() >= 8);
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    let (c0, c1) =
        crate::barrier::barrier_contacts(lo, hi, ImpactKind::Frontal, 50.0 / 3.6, 1400.0, 0.0);
    let mut out = Vec::new();
    let mut versions = vec![v0];
    for step in 0..120 {
        let c: &[ContactImpulse] = match step {
            0 => &c0,
            1 => &c1,
            _ => &[],
        };
        m.apply_contacts(c, &sim_mode(), PHYSICS_DT, &mut out);
        if *versions.last().unwrap() != m.hull_version() {
            versions.push(m.hull_version());
        }
    }
    assert!(versions.len() > 1, "hull follows the crush");
    assert!(
        versions.len() <= 1 + 120 / 20,
        "at most one hull per 20 steps"
    );
    let max_y = m
        .convex_hull_outer()
        .iter()
        .map(|p| p[1])
        .fold(f32::MIN, f32::max);
    assert!(max_y < hi[1] - 0.2, "front of the hull moved back: {max_y}");
}

#[test]
fn detached_parts_become_debris_bodies() {
    let car = sedan();
    let (mut m, _) = run_barrier(&car, &sim_mode(), ImpactKind::Frontal, 80.0, 0.0, 0.5, 1);
    let parts = m.take_detached_parts();
    assert!(!parts.is_empty());
    assert!(m.take_detached_parts().is_empty(), "drained");
    for p in &parts {
        let (body, collider) = p
            .to_rapier(
                [10.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 20.0, 0.0],
                0.32,
                0.22,
            )
            .expect("debris body");
        assert!(body.is_dynamic());
        assert!((collider.mass() - p.mass_kg).abs() < 1e-3 * p.mass_kg);
        if let DetachedKind::Part { group, .. } = p.kind {
            assert!(m.detached_groups().contains(&group));
        }
    }
    for i in 0..m.node_positions().len() {
        if m.node_detached(i) {
            assert!(!m.node_active()[i]);
        }
    }
}

#[test]
fn deform_skin_follows_bound_nodes() {
    let car = sedan();
    let mut def = car.lattice.clone();
    def.skin_binding = vec![wp_contracts::SkinBind {
        vertex_index: 1,
        node_ids: [0, 1, 0, 0],
        weights: [0.5, 0.5, 0.0, 0.0],
    }];
    let mut m = DamageModel::new(&def, &car.materials);
    m.pos[0][1] += 0.2;
    m.pos[1][1] += 0.4;
    let mut out = Vec::new();
    m.deform_skin(&[[0.0; 3], [1.0, 1.0, 1.0]], &mut out);
    assert_eq!(out[0], [0.0; 3]);
    assert!((out[1][1] - 1.3).abs() < 1e-6);
}

#[test]
fn mode_tables_load_and_arcade_is_derived_from_sim() {
    let s = sim_mode();
    let a = arcade_mode();
    assert_eq!(s.mode, DamageMode::Simulation);
    assert_eq!(a.mode, DamageMode::Arcade);
    assert!((a.impulse_to_lattice_scale - 0.45 * s.impulse_to_lattice_scale).abs() < 1e-6);
    assert!(
        (a.component_fail_threshold_scale - 1.8 * s.component_fail_threshold_scale).abs() < 1e-6
    );
    assert!(a.total_loss_health < s.total_loss_health);
    assert!((a.wheel_detach_scale - 0.5 * s.wheel_detach_scale).abs() < 1e-6);
    assert!((a.repair_cost_scale - 0.5 * s.repair_cost_scale).abs() < 1e-6);
    assert_eq!(s.cabin_intrusion_total_loss_m, 0.25);
    assert_eq!(a.cabin_intrusion_total_loss_m, 0.5);
}

#[test]
fn every_body_profile_crashes_plausibly() {
    use crate::lattice_gen::{generate_lattice, BodyProfile, LatticeParams};
    let base = sedan();
    for profile in BodyProfile::ALL {
        let mut p = LatticeParams::new("p", profile, [1.85, 4.7, 1.5], 2.75, 360.0);
        p.car_mass_kg = 1450.0;
        let mut car = base.clone();
        car.lattice = generate_lattice(&p);
        car.def.mass_kg = p.car_mass_kg;
        let (_, front) = run_barrier(&car, &sim_mode(), ImpactKind::Frontal, 56.0, 0.0, 0.5, 1);
        let crush = front.metrics["crush_m"];
        assert!(front.contained, "{profile:?}");
        assert!(
            (0.3..=0.9).contains(&crush),
            "{profile:?} 56 km/h crush {crush}"
        );
        assert!(front.metrics["total_loss"] > 0.5, "{profile:?}");
        let (_, side) = run_barrier(&car, &sim_mode(), ImpactKind::Side, 40.0, 1450.0, 0.5, 1);
        let intrusion = side.metrics["cabin_intrusion_m"];
        assert!(
            (0.05..=0.35).contains(&intrusion),
            "{profile:?} side intrusion {intrusion}"
        );
        assert!(
            side.failed.contains(&Component::SuspensionFL),
            "{profile:?}"
        );
    }
}

#[test]
fn car_to_car_contacts_split_the_energy() {
    // The same impulse and closing speed against a static wall vs another car: each car's
    // lattice gets half the energy against a car (dynamic_closing_share = 0.5).
    let car = sedan();
    let contact = |other_mass_kg: f32| ContactImpulse {
        point_local_m: [0.0, 2.35, 0.5],
        normal_local: [0.0, -1.0, 0.0],
        impulse_ns: 10_000.0,
        tangent_impulse_local_ns: [0.0; 3],
        rel_speed_mps: 15.0,
        other: (other_mass_kg > 0.0).then_some(wp_contracts::EntityId(9)),
        other_mass_kg,
    };
    let injected = |other_mass_kg: f32| {
        let mut m = DamageModel::new(&car.lattice, &car.materials);
        let mut out = Vec::new();
        m.apply_contacts(&[contact(other_mass_kg)], &sim_mode(), PHYSICS_DT, &mut out);
        crate::ALL_REGIONS
            .iter()
            .map(|&r| m.region_injected_energy_j(r))
            .sum::<f32>()
    };
    let wall = injected(0.0);
    let car_hit = injected(1400.0);
    assert!((wall - 0.5 * 10_000.0 * 15.0).abs() < 1.0, "{wall}");
    assert!((car_hit - 0.5 * wall).abs() < 1.0, "{car_hit}");
}

#[test]
fn structural_crush_ignores_cover_and_glass() {
    let car = sedan();
    let (m, r) = run_barrier(&car, &sim_mode(), ImpactKind::Frontal, 56.0, 0.0, 0.5, 1);
    let s = m.structural_crush_m();
    assert!(
        s >= r.metrics["crush_m"] - 1e-6,
        "any-direction >= along-normal"
    );
    assert!(
        s < 1.2 * r.metrics["crush_m"] + 0.05,
        "{s} vs {}",
        r.metrics["crush_m"]
    );
}

#[test]
fn glass_and_region_events_are_emitted_once_per_threshold() {
    let car = sedan();
    let mut m = DamageModel::new(&car.lattice, &car.materials);
    m.set_entity(wp_contracts::EntityId(5));
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    let (c0, c1) =
        crate::barrier::barrier_contacts(lo, hi, ImpactKind::Frontal, 70.0 / 3.6, 1400.0, 0.0);
    let mut out = Vec::new();
    m.apply_contacts(&c0, &sim_mode(), PHYSICS_DT, &mut out);
    m.apply_contacts(&c1, &sim_mode(), PHYSICS_DT, &mut out);
    for _ in 0..240 {
        m.apply_contacts(&[], &sim_mode(), PHYSICS_DT, &mut out);
    }
    let mut glass = Vec::new();
    let mut last: BTreeMap<BodyRegion, f32> = BTreeMap::new();
    for e in &out {
        match e {
            SimEvent::GlassShattered { vehicle, region } => {
                assert_eq!(*vehicle, wp_contracts::EntityId(5));
                glass.push(*region);
            }
            SimEvent::RegionDamaged {
                vehicle,
                region,
                severity,
            } => {
                assert_eq!(*vehicle, wp_contracts::EntityId(5));
                assert!((0.0..=1.0).contains(severity));
                let prev = last.insert(*region, *severity).unwrap_or(0.0);
                assert!(
                    *severity > prev,
                    "{region:?} severity must rise between events"
                );
            }
            _ => {}
        }
    }
    let shattered: Vec<BodyRegion> = m.shattered_regions().iter().copied().collect();
    let mut sorted = glass.clone();
    sorted.sort();
    assert_eq!(sorted, shattered, "one GlassShattered per shattered region");
    assert!(!shattered.is_empty(), "a 70 km/h frontal breaks glass");
    assert!(last.contains_key(&BodyRegion::FrontBumper), "{last:?}");
    assert!(last.len() <= crate::REGION_COUNT);
}
