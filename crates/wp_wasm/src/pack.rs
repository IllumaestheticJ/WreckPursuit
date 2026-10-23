//! Packed `f32`/`u32` layouts handed to JavaScript as typed arrays, plus the debug-only
//! derived quantities (beam strain, region energy). Layouts are
//! documented in `docs/integration/testbed.md` and mirrored in
//! `apps/testbed-web/src/wasm/layout.ts`.

use wp_contracts::{
    BodyRegion, Component, LatticeDef, MaterialDef, SurfaceType, VehicleInput, VehicleState,
};

/// Floats per vehicle in `vehicle_states()`.
pub const STATE_STRIDE: usize = 50;
/// Offset of the first wheel block in a state record.
pub const STATE_WHEEL_BASE: usize = 18;
/// Floats per wheel block in a state record.
pub const STATE_WHEEL_STRIDE: usize = 8;

/// Floats in `set_input()`'s array.
pub const INPUT_LEN: usize = 7;

/// Floats per wheel block in `telemetry()`.
pub const TELEMETRY_WHEEL_STRIDE: usize = 10;
/// Offset of the first wheel block in `telemetry()`.
pub const TELEMETRY_WHEEL_BASE: usize = 13;

/// Every body region in declaration order (index = position in region arrays).
pub const ALL_REGIONS: [BodyRegion; 20] = [
    BodyRegion::FrontBumper,
    BodyRegion::Hood,
    BodyRegion::FrontLeftFender,
    BodyRegion::FrontRightFender,
    BodyRegion::LeftDoorF,
    BodyRegion::LeftDoorR,
    BodyRegion::RightDoorF,
    BodyRegion::RightDoorR,
    BodyRegion::Roof,
    BodyRegion::RearLeftQuarter,
    BodyRegion::RearRightQuarter,
    BodyRegion::Trunk,
    BodyRegion::RearBumper,
    BodyRegion::Windshield,
    BodyRegion::RearGlass,
    BodyRegion::FrameFront,
    BodyRegion::FrameRear,
    BodyRegion::FrameLeft,
    BodyRegion::FrameRight,
    BodyRegion::Cabin,
];

/// Every component in declaration order (index = position in component arrays).
pub const ALL_COMPONENTS: [Component; 21] = [
    Component::Engine,
    Component::Radiator,
    Component::Battery,
    Component::FuelTank,
    Component::SteeringRack,
    Component::SuspensionFL,
    Component::SuspensionFR,
    Component::SuspensionRL,
    Component::SuspensionRR,
    Component::WheelFL,
    Component::WheelFR,
    Component::WheelRL,
    Component::WheelRR,
    Component::Gearbox,
    Component::Driveshaft,
    Component::BrakeFL,
    Component::BrakeFR,
    Component::BrakeRL,
    Component::BrakeRR,
    Component::Exhaust,
    Component::Lights,
];

/// Every surface in declaration order (index used in telemetry).
pub const ALL_SURFACES: [SurfaceType; 8] = [
    SurfaceType::Asphalt,
    SurfaceType::WetAsphalt,
    SurfaceType::Concrete,
    SurfaceType::Gravel,
    SurfaceType::Grass,
    SurfaceType::Dirt,
    SurfaceType::Curb,
    SurfaceType::Metal,
];

/// Serde name of a unit enum variant (e.g. `"FrontBumper"`).
pub fn variant_name<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Index of a region in [`ALL_REGIONS`].
pub fn region_index(r: BodyRegion) -> usize {
    ALL_REGIONS.iter().position(|x| *x == r).unwrap_or(0)
}

fn surface_index(s: SurfaceType) -> f32 {
    ALL_SURFACES.iter().position(|x| *x == s).unwrap_or(0) as f32
}

fn b(v: bool) -> f32 {
    if v {
        1.0
    } else {
        0.0
    }
}

/// Append one vehicle's state record ([`STATE_STRIDE`] floats).
pub fn pack_state(entity: u32, s: &VehicleState, out: &mut Vec<f32>) {
    out.push(entity as f32);
    out.extend_from_slice(&s.pos);
    out.extend_from_slice(&s.rot);
    out.extend_from_slice(&s.lin_vel);
    out.extend_from_slice(&s.ang_vel);
    out.extend_from_slice(&[s.speed_mps, s.engine_rpm, s.gear as f32, s.health]);
    for w in &s.wheel_states {
        out.extend_from_slice(&[
            w.omega,
            w.steer_rad,
            w.compression_m,
            w.slip_angle,
            w.slip_ratio,
            w.fz_n,
            b(w.contact),
            b(w.detached),
        ]);
    }
}

/// `v` if it is finite, else 0. Numbers from JS that reach the sim go through this:
/// `f32::clamp` keeps NaN, so nothing downstream should have to cope with one.
pub fn finite(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

/// Decode `set_input()`'s array:
/// `[steer, throttle, brake, handbrake, clutch, flags, tool_use]` with flags bit 0 gear_up,
/// 1 gear_down, 2 horn, 3 headlights, 4 siren. Missing trailing values are 0.
pub fn unpack_input(a: &[f32]) -> VehicleInput {
    let at = |i: usize| a.get(i).copied().map_or(0.0, finite);
    let flags = at(5) as u32;
    VehicleInput {
        steer: at(0).clamp(-1.0, 1.0),
        throttle: at(1).clamp(0.0, 1.0),
        brake: at(2).clamp(0.0, 1.0),
        handbrake: at(3).clamp(0.0, 1.0),
        clutch: at(4).clamp(0.0, 1.0),
        gear_up: flags & 1 != 0,
        gear_down: flags & 2 != 0,
        horn: flags & 4 != 0,
        headlights: flags & 8 != 0,
        siren: flags & 16 != 0,
        tool_use: at(6).clamp(0.0, 255.0) as u8,
    }
}

/// Names of the `telemetry()` channels, in order.
pub fn telemetry_names() -> Vec<String> {
    let mut n: Vec<String> = [
        "speed_mps",
        "engine_rpm",
        "gear",
        "steer_angle_rad",
        "health",
        "accel_long_g",
        "accel_lat_g",
        "accel_vert_g",
        "yaw_rate_rad_s",
        "in_throttle",
        "in_brake",
        "in_steer",
        "in_handbrake",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for w in ["fl", "fr", "rl", "rr"] {
        for f in [
            "slip_angle_rad",
            "slip_ratio",
            "load_n",
            "surface",
            "compression_m",
            "contact",
            "detached",
            "omega_rad_s",
            "fx_n",
            "fy_left_n",
        ] {
            n.push(format!("{w}_{f}"));
        }
    }
    for c in ALL_COMPONENTS {
        n.push(format!("health_{}", variant_name(&c)));
    }
    n
}

/// Telemetry record (see [`telemetry_names`]). Tire forces are the contract's
/// `WheelState::{fx_n, fy_n}` (v0.2.0): applied this tick, tire frame, Fx + forward, Fy + left.
pub fn pack_telemetry(s: &VehicleState, input: &VehicleInput, accel_local: [f32; 3]) -> Vec<f32> {
    let g = wp_contracts::GRAVITY_MPS2;
    let steer = (s.wheel_states[0].steer_rad + s.wheel_states[1].steer_rad) * 0.5;
    let mut out = vec![
        s.speed_mps,
        s.engine_rpm,
        s.gear as f32,
        steer,
        s.health,
        accel_local[1] / g,
        accel_local[0] / g,
        accel_local[2] / g,
        s.ang_vel[2],
        input.throttle,
        input.brake,
        input.steer,
        input.handbrake,
    ];
    for w in &s.wheel_states {
        out.extend_from_slice(&[
            w.slip_angle,
            w.slip_ratio,
            w.fz_n,
            surface_index(w.surface),
            w.compression_m,
            b(w.contact),
            b(w.detached),
            w.omega,
            w.fx_n,
            w.fy_n,
        ]);
    }
    for c in ALL_COMPONENTS {
        out.push(s.component_health.get(&c).copied().unwrap_or(1.0));
    }
    out
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Per-beam engineering strain `(L − L0) / L0` for current node positions.
fn beam_strains<'a>(
    lattice: &'a LatticeDef,
    nodes: &'a [[f32; 3]],
) -> impl Iterator<Item = (usize, f32)> + 'a {
    lattice.beams.iter().enumerate().map(move |(i, beam)| {
        let (a, b) = (beam.a as usize, beam.b as usize);
        let l0 = dist(lattice.nodes[a].pos_m, lattice.nodes[b].pos_m).max(1e-6);
        let l = match (nodes.get(a), nodes.get(b)) {
            (Some(pa), Some(pb)) => dist(*pa, *pb),
            _ => l0,
        };
        (i, (l - l0) / l0)
    })
}

/// Per-node stress proxy: the maximum |strain| of the beams meeting at each node.
pub fn node_strain(lattice: &LatticeDef, nodes: &[[f32; 3]]) -> Vec<f32> {
    let mut out = vec![0.0f32; lattice.nodes.len()];
    for (i, strain) in beam_strains(lattice, nodes) {
        let beam = &lattice.beams[i];
        for n in [beam.a as usize, beam.b as usize] {
            if let Some(v) = out.get_mut(n) {
                *v = v.max(strain.abs());
            }
        }
    }
    out
}

/// Deformation energy per body region (J), indexed like [`ALL_REGIONS`]: elastic energy
/// `½·(E·A/L0)·ΔL²` of each beam, split evenly between the regions of its two nodes. A proxy
/// until `wp_damage` reports absorbed (plastic) energy (contract change request TB-2).
pub fn region_energy(
    lattice: &LatticeDef,
    materials: &std::collections::BTreeMap<String, MaterialDef>,
    nodes: &[[f32; 3]],
) -> Vec<f32> {
    let mut out = vec![0.0f32; ALL_REGIONS.len()];
    for (i, strain) in beam_strains(lattice, nodes) {
        let beam = &lattice.beams[i];
        let Some(m) = materials.get(&beam.material_id) else {
            continue;
        };
        let (a, b) = (beam.a as usize, beam.b as usize);
        let l0 = dist(lattice.nodes[a].pos_m, lattice.nodes[b].pos_m).max(1e-6);
        let dl = strain * l0;
        let k = m.young_mpa * 1e6 * beam.area_m2 / l0;
        let e = 0.5 * k * dl * dl;
        out[region_index(lattice.nodes[a].region)] += 0.5 * e;
        out[region_index(lattice.nodes[b].region)] += 0.5 * e;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_record_has_documented_stride() {
        let mut out = Vec::new();
        pack_state(3, &VehicleState::default(), &mut out);
        assert_eq!(out.len(), STATE_STRIDE);
        assert_eq!(out[0], 3.0);
        assert_eq!(STATE_WHEEL_BASE + 4 * STATE_WHEEL_STRIDE, STATE_STRIDE);
    }

    #[test]
    fn telemetry_names_match_record() {
        let rec = pack_telemetry(&VehicleState::default(), &VehicleInput::default(), [0.0; 3]);
        assert_eq!(rec.len(), telemetry_names().len());
        assert_eq!(
            TELEMETRY_WHEEL_BASE + 4 * TELEMETRY_WHEEL_STRIDE + ALL_COMPONENTS.len(),
            rec.len()
        );
    }

    #[test]
    fn input_flags_decode() {
        let i = unpack_input(&[0.5, 1.0, 0.0, 0.0, 0.0, 9.0, 2.0]);
        assert!(i.gear_up && i.headlights && !i.gear_down);
        assert_eq!(i.tool_use, 2);
        assert_eq!(unpack_input(&[2.0]).steer, 1.0);
    }

    #[test]
    fn rest_lattice_has_zero_strain_and_energy() {
        let car = crate::data::load_car("debug_sedan").unwrap();
        let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
        assert!(node_strain(&car.lattice, &rest).iter().all(|s| *s < 1e-6));
        assert!(region_energy(&car.lattice, &car.materials, &rest)
            .iter()
            .all(|e| *e < 1e-3));
        let mut bent = rest.clone();
        bent[0][1] += 0.05;
        assert!(node_strain(&car.lattice, &bent)[0] > 0.01);
        let e = region_energy(&car.lattice, &car.materials, &bent);
        assert!(e[region_index(BodyRegion::RearBumper)] > 0.0);
    }

    #[test]
    fn region_and_component_tables_are_complete() {
        // Serde names are unique, so a duplicated or missing variant would show up here.
        let mut r: Vec<String> = ALL_REGIONS.iter().map(variant_name).collect();
        r.sort();
        r.dedup();
        assert_eq!(r.len(), ALL_REGIONS.len());
        let mut c: Vec<String> = ALL_COMPONENTS.iter().map(variant_name).collect();
        c.sort();
        c.dedup();
        assert_eq!(c.len(), ALL_COMPONENTS.len());
    }
}
