//! Procedural box lattice for debug cars: a regular `4 × 10 × 3` node grid (120 nodes) filling
//! the body box, with axis + face-diagonal beams, regions assigned by position, three
//! detachable groups (front bumper, rear bumper, hood) and component mounts near their real
//! locations. Prompt 03 grows this into `gen-lattice` with body profiles.

use wp_contracts::{
    BodyRegion, Component, ComponentMount, Detachable, LatticeBeam, LatticeDef, LatticeNode,
};

/// Inputs for [`generate_box_lattice`].
#[derive(Debug, Clone, PartialEq)]
pub struct BoxLatticeParams {
    /// Lattice id (also the output file stem).
    pub id: String,
    /// Body width (X), m.
    pub width_m: f32,
    /// Body length (Y), m.
    pub length_m: f32,
    /// Body height (Z), m.
    pub height_m: f32,
    /// Height of the lowest node layer above the ground, m.
    pub ground_clearance_m: f32,
    /// Wheelbase, m (axles at ±wheelbase/2), used to place suspension/wheel mounts.
    pub wheelbase_m: f32,
    /// Total lattice mass spread evenly over nodes, kg.
    pub lattice_mass_kg: f32,
}

/// Grid resolution along X (width).
pub const NX: usize = 4;
/// Grid resolution along Y (length).
pub const NY: usize = 10;
/// Grid resolution along Z (height).
pub const NZ: usize = 3;

fn index(i: usize, j: usize, k: usize) -> usize {
    (k * NY + j) * NX + i
}

fn region_for(i: usize, j: usize, k: usize) -> BodyRegion {
    let v = j as f32 / (NY - 1) as f32; // 0 = rear, 1 = front
    let left = i == 0;
    let right = i == NX - 1;
    let top = k == NZ - 1;
    let bottom = k == 0;
    if j == NY - 1 {
        return BodyRegion::FrontBumper;
    }
    if j == 0 {
        return BodyRegion::RearBumper;
    }
    if top {
        return if v > 0.7 {
            BodyRegion::Hood
        } else if v > 0.6 {
            BodyRegion::Windshield
        } else if v < 0.2 {
            BodyRegion::Trunk
        } else if v < 0.3 {
            BodyRegion::RearGlass
        } else {
            BodyRegion::Roof
        };
    }
    if left || right {
        if bottom && (0.3..=0.7).contains(&v) {
            return if left {
                BodyRegion::FrameLeft
            } else {
                BodyRegion::FrameRight
            };
        }
        return match (left, v) {
            (true, v) if v > 0.7 => BodyRegion::FrontLeftFender,
            (true, v) if v > 0.5 => BodyRegion::LeftDoorF,
            (true, v) if v >= 0.3 => BodyRegion::LeftDoorR,
            (true, _) => BodyRegion::RearLeftQuarter,
            (false, v) if v > 0.7 => BodyRegion::FrontRightFender,
            (false, v) if v > 0.5 => BodyRegion::RightDoorF,
            (false, v) if v >= 0.3 => BodyRegion::RightDoorR,
            (false, _) => BodyRegion::RearRightQuarter,
        };
    }
    if v > 0.7 {
        BodyRegion::FrameFront
    } else if v < 0.3 {
        BodyRegion::FrameRear
    } else {
        BodyRegion::Cabin
    }
}

fn is_structural(r: BodyRegion) -> bool {
    matches!(
        r,
        BodyRegion::FrameFront
            | BodyRegion::FrameRear
            | BodyRegion::FrameLeft
            | BodyRegion::FrameRight
            | BodyRegion::Cabin
    )
}

fn material_for(a: BodyRegion, b: BodyRegion) -> (&'static str, f32) {
    use BodyRegion::*;
    let either = |r: BodyRegion| a == r || b == r;
    if (either(Windshield) || either(RearGlass)) && !is_structural(a) && !is_structural(b) {
        ("glass", 1.0e-4)
    } else if either(FrontBumper) || either(RearBumper) {
        ("pp_bumper", 2.0e-4)
    } else if a == Hood && b == Hood {
        ("aluminum", 1.5e-4)
    } else if is_structural(a) && is_structural(b) {
        ("hss", 4.0e-4)
    } else {
        ("mild_steel", 1.5e-4)
    }
}

/// Group ids of the generated detachables.
pub const GROUP_FRONT_BUMPER: u16 = 0;
/// Rear bumper group id.
pub const GROUP_REAR_BUMPER: u16 = 1;
/// Hood group id.
pub const GROUP_HOOD: u16 = 2;

fn group_of(r: BodyRegion) -> Option<u16> {
    match r {
        BodyRegion::FrontBumper => Some(GROUP_FRONT_BUMPER),
        BodyRegion::RearBumper => Some(GROUP_REAR_BUMPER),
        BodyRegion::Hood => Some(GROUP_HOOD),
        _ => None,
    }
}

/// Generate the debug box lattice. Deterministic: same params → identical output.
pub fn generate_box_lattice(p: &BoxLatticeParams) -> LatticeDef {
    let n_nodes = NX * NY * NZ;
    let node_mass = p.lattice_mass_kg / n_nodes as f32;
    let mut nodes = Vec::with_capacity(n_nodes);
    for k in 0..NZ {
        for j in 0..NY {
            for i in 0..NX {
                let x = -p.width_m * 0.5 + p.width_m * i as f32 / (NX - 1) as f32;
                let y = -p.length_m * 0.5 + p.length_m * j as f32 / (NY - 1) as f32;
                let z = p.ground_clearance_m
                    + (p.height_m - p.ground_clearance_m) * k as f32 / (NZ - 1) as f32;
                let region = region_for(i, j, k);
                let interior_x = i > 0 && i < NX - 1;
                nodes.push(LatticeNode {
                    id: index(i, j, k) as u16,
                    pos_m: [x, y, z],
                    mass_kg: node_mass,
                    pinned_to_chassis: k == 0 && interior_x && region == BodyRegion::Cabin,
                    region,
                });
            }
        }
    }

    // Neighbour offsets: 3 axes + 6 face diagonals (each undirected edge emitted once).
    let offsets: [(i32, i32, i32); 9] = [
        (1, 0, 0),
        (0, 1, 0),
        (0, 0, 1),
        (1, 1, 0),
        (1, -1, 0),
        (1, 0, 1),
        (1, 0, -1),
        (0, 1, 1),
        (0, 1, -1),
    ];
    let mut beams = Vec::new();
    for k in 0..NZ {
        for j in 0..NY {
            for i in 0..NX {
                for (di, dj, dk) in offsets {
                    let (ni, nj, nk) = (i as i32 + di, j as i32 + dj, k as i32 + dk);
                    if ni < 0
                        || nj < 0
                        || nk < 0
                        || ni >= NX as i32
                        || nj >= NY as i32
                        || nk >= NZ as i32
                    {
                        continue;
                    }
                    let a = index(i, j, k);
                    let b = index(ni as usize, nj as usize, nk as usize);
                    let (ra, rb) = (nodes[a].region, nodes[b].region);
                    let (material, area) = material_for(ra, rb);
                    // Beams crossing a detachable part's boundary are its attachment beams.
                    let detach_group = match (group_of(ra), group_of(rb)) {
                        (Some(g), None) | (None, Some(g)) => Some(g),
                        _ => None,
                    };
                    beams.push(LatticeBeam {
                        a: a as u16,
                        b: b as u16,
                        material_id: material.to_string(),
                        area_m2: area,
                        breakable: true,
                        detach_group,
                    });
                }
            }
        }
    }

    let detachables = vec![
        Detachable {
            group: GROUP_FRONT_BUMPER,
            region: BodyRegion::FrontBumper,
            hinge_nodes: [index(0, NY - 1, 1) as u16, index(NX - 1, NY - 1, 1) as u16],
            break_impulse_ns: 3000.0,
            mass_kg: 6.0,
        },
        Detachable {
            group: GROUP_REAR_BUMPER,
            region: BodyRegion::RearBumper,
            hinge_nodes: [index(0, 0, 1) as u16, index(NX - 1, 0, 1) as u16],
            break_impulse_ns: 3000.0,
            mass_kg: 6.0,
        },
        Detachable {
            group: GROUP_HOOD,
            region: BodyRegion::Hood,
            hinge_nodes: [index(0, 7, NZ - 1) as u16, index(NX - 1, 7, NZ - 1) as u16],
            break_impulse_ns: 5000.0,
            mass_kg: 15.0,
        },
    ];

    // Row index nearest a chassis-space y coordinate.
    let row = |y: f32| -> usize {
        let t = (y + p.length_m * 0.5) / p.length_m * (NY - 1) as f32;
        (t.round().max(1.0) as usize).min(NY - 2)
    };
    let front = row(p.wheelbase_m * 0.5);
    let rear = row(-p.wheelbase_m * 0.5);
    let mount = |kind: Component, ids: Vec<usize>, fail: f32, total: f32| ComponentMount {
        kind,
        node_ids: ids.into_iter().map(|i| i as u16).collect(),
        intrusion_fail_m: fail,
        intrusion_total_m: total,
    };
    let (l, r) = (0, NX - 1);
    let components = vec![
        mount(
            Component::Engine,
            vec![
                index(1, 7, 1),
                index(2, 7, 1),
                index(1, 8, 1),
                index(2, 8, 1),
            ],
            0.25,
            0.40,
        ),
        mount(
            Component::Radiator,
            vec![index(1, NY - 2, 1), index(2, NY - 2, 1)],
            0.08,
            0.15,
        ),
        mount(Component::Battery, vec![index(r, 8, 1)], 0.10, 0.20),
        mount(
            Component::FuelTank,
            vec![index(1, 2, 0), index(2, 2, 0)],
            0.15,
            0.30,
        ),
        mount(
            Component::SteeringRack,
            vec![index(1, front, 0), index(2, front, 0)],
            0.06,
            0.15,
        ),
        mount(
            Component::Gearbox,
            vec![index(1, 6, 0), index(2, 6, 0)],
            0.15,
            0.30,
        ),
        mount(
            Component::Driveshaft,
            vec![index(1, 4, 0), index(2, 4, 0)],
            0.10,
            0.25,
        ),
        mount(Component::Exhaust, vec![index(2, 1, 0)], 0.10, 0.25),
        mount(
            Component::Lights,
            vec![
                index(l, NY - 1, 1),
                index(r, NY - 1, 1),
                index(l, 0, 1),
                index(r, 0, 1),
            ],
            0.03,
            0.08,
        ),
        mount(
            Component::SuspensionFL,
            vec![index(l, front, 0), index(l, front, 1)],
            0.05,
            0.12,
        ),
        mount(
            Component::SuspensionFR,
            vec![index(r, front, 0), index(r, front, 1)],
            0.05,
            0.12,
        ),
        mount(
            Component::SuspensionRL,
            vec![index(l, rear, 0), index(l, rear, 1)],
            0.05,
            0.12,
        ),
        mount(
            Component::SuspensionRR,
            vec![index(r, rear, 0), index(r, rear, 1)],
            0.05,
            0.12,
        ),
        mount(Component::WheelFL, vec![index(l, front, 0)], 0.12, 0.20),
        mount(Component::WheelFR, vec![index(r, front, 0)], 0.12, 0.20),
        mount(Component::WheelRL, vec![index(l, rear, 0)], 0.12, 0.20),
        mount(Component::WheelRR, vec![index(r, rear, 0)], 0.12, 0.20),
        mount(Component::BrakeFL, vec![index(l, front, 0)], 0.10, 0.20),
        mount(Component::BrakeFR, vec![index(r, front, 0)], 0.10, 0.20),
        mount(Component::BrakeRL, vec![index(l, rear, 0)], 0.10, 0.20),
        mount(Component::BrakeRR, vec![index(r, rear, 0)], 0.10, 0.20),
    ];

    LatticeDef {
        id: p.id.clone(),
        nodes,
        beams,
        detachables,
        components,
        skin_binding: Vec::new(),
    }
}

/// Serialize a lattice as JSON with one node/beam/etc. per line (diff-friendly, compact).
/// Parses back to an identical [`LatticeDef`].
pub fn lattice_to_json_lines(l: &LatticeDef) -> String {
    fn list<T: serde::Serialize>(out: &mut String, key: &str, items: &[T], last: bool) {
        out.push_str(&format!("  \"{key}\": ["));
        for (i, item) in items.iter().enumerate() {
            out.push_str(if i == 0 { "\n    " } else { ",\n    " });
            out.push_str(&serde_json::to_string(item).expect("serialize lattice item"));
        }
        out.push_str(if items.is_empty() { "]" } else { "\n  ]" });
        out.push_str(if last { "\n" } else { ",\n" });
    }
    let mut out = String::from("{\n");
    out.push_str(&format!(
        "  \"id\": {},\n",
        serde_json::to_string(&l.id).expect("serialize id")
    ));
    list(&mut out, "nodes", &l.nodes, false);
    list(&mut out, "beams", &l.beams, false);
    list(&mut out, "detachables", &l.detachables, false);
    list(&mut out, "components", &l.components, false);
    list(&mut out, "skin_binding", &l.skin_binding, true);
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> BoxLatticeParams {
        BoxLatticeParams {
            id: "test".into(),
            width_m: 1.8,
            length_m: 4.7,
            height_m: 1.45,
            ground_clearance_m: 0.15,
            wheelbase_m: 2.7,
            lattice_mass_kg: 350.0,
        }
    }

    #[test]
    fn generates_valid_120_node_lattice() {
        let l = generate_box_lattice(&params());
        assert_eq!(l.nodes.len(), 120);
        wp_contracts::validate_lattice_def(&l).expect("valid");
        assert!(l.nodes.iter().any(|n| n.pinned_to_chassis));
        assert!(l.beams.iter().any(|b| b.detach_group == Some(GROUP_HOOD)));
        assert!(l.beams.iter().any(|b| b.material_id == "glass"));
    }

    #[test]
    fn json_lines_round_trip() {
        let l = generate_box_lattice(&params());
        let text = lattice_to_json_lines(&l);
        let back = wp_contracts::parse_lattice_def("test", &text).expect("parses");
        assert_eq!(back, l);
    }

    #[test]
    fn deterministic() {
        assert_eq!(
            generate_box_lattice(&params()),
            generate_box_lattice(&params())
        );
    }

    #[test]
    fn checked_in_debug_lattices_are_current() {
        // Regenerate with the same params as `just lattices` and compare with data/.
        for (id, w, l, h, wb, m) in [
            ("debug_sedan", 1.80, 4.70, 1.45, 2.70, 350.0),
            ("debug_coupe", 1.85, 4.50, 1.32, 2.65, 375.0),
        ] {
            let p = BoxLatticeParams {
                id: id.into(),
                width_m: w,
                length_m: l,
                height_m: h,
                ground_clearance_m: 0.15,
                wheelbase_m: wb,
                lattice_mass_kg: m,
            };
            let path = format!(
                "{}/../../data/lattices/{id}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            let on_disk = wp_contracts::load_lattice_def(&path).expect("lattice loads");
            assert_eq!(
                on_disk,
                generate_box_lattice(&p),
                "{id} is stale; run `just lattices`"
            );
        }
    }
}
