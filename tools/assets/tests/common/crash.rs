//! Frontal crash runner shared by `tests/assets.rs` and `examples/crumple_matrix.rs`: the real
//! sim (Rapier contacts, lattice, panels) through simcli, with the browser's crumple memory
//! updated every frame (`frame_ticks` physics ticks: 4 at 60 fps, 8 at 30 fps).
#![allow(dead_code)]

use car_assets::crumple::{render_part, Pose};
use car_assets::mesh::CarMesh;
use car_assets::repo_root;
use car_assets::skin::FoldMemory;
use wp_contracts::{DamageSim, LatticeDef};

/// What a frontal hit leaves: rest and final nodes, final panel states and the crumple
/// memory of every part with a fold binding (by part index).
pub struct Crash {
    /// Rest node positions, m.
    pub rest: Vec<[f32; 3]>,
    /// Final node positions, m.
    pub now: Vec<[f32; 3]>,
    /// Final panel states.
    pub states: Vec<wp_damage::PanelStatus>,
    /// Crumple memory per part index (empty for parts without a fold).
    pub memory: Vec<FoldMemory>,
}

/// `car` drives at `kmh` into a full-width rigid barrier (`target = "barrier"`) or head-on
/// into a parked sedan (`"car"`), in `mode` (`Arcade` / `Simulation`), for `duration_s`; the
/// renderer's crumple memory sees every `frame_ticks`-th tick.
#[allow(clippy::too_many_arguments)]
pub fn frontal(
    car: &str,
    lattice: &LatticeDef,
    mesh: &CarMesh,
    kmh: f32,
    target: &str,
    mode: &str,
    duration_s: f32,
    frame_ticks: u64,
) -> Crash {
    let v = kmh / 3.6;
    let (barriers, other) = match target {
        "barrier" => ("[(pos: (0.0, 8.0, 0.75), size: (6.0, 1.0, 1.5))]", ""),
        _ => (
            "[]",
            ", (id: \"parked\", car: \"debug_sedan\", pos: (0.0, 12.0, 0.0), yaw: 3.14159265)",
        ),
    };
    let ron = format!(
        "(name: \"crumple\", seed: 7, mode: {mode}, assists: ai, panels: true,
          barriers: {barriers},
          entities: [(id: \"car\", car: \"{car}\", speed_mps: {v}, driver: cruise(target_mps: {v}, until_s: 0.9)){other}],
          duration_s: {duration_s}, hash_every: 240)"
    );
    run(&ron, lattice, mesh, frame_ticks)
}

/// `car` stands parked and a sedan drives into its rear (full overlap) at `kmh`, in `mode`.
pub fn rear_ended(
    car: &str,
    lattice: &LatticeDef,
    mesh: &CarMesh,
    kmh: f32,
    mode: &str,
    frame_ticks: u64,
) -> Crash {
    let v = kmh / 3.6;
    let ron = format!(
        "(name: \"crumple_rear\", seed: 7, mode: {mode}, assists: ai, panels: true,
          entities: [(id: \"car\", car: \"{car}\"),
                     (id: \"striker\", car: \"debug_sedan\", pos: (0.0, -12.0, 0.0), speed_mps: {v}, driver: cruise(target_mps: {v}, until_s: 0.9))],
          duration_s: 2.0, hash_every: 240)"
    );
    run(&ron, lattice, mesh, frame_ticks)
}

/// Run scenario `ron` (RON text) and collect the first entity's [`Crash`].
pub fn run(ron: &str, lattice: &LatticeDef, mesh: &CarMesh, frame_ticks: u64) -> Crash {
    let sc = simcli::Scenario::from_ron("crumple.ron", ron).unwrap();
    let data = simcli::FsData {
        root: repo_root().join("data"),
    };
    let mut out = Crash {
        rest: Vec::new(),
        now: Vec::new(),
        states: Vec::new(),
        memory: vec![FoldMemory::new(); mesh.parts.len()],
    };
    let mut hook = |t: &simcli::TickView| -> Result<(), String> {
        let d = &t.sim.vehicles()[0].damage;
        if out.rest.is_empty() {
            out.rest = (0..d.lattice().nodes.len())
                .map(|i| d.rest_position(i))
                .collect();
        }
        if !t.sim.tick().is_multiple_of(frame_ticks) {
            return Ok(());
        }
        out.now = d.node_positions().to_vec();
        out.states = d.panel_states();
        let pose = Pose {
            lattice,
            rest: &out.rest,
            nodes: &out.now,
            states: &out.states,
        };
        for (p, m) in mesh.parts.iter().zip(out.memory.iter_mut()) {
            if p.fold.is_some() {
                render_part(p, &pose, Some(m));
            }
        }
        Ok(())
    };
    simcli::run_with_hook(&sc, &data, &simcli::RunOptions::default(), &mut hook).unwrap();
    out
}
