//! Panel tests: swing period, stop bounce, unlatch thresholds, braking only swings an
//! unlatched door, hanging bumper, detachment with swing velocity, determinism, data files.

use wp_contracts::{BodyRegion, ContactImpulse, DamageSim, LoadedCar, ModeTable, PHYSICS_DT};

use super::*;
use crate::barrier::{barrier_contacts, ImpactKind};
use crate::debris::DetachedKind;

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn sim_mode() -> ModeTable {
    wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap()
}

fn car(id: &str) -> LoadedCar {
    wp_contracts::load_car(data(&format!("cars/{id}.json"))).unwrap()
}

fn derived(car: &LoadedCar) -> PanelSpec {
    PanelSpec::derive(
        &car.def.id,
        &car.lattice,
        DeriveOptions {
            body_size_m: car.def.body_size_m,
            fenders: true,
        },
    )
}

fn model_with_panels(car: &LoadedCar) -> DamageModel {
    let mut m = DamageModel::new(&car.lattice, &car.materials);
    m.attach_panels(&derived(car))
        .expect("derived spec validates");
    m
}

fn step(m: &mut DamageModel, mode: &ModeTable, seconds: f32) {
    let mut out = Vec::new();
    for _ in 0..(seconds / PHYSICS_DT).round() as usize {
        m.apply_contacts(&[], mode, PHYSICS_DT, &mut out);
    }
}

fn status(m: &DamageModel, r: BodyRegion) -> PanelStatus {
    *m.panel_states()
        .iter()
        .find(|p| p.region == r)
        .expect("panel")
}

fn rt(m: &mut DamageModel, r: BodyRegion) -> &mut PanelRt {
    let set = m.panels.as_mut().expect("panels");
    set.panels
        .iter_mut()
        .find(|p| p.def.region == r)
        .expect("panel")
}

/// A barrier-style impact of `kind` at `kmh` with panels attached.
fn crash(
    car: &LoadedCar,
    kind: ImpactKind,
    kmh: f32,
    other_kg: f32,
    seconds: f32,
) -> (DamageModel, Vec<wp_contracts::SimEvent>) {
    let mut m = model_with_panels(car);
    let mode = sim_mode();
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    let (c0, c1) = barrier_contacts(lo, hi, kind, kmh / 3.6, car.def.mass_kg, other_kg);
    let mut out = Vec::new();
    for i in 0..(seconds / PHYSICS_DT).round() as usize {
        let c: &[ContactImpulse] = match i {
            0 => &c0,
            1 => &c1,
            _ => &[],
        };
        m.apply_contacts(c, &mode, PHYSICS_DT, &mut out);
    }
    (m, out)
}

#[test]
fn derived_specs_validate_for_every_car() {
    for entry in std::fs::read_dir(data("cars")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let c = wp_contracts::load_car(&path).unwrap();
        let spec = derived(&c);
        spec.validate(&c.lattice).unwrap();
        let kinds: Vec<PanelKind> = spec.panels.iter().map(|p| p.kind).collect();
        assert!(kinds.contains(&PanelKind::Door), "{}: no doors", c.def.id);
        assert!(
            kinds.contains(&PanelKind::Bumper),
            "{}: no bumpers",
            c.def.id
        );
        assert!(kinds.contains(&PanelKind::Hood), "{}: no hood", c.def.id);
    }
}

#[test]
fn every_car_has_a_valid_panel_file() {
    for entry in std::fs::read_dir(data("cars")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let c = wp_contracts::load_car(&path).unwrap();
        let file = data(&format!("panels/{}.json", c.def.id));
        let spec = PanelSpec::load(std::path::Path::new(&file))
            .unwrap_or_else(|e| panic!("{}: {e}", c.def.id));
        spec.validate(&c.lattice).unwrap();
        let mut m = DamageModel::new(&c.lattice, &c.materials);
        m.attach_panels(&spec).unwrap();
        assert_eq!(m.panel_states().len(), spec.panels.len());
    }
}

#[test]
fn validation_rejects_bad_specs() {
    let c = car("debug_sedan");
    let good = derived(&c);
    let mut bad = good.clone();
    bad.panels[0].hinge_nodes[0] = 0; // a rear-bumper node in the front bumper panel
    assert!(bad.validate(&c.lattice).is_err());
    let mut bad = good.clone();
    bad.panels[0].restitution = 1.5;
    assert!(bad.validate(&c.lattice).is_err());
    let mut bad = good.clone();
    bad.panels.push(bad.panels[0].clone());
    assert!(bad.validate(&c.lattice).is_err(), "duplicate");
    let mut bad = good;
    bad.lattice_id = "other".into();
    assert!(bad.validate(&c.lattice).is_err());
    assert!(PanelSpec::parse("x", r#"{"id":"y","lattice_id":"y","panels":[]}"#).is_err());
}

#[test]
fn no_panels_means_no_change() {
    let c = car("debug_sedan");
    let (a, _) =
        crate::barrier::run_barrier(&c, &sim_mode(), ImpactKind::Side, 30.0, 1400.0, 0.4, 1);
    let (b, _) =
        crate::barrier::run_barrier(&c, &sim_mode(), ImpactKind::Side, 30.0, 1400.0, 0.4, 1);
    assert_eq!(a.state_hash(), b.state_hash());
    assert!(a.panel_states().is_empty());
    // Attaching panels to a car that is never hit changes nothing either.
    let mut idle = model_with_panels(&c);
    step(&mut idle, &sim_mode(), 1.0);
    assert!(idle
        .panel_states()
        .iter()
        .all(|p| p.state == PanelState::Attached && p.angle_rad == 0.0));
    let plain = DamageModel::new(&c.lattice, &c.materials);
    assert_eq!(idle.node_positions(), plain.node_positions());
}

/// Small oscillations of an unlatched door in a sideways field match `2π √(I / (m a l))`.
#[test]
fn swing_period_matches_pendulum() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    assert!(m.unlatch_panel(BodyRegion::LeftDoorF));
    let mode = sim_mode();
    let a = 6.0;
    let eq: f32 = 0.6;
    // The open door's centre-of-mass direction at angle θ is R(θ)·com_dir0; put the field
    // along the direction it has at θ = eq so that is the equilibrium.
    let (axis, com0) = {
        let p = rt(&mut m, BodyRegion::LeftDoorF);
        p.def.damping_per_s = 0.0;
        p.def.area_m2 = 0.0;
        p.angle = eq - 0.08;
        p.omega = 0.0;
        (p.axis, p.com_dir0)
    };
    let (s, co) = wp_math::sin_cos(eq);
    let dir = rotate(com0, axis, s, co);
    m.set_panel_feel(vec3::scale(dir, a), [0.0; 3]);
    let (mass, gyr, lever) = {
        let p = rt(&mut m, BodyRegion::LeftDoorF);
        (p.def.mass_kg, p.def.gyration_m, p.def.lever_m)
    };
    let expected = 2.0 * std::f32::consts::PI * (mass * gyr * gyr / (mass * a * lever)).sqrt();
    let mut crossings = Vec::new();
    let mut prev = status(&m, BodyRegion::LeftDoorF).angle_rad - eq;
    let mut out = Vec::new();
    let mut t = 0.0;
    while crossings.len() < 5 && t < 20.0 {
        m.apply_contacts(&[], &mode, PHYSICS_DT, &mut out);
        t += PHYSICS_DT;
        let x = status(&m, BodyRegion::LeftDoorF).angle_rad - eq;
        if prev < 0.0 && x >= 0.0 {
            crossings.push(t);
        }
        prev = x;
    }
    assert!(crossings.len() >= 3, "door did not oscillate");
    let period = (crossings[crossings.len() - 1] - crossings[0]) / (crossings.len() - 1) as f32;
    assert!(
        (period - expected).abs() < 0.04 * expected,
        "period {period} s vs pendulum {expected} s"
    );
}

#[test]
fn stop_bounce_reverses_with_restitution_and_loads_the_hinge() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    assert!(m.unlatch_panel(BodyRegion::LeftDoorF));
    let mode = sim_mode();
    let e = {
        let p = rt(&mut m, BodyRegion::LeftDoorF);
        p.def.damping_per_s = 0.0;
        p.angle = 0.6;
        p.omega = 4.0;
        p.def.restitution
    };
    m.set_panel_feel([0.0; 3], [0.0; 3]);
    let mut out = Vec::new();
    let mut before = 0.0;
    for _ in 0..240 {
        let s = status(&m, BodyRegion::LeftDoorF);
        if s.omega_rad_s > 0.0 {
            before = s.omega_rad_s;
        }
        m.apply_contacts(&[], &mode, PHYSICS_DT, &mut out);
        let after = status(&m, BodyRegion::LeftDoorF);
        if after.omega_rad_s < 0.0 {
            assert!(
                ((-after.omega_rad_s) - e * before).abs() < 0.05 * before,
                "bounce {} vs {}",
                after.omega_rad_s,
                e * before
            );
            assert!(after.angle_rad <= 1.2 + 1e-4);
            assert!(after.hinge_load_ns > 0.0, "slam loads the hinge");
            return;
        }
    }
    panic!("never reached the stop");
}

#[test]
fn hard_braking_opens_a_door_only_once_unlatched() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    let mode = sim_mode();
    let braking = [0.0, 9.0, -9.81]; // g − a with a = −9 m/s² along +Y
    m.set_panel_feel(braking, [0.0; 3]);
    step(&mut m, &mode, 1.0);
    let s = status(&m, BodyRegion::LeftDoorF);
    assert_eq!(s.state, PanelState::Attached);
    assert_eq!(s.angle_rad, 0.0);
    let rest = c.lattice.nodes[c.lattice.detachables[4].hinge_nodes[0] as usize].pos_m;
    assert_eq!(
        m.node_positions()[c.lattice.detachables[4].hinge_nodes[0] as usize],
        rest
    );

    assert!(m.unlatch_panel(BodyRegion::LeftDoorF));
    step(&mut m, &mode, 0.8);
    let s = status(&m, BodyRegion::LeftDoorF);
    assert_eq!(s.state, PanelState::Unlatched);
    assert!(
        s.angle_rad > 0.5,
        "door swung open under braking: {}",
        s.angle_rad
    );
    // The door's rear edge moved out (-X) and forward.
    let latch = derived(&c)
        .panels
        .iter()
        .find(|p| p.region == BodyRegion::LeftDoorF)
        .unwrap()
        .latch_nodes[0] as usize;
    let p = m.node_positions()[latch];
    let r = c.lattice.nodes[latch].pos_m;
    assert!(p[0] < r[0] - 0.1, "rear edge swung outward: {p:?} vs {r:?}");
    assert!(p[1] > r[1], "and forward");
    // Accelerating instead swings it shut and it settles on the closed stop.
    m.set_panel_feel([0.0, -6.0, -9.81], [0.0; 3]);
    step(&mut m, &mode, 3.0);
    assert!(status(&m, BodyRegion::LeftDoorF).angle_rad < 0.05);
}

#[test]
fn airflow_pushes_an_open_door_shut() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    let mode = sim_mode();
    assert!(m.unlatch_panel(BodyRegion::RightDoorF));
    rt(&mut m, BodyRegion::RightDoorF).angle = 0.9;
    rt(&mut m, BodyRegion::RightDoorF).omega = 0.0;
    // Level road, 30 m/s: the door hinges at its front edge, so the wind closes it.
    m.set_panel_feel([0.0, 0.0, -9.81], [0.0, -30.0, 0.0]);
    step(&mut m, &mode, 1.5);
    // It slammed into the closed stop (hinge load) and flaps near closed in the wind.
    let s = status(&m, BodyRegion::RightDoorF);
    assert!(s.hinge_load_ns > 0.0 && s.angle_rad < 0.4, "{s:?}");
}

#[test]
fn light_side_contact_keeps_the_latch_heavy_one_pops_it() {
    let c = car("debug_sedan");
    let (soft, _) = crash(&c, ImpactKind::Side, 8.0, 1400.0, 0.5);
    let s = status(&soft, BodyRegion::LeftDoorF);
    assert_eq!(
        s.state,
        PanelState::Attached,
        "8 km/h nudge must not unlatch"
    );
    let (hard, _) = crash(&c, ImpactKind::Side, 35.0, 1400.0, 1.5);
    let s = status(&hard, BodyRegion::LeftDoorF);
    assert_ne!(
        s.state,
        PanelState::Attached,
        "35 km/h side hit pops the door latch"
    );
}

#[test]
fn frontal_hit_leaves_the_bumper_hanging() {
    let c = car("debug_sedan");
    let (m, _) = crash(&c, ImpactKind::Offset, 30.0, 0.0, 1.5);
    let s = status(&m, BodyRegion::FrontBumper);
    assert!(
        matches!(s.state, PanelState::HangingByOne | PanelState::Detached),
        "bumper {:?}",
        s.state
    );
}

#[test]
fn hanging_bumper_sags_under_gravity_and_rests_on_its_limit() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    let mode = sim_mode();
    assert!(m.unlatch_panel(BodyRegion::FrontBumper));
    let lim = rt(&mut m, BodyRegion::FrontBumper).def.hang_limit_rad;
    step(&mut m, &mode, 3.0);
    let s = status(&m, BodyRegion::FrontBumper);
    assert_eq!(s.state, PanelState::HangingByOne);
    assert!(
        (s.angle_rad.abs() - lim).abs() < 0.02,
        "rests on the road limit: {}",
        s.angle_rad
    );
    // The free end dropped.
    let spec = derived(&c);
    let b = spec
        .panels
        .iter()
        .find(|p| p.region == BodyRegion::FrontBumper)
        .unwrap();
    let pivot = s.pivot;
    let free_end = b
        .hinge_nodes
        .iter()
        .map(|&i| i as usize)
        .find(|&i| m.node_positions()[i] != pivot)
        .unwrap();
    assert!(m.node_positions()[free_end][2] < c.lattice.nodes[free_end].pos_m[2] - 0.1);
}

#[test]
fn repeated_slams_tear_the_panel_off_with_its_swing_velocity() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    let mode = sim_mode();
    assert!(m.unlatch_panel(BodyRegion::Hood));
    let mut out = Vec::new();
    let mut parts = Vec::new();
    for _ in 0..200 {
        // Fling it open hard every 0.1 s.
        {
            let p = rt(&mut m, BodyRegion::Hood);
            if p.state == PanelState::Detached {
                break;
            }
            p.omega = 12.0;
        }
        for _ in 0..24 {
            m.apply_contacts(&[], &mode, PHYSICS_DT, &mut out);
        }
        parts.extend(m.take_detached_parts());
    }
    let s = status(&m, BodyRegion::Hood);
    assert_eq!(s.state, PanelState::Detached);
    assert!(m
        .log()
        .iter()
        .any(|e| e.kind == crate::DamageLogKind::PartDetached(BodyRegion::Hood)));
    let part = parts
        .iter()
        .find(|p| {
            matches!(
                p.kind,
                DetachedKind::Part {
                    region: BodyRegion::Hood,
                    ..
                }
            )
        })
        .expect("hood debris");
    assert!(
        crate::vec3::length(part.velocity_local) > 1.0,
        "carries swing velocity {:?}",
        part.velocity_local
    );
    assert!(out
        .iter()
        .any(|e| matches!(e, wp_contracts::SimEvent::PartDetached { .. })));
}

#[test]
fn fenders_are_synthetic_groups_that_can_detach() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    let gi = m
        .panels
        .as_ref()
        .unwrap()
        .panels
        .iter()
        .find(|p| p.def.region == BodyRegion::FrontLeftFender)
        .unwrap()
        .gi;
    assert!(m.groups[gi].group >= SYNTHETIC_GROUP_BASE);
    assert!(!m.groups[gi].attach_beams.is_empty());
    let mut out = Vec::new();
    m.detach_group(gi, &mut out);
    let mode = sim_mode();
    step(&mut m, &mode, 0.1);
    assert_eq!(
        status(&m, BodyRegion::FrontLeftFender).state,
        PanelState::Detached
    );
}

#[test]
fn chassis_motion_feeds_gravity_and_braking() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    let id = [0.0, 0.0, 0.0, 1.0];
    m.set_chassis_motion([0.0, 20.0, 0.0], [0.0; 3], id, PHYSICS_DT);
    for _ in 0..60 {
        m.set_chassis_motion([0.0, 20.0, 0.0], [0.0; 3], id, PHYSICS_DT);
    }
    let f = m.panels.as_ref().unwrap().feel;
    assert!((f.accel[2] + 9.81).abs() < 1e-3 && f.accel[1].abs() < 1e-3);
    assert!((f.air[1] + 20.0).abs() < 1e-4);
    // Braking at 9 m/s² for 0.25 s: the panels feel +Y.
    let mut v = 20.0;
    for _ in 0..60 {
        v -= 9.0 * PHYSICS_DT;
        m.set_chassis_motion([0.0, v, 0.0], [0.0; 3], id, PHYSICS_DT);
    }
    let f = m.panels.as_ref().unwrap().feel;
    assert!((f.accel[1] - 9.0).abs() < 0.2, "{:?}", f.accel);
    // Car rotated 90° about Z (facing −X): world braking along −X... is still +Y in chassis.
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let q = [0.0, 0.0, h, h];
    let mut m2 = model_with_panels(&c);
    let mut v = 20.0;
    for _ in 0..120 {
        v -= 9.0 * PHYSICS_DT;
        m2.set_chassis_motion([-v, 0.0, 0.0], [0.0; 3], q, PHYSICS_DT);
    }
    let f = m2.panels.as_ref().unwrap().feel;
    assert!(
        (f.accel[1] - 9.0).abs() < 0.2 && f.accel[0].abs() < 0.2,
        "{:?}",
        f.accel
    );
}

#[test]
fn panel_crashes_are_deterministic() {
    let c = car("debug_sedan");
    let run = || {
        let (mut m, ev) = crash(&c, ImpactKind::Side, 40.0, 1400.0, 1.0);
        m.set_panel_feel([0.0, 7.0, -9.81], [0.0, -15.0, 0.0]);
        step(&mut m, &sim_mode(), 1.0);
        (m.state_hash(), ev.len(), m.panel_states())
    };
    let (h1, e1, s1) = run();
    let (h2, e2, s2) = run();
    assert_eq!(h1, h2);
    assert_eq!(e1, e2);
    assert_eq!(s1, s2);
    // Panel state is part of the hash.
    let (mut m, _) = crash(&c, ImpactKind::Side, 40.0, 1400.0, 1.0);
    let before = m.state_hash();
    rt(&mut m, BodyRegion::LeftDoorF).hinge_load_ns += 5.0;
    assert_ne!(before, m.state_hash());
}

#[test]
fn reset_keeps_the_spec_and_restores_panels() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    m.unlatch_panel(BodyRegion::Trunk);
    step(&mut m, &sim_mode(), 0.2);
    m.reset();
    assert!(m.panel_spec().is_some());
    assert!(m
        .panel_states()
        .iter()
        .all(|p| p.state == PanelState::Attached));
    let groups = m.groups.len();
    m.attach_panels(&derived(&c)).unwrap();
    assert_eq!(
        m.groups.len(),
        groups,
        "re-attach does not duplicate synthetic groups"
    );
}

#[test]
fn contact_on_an_open_door_swings_it() {
    let c = car("debug_sedan");
    let mut m = model_with_panels(&c);
    let mode = sim_mode();
    assert!(m.unlatch_panel(BodyRegion::LeftDoorF));
    rt(&mut m, BodyRegion::LeftDoorF).angle = 1.0;
    rt(&mut m, BodyRegion::LeftDoorF).omega = 0.0;
    m.set_panel_feel([0.0, 0.0, -9.81], [0.0; 3]);
    step(&mut m, &mode, PHYSICS_DT * 2.0);
    // Something hits the open door's outer face from behind, pushing it forward/closed.
    let door = derived(&c)
        .panels
        .iter()
        .find(|p| p.region == BodyRegion::LeftDoorF)
        .unwrap()
        .clone();
    let tip = m.node_positions()[door.latch_nodes[0] as usize];
    let hit = ContactImpulse {
        point_local_m: tip,
        normal_local: [1.0, 0.0, 0.0],
        impulse_ns: 3000.0,
        tangent_impulse_local_ns: [0.0; 3],
        rel_speed_mps: 6.0,
        other: None,
        other_mass_kg: 0.0,
    };
    let mut out = Vec::new();
    m.apply_contacts(&[hit], &mode, PHYSICS_DT, &mut out);
    m.apply_contacts(&[], &mode, PHYSICS_DT, &mut out);
    let s = status(&m, BodyRegion::LeftDoorF);
    assert!(
        s.omega_rad_s.abs() > 1.0,
        "door pushed: ω = {}",
        s.omega_rad_s
    );
}
