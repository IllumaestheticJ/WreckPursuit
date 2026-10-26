//! Data loading and cross-reference validation.
//!
//! Layout under a data root (normally `data/`):
//! `cars/<id>.json`, `tires/<id>.json`, `lattices/<id>.json`, `materials/<id>.json`,
//! `modes/{arcade,sim}.toml`. Every file's `id` must equal its file stem.
//!
//! Filesystem loaders ([`load_car`] etc.) are for native code. The browser embeds file
//! contents and uses the `parse_*` functions plus [`resolve_car`], which perform the same
//! validation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::{DriveLayout, LatticeDef, MaterialDef, ModeTable, TireDef, VehicleDef};

/// Errors produced while loading or validating data.
#[derive(Debug, Error)]
pub enum LoadError {
    /// File could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// Offending path.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// JSON did not match the schema.
    #[error("invalid JSON in {name}: {source}")]
    Json {
        /// File name or label.
        name: String,
        /// Underlying error.
        source: serde_json::Error,
    },
    /// TOML did not match the schema.
    #[error("invalid TOML in {name}: {source}")]
    Toml {
        /// File name or label.
        name: String,
        /// Underlying error.
        source: toml::de::Error,
    },
    /// A file's `id` differs from its file stem.
    #[error("{path}: id {found:?} does not match file stem {expected:?}")]
    IdMismatch {
        /// Offending path.
        path: PathBuf,
        /// File stem.
        expected: String,
        /// Id inside the file.
        found: String,
    },
    /// A car references a tire that does not exist.
    #[error("car {car:?} references missing tire {tire_id:?}")]
    MissingTire {
        /// Car id.
        car: String,
        /// Missing tire id.
        tire_id: String,
    },
    /// A car references a lattice that does not exist.
    #[error("car {car:?} references missing lattice {lattice_id:?}")]
    MissingLattice {
        /// Car id.
        car: String,
        /// Missing lattice id.
        lattice_id: String,
    },
    /// A lattice references a material that does not exist.
    #[error("lattice {lattice:?} references missing material {material_id:?}")]
    MissingMaterial {
        /// Lattice id.
        lattice: String,
        /// Missing material id.
        material_id: String,
    },
    /// A value is out of range or structurally inconsistent.
    #[error("{what}: {reason}")]
    Invalid {
        /// Which object (e.g. `car debug_sedan`).
        what: String,
        /// What is wrong.
        reason: String,
    },
}

/// A vehicle with every cross-referenced definition resolved and validated.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedCar {
    /// The vehicle definition.
    pub def: VehicleDef,
    /// Tires used by the car, keyed by tire id.
    pub tires: BTreeMap<String, TireDef>,
    /// The car's damage lattice.
    pub lattice: LatticeDef,
    /// Materials used by the lattice, keyed by material id.
    pub materials: BTreeMap<String, MaterialDef>,
}

fn invalid(what: impl Into<String>, reason: impl Into<String>) -> LoadError {
    LoadError::Invalid {
        what: what.into(),
        reason: reason.into(),
    }
}

fn parse_json<T: DeserializeOwned>(name: &str, text: &str) -> Result<T, LoadError> {
    serde_json::from_str(text).map_err(|source| LoadError::Json {
        name: name.to_string(),
        source,
    })
}

/// Parse and validate a `VehicleDef` from JSON text. `name` labels errors.
pub fn parse_vehicle_def(name: &str, text: &str) -> Result<VehicleDef, LoadError> {
    let def: VehicleDef = parse_json(name, text)?;
    validate_vehicle_def(&def)?;
    Ok(def)
}

/// Parse and validate a `TireDef` from JSON text.
pub fn parse_tire_def(name: &str, text: &str) -> Result<TireDef, LoadError> {
    let def: TireDef = parse_json(name, text)?;
    validate_tire_def(&def)?;
    Ok(def)
}

/// Parse and validate a `MaterialDef` from JSON text.
pub fn parse_material_def(name: &str, text: &str) -> Result<MaterialDef, LoadError> {
    let def: MaterialDef = parse_json(name, text)?;
    validate_material_def(&def)?;
    Ok(def)
}

/// Parse and validate a `LatticeDef` from JSON text.
pub fn parse_lattice_def(name: &str, text: &str) -> Result<LatticeDef, LoadError> {
    let def: LatticeDef = parse_json(name, text)?;
    validate_lattice_def(&def)?;
    Ok(def)
}

/// Parse and validate a `ModeTable` from TOML text.
pub fn parse_mode_table(name: &str, text: &str) -> Result<ModeTable, LoadError> {
    let table: ModeTable = toml::from_str(text).map_err(|source| LoadError::Toml {
        name: name.to_string(),
        source,
    })?;
    validate_mode_table(&table)?;
    Ok(table)
}

fn read(path: &Path) -> Result<String, LoadError> {
    std::fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn check_stem(path: &Path, id: &str) -> Result<(), LoadError> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if stem != id {
        return Err(LoadError::IdMismatch {
            path: path.to_path_buf(),
            expected: stem.to_string(),
            found: id.to_string(),
        });
    }
    Ok(())
}

/// Load and validate a vehicle definition file (no cross-references resolved).
pub fn load_vehicle_def(path: impl AsRef<Path>) -> Result<VehicleDef, LoadError> {
    let path = path.as_ref();
    let def = parse_vehicle_def(&path.display().to_string(), &read(path)?)?;
    check_stem(path, &def.id)?;
    Ok(def)
}

/// Load and validate a tire file.
pub fn load_tire_def(path: impl AsRef<Path>) -> Result<TireDef, LoadError> {
    let path = path.as_ref();
    let def = parse_tire_def(&path.display().to_string(), &read(path)?)?;
    check_stem(path, &def.id)?;
    Ok(def)
}

/// Load and validate a material file.
pub fn load_material_def(path: impl AsRef<Path>) -> Result<MaterialDef, LoadError> {
    let path = path.as_ref();
    let def = parse_material_def(&path.display().to_string(), &read(path)?)?;
    check_stem(path, &def.id)?;
    Ok(def)
}

/// Load and validate a lattice file.
pub fn load_lattice_def(path: impl AsRef<Path>) -> Result<LatticeDef, LoadError> {
    let path = path.as_ref();
    let def = parse_lattice_def(&path.display().to_string(), &read(path)?)?;
    check_stem(path, &def.id)?;
    Ok(def)
}

/// Load and validate a mode table (`data/modes/*.toml`).
pub fn load_mode_table(path: impl AsRef<Path>) -> Result<ModeTable, LoadError> {
    let path = path.as_ref();
    parse_mode_table(&path.display().to_string(), &read(path)?)
}

/// Load a car from `data/cars/<id>.json`, resolving its tires, lattice and the lattice's
/// materials from sibling folders of the same data root (`<path>/../..`), and validate every
/// cross-reference.
pub fn load_car(path: impl AsRef<Path>) -> Result<LoadedCar, LoadError> {
    let path = path.as_ref();
    let def = load_vehicle_def(path)?;
    let root = path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid(path.display().to_string(), "car file has no data root"))?;

    let mut tires = BTreeMap::new();
    for wheel in &def.wheels {
        if tires.contains_key(&wheel.tire_id) {
            continue;
        }
        let tire_path = root.join("tires").join(format!("{}.json", wheel.tire_id));
        if !tire_path.exists() {
            return Err(LoadError::MissingTire {
                car: def.id.clone(),
                tire_id: wheel.tire_id.clone(),
            });
        }
        tires.insert(wheel.tire_id.clone(), load_tire_def(&tire_path)?);
    }

    let lattice_path = root
        .join("lattices")
        .join(format!("{}.json", def.lattice_id));
    if !lattice_path.exists() {
        return Err(LoadError::MissingLattice {
            car: def.id.clone(),
            lattice_id: def.lattice_id.clone(),
        });
    }
    let lattice = load_lattice_def(&lattice_path)?;

    let mut materials = BTreeMap::new();
    for beam in &lattice.beams {
        if materials.contains_key(&beam.material_id) {
            continue;
        }
        let mat_path = root
            .join("materials")
            .join(format!("{}.json", beam.material_id));
        if !mat_path.exists() {
            return Err(LoadError::MissingMaterial {
                lattice: lattice.id.clone(),
                material_id: beam.material_id.clone(),
            });
        }
        materials.insert(beam.material_id.clone(), load_material_def(&mat_path)?);
    }

    resolve_car(def, &tires, &[lattice], &materials)
}

/// Resolve and validate a car against in-memory databases (used by the browser build).
/// Only the tires/materials the car actually uses are copied into the result.
pub fn resolve_car(
    def: VehicleDef,
    tires: &BTreeMap<String, TireDef>,
    lattices: &[LatticeDef],
    materials: &BTreeMap<String, MaterialDef>,
) -> Result<LoadedCar, LoadError> {
    validate_vehicle_def(&def)?;
    let mut used_tires = BTreeMap::new();
    for wheel in &def.wheels {
        let tire = tires
            .get(&wheel.tire_id)
            .ok_or_else(|| LoadError::MissingTire {
                car: def.id.clone(),
                tire_id: wheel.tire_id.clone(),
            })?;
        used_tires.insert(wheel.tire_id.clone(), tire.clone());
    }
    let lattice = lattices
        .iter()
        .find(|l| l.id == def.lattice_id)
        .ok_or_else(|| LoadError::MissingLattice {
            car: def.id.clone(),
            lattice_id: def.lattice_id.clone(),
        })?
        .clone();
    validate_lattice_def(&lattice)?;
    let mut used_materials = BTreeMap::new();
    for beam in &lattice.beams {
        let mat = materials
            .get(&beam.material_id)
            .ok_or_else(|| LoadError::MissingMaterial {
                lattice: lattice.id.clone(),
                material_id: beam.material_id.clone(),
            })?;
        used_materials.insert(beam.material_id.clone(), mat.clone());
    }
    Ok(LoadedCar {
        def,
        tires: used_tires,
        lattice,
        materials: used_materials,
    })
}

fn check_range(what: &str, field: &str, v: f32, lo: f32, hi: f32) -> Result<(), LoadError> {
    if !(v.is_finite() && v >= lo && v <= hi) {
        return Err(invalid(
            what,
            format!("{field} = {v} is outside [{lo}, {hi}]"),
        ));
    }
    Ok(())
}

fn check_positive(what: &str, field: &str, v: f32) -> Result<(), LoadError> {
    if !(v.is_finite() && v > 0.0) {
        return Err(invalid(what, format!("{field} = {v} must be > 0")));
    }
    Ok(())
}

/// Validate value ranges and internal consistency of a vehicle definition.
pub fn validate_vehicle_def(def: &VehicleDef) -> Result<(), LoadError> {
    let what = format!("car {}", def.id);
    let w = what.as_str();
    for (i, s) in def.body_size_m.iter().enumerate() {
        check_positive(w, &format!("body_size_m[{i}]"), *s)?;
    }
    check_positive(w, "mass_kg", def.mass_kg)?;
    check_range(w, "front_weight_frac", def.front_weight_frac, 0.0, 1.0)?;
    check_positive(w, "wheelbase_m", def.wheelbase_m)?;
    check_positive(w, "track_front_m", def.track_front_m)?;
    check_positive(w, "track_rear_m", def.track_rear_m)?;
    check_positive(w, "cg_height_m", def.cg_height_m)?;
    for (i, v) in def.inertia_kgm2.iter().enumerate() {
        check_positive(w, &format!("inertia_kgm2[{i}]"), *v)?;
    }
    check_range(w, "drag_cd_a_m2", def.drag_cd_a_m2, 0.0, 10.0)?;
    check_range(w, "durability_class", def.durability_class as f32, 1.0, 5.0)?;

    let e = &def.engine;
    if e.torque_curve_nm.len() < 2 {
        return Err(invalid(w, "engine.torque_curve_nm needs at least 2 points"));
    }
    for pair in e.torque_curve_nm.windows(2) {
        if pair[1].0 <= pair[0].0 {
            return Err(invalid(
                w,
                "engine.torque_curve_nm rpm must be strictly increasing",
            ));
        }
    }
    if e.torque_curve_nm
        .iter()
        .any(|(_, nm)| !nm.is_finite() || *nm < 0.0)
    {
        return Err(invalid(w, "engine.torque_curve_nm torque must be ≥ 0"));
    }
    if !(e.idle_rpm > 0.0 && e.idle_rpm < e.redline_rpm && e.redline_rpm <= e.rev_limit_rpm) {
        return Err(invalid(
            w,
            "engine rpm must satisfy 0 < idle < redline <= rev_limit",
        ));
    }
    check_positive(w, "engine.inertia_kgm2", e.inertia_kgm2)?;

    let d = &def.drivetrain;
    if d.gear_ratios.is_empty() || d.gear_ratios.iter().any(|r| !r.is_finite() || *r <= 0.0) {
        return Err(invalid(
            w,
            "drivetrain.gear_ratios must be non-empty and > 0",
        ));
    }
    check_positive(w, "drivetrain.final_drive", d.final_drive)?;
    check_positive(w, "drivetrain.reverse_ratio", d.reverse_ratio)?;
    check_positive(w, "drivetrain.clutch_max_nm", d.clutch_max_nm)?;
    if let DriveLayout::Awd { front_split } = d.layout {
        check_range(w, "drivetrain.layout.front_split", front_split, 0.0, 1.0)?;
    }

    for (i, wheel) in def.wheels.iter().enumerate() {
        check_positive(w, &format!("wheels[{i}].radius_m"), wheel.radius_m)?;
        check_positive(w, &format!("wheels[{i}].width_m"), wheel.width_m)?;
        check_positive(w, &format!("wheels[{i}].mass_kg"), wheel.mass_kg)?;
        check_range(
            w,
            &format!("wheels[{i}].ackermann_frac"),
            wheel.ackermann_frac,
            0.0,
            1.0,
        )?;
        let left = i % 2 == 0;
        if (wheel.position_m[0] < 0.0) != left {
            return Err(invalid(
                w,
                format!("wheels[{i}] is on the wrong side; order is FL, FR, RL, RR (+X right)"),
            ));
        }
    }
    if def.wheels[0].position_m[1] <= def.wheels[2].position_m[1] {
        return Err(invalid(
            w,
            "front wheels must be ahead (+Y) of rear wheels; order is FL, FR, RL, RR",
        ));
    }
    for (i, s) in def.suspension.iter().enumerate() {
        check_positive(
            w,
            &format!("suspension[{i}].spring_n_per_m"),
            s.spring_n_per_m,
        )?;
        check_positive(w, &format!("suspension[{i}].travel_m"), s.travel_m)?;
    }
    check_range(w, "brakes.bias_front", def.brakes.bias_front, 0.0, 1.0)?;
    check_positive(w, "steering.ratio", def.steering.ratio)?;
    Ok(())
}

/// Validate a tire definition.
pub fn validate_tire_def(def: &TireDef) -> Result<(), LoadError> {
    let what = format!("tire {}", def.id);
    let w = what.as_str();
    for (axis, mf) in [
        ("lateral", &def.lateral),
        ("longitudinal", &def.longitudinal),
    ] {
        check_positive(w, &format!("{axis}.b"), mf.b)?;
        check_positive(w, &format!("{axis}.c"), mf.c)?;
        check_range(w, &format!("{axis}.d"), mf.d, 0.0, 5.0)?;
    }
    check_range(w, "load_sensitivity", def.load_sensitivity, 0.0, 1.0)?;
    check_positive(w, "fz0_n", def.fz0_n)?;
    check_positive(w, "relaxation_length_m", def.relaxation_length_m)?;
    for (s, m) in &def.surface_multipliers {
        check_range(w, &format!("surface_multipliers[{s:?}]"), *m, 0.0, 2.0)?;
    }
    Ok(())
}

/// Validate a material definition.
pub fn validate_material_def(def: &MaterialDef) -> Result<(), LoadError> {
    let what = format!("material {}", def.id);
    let w = what.as_str();
    check_positive(w, "density_kg_m3", def.density_kg_m3)?;
    check_positive(w, "young_mpa", def.young_mpa)?;
    check_positive(w, "yield_mpa", def.yield_mpa)?;
    check_range(
        w,
        "plastic_modulus_mpa",
        def.plastic_modulus_mpa,
        0.0,
        def.young_mpa,
    )?;
    check_positive(w, "failure_strain", def.failure_strain)?;
    check_range(w, "restitution", def.restitution, 0.0, 1.0)?;
    check_range(w, "friction", def.friction, 0.0, 2.0)?;
    Ok(())
}

/// Validate a lattice's internal references (node ids, beams, groups, mounts, skin).
pub fn validate_lattice_def(def: &LatticeDef) -> Result<(), LoadError> {
    let what = format!("lattice {}", def.id);
    let w = what.as_str();
    let n = def.nodes.len();
    if n == 0 || n > u16::MAX as usize {
        return Err(invalid(w, format!("node count {n} must be in 1..=65535")));
    }
    for (i, node) in def.nodes.iter().enumerate() {
        if node.id as usize != i {
            return Err(invalid(
                w,
                format!("nodes[{i}].id = {} must equal index", node.id),
            ));
        }
        check_positive(w, &format!("nodes[{i}].mass_kg"), node.mass_kg)?;
    }
    let node_ok = |id: u16| (id as usize) < n;
    let groups: Vec<u16> = def.detachables.iter().map(|d| d.group).collect();
    for (i, g) in groups.iter().enumerate() {
        if groups[..i].contains(g) {
            return Err(invalid(w, format!("detachable group {g} defined twice")));
        }
    }
    for (i, beam) in def.beams.iter().enumerate() {
        if !node_ok(beam.a) || !node_ok(beam.b) || beam.a == beam.b {
            return Err(invalid(w, format!("beams[{i}] has invalid endpoints")));
        }
        check_positive(w, &format!("beams[{i}].area_m2"), beam.area_m2)?;
        if let Some(g) = beam.detach_group {
            if !groups.contains(&g) {
                return Err(invalid(
                    w,
                    format!("beams[{i}] references unknown detach_group {g}"),
                ));
            }
        }
    }
    for d in &def.detachables {
        if !d.hinge_nodes.iter().all(|id| node_ok(*id)) {
            return Err(invalid(
                w,
                format!("detachable {} has invalid hinge nodes", d.group),
            ));
        }
        check_positive(w, "detachable.break_impulse_ns", d.break_impulse_ns)?;
    }
    for c in &def.components {
        if c.node_ids.is_empty() || !c.node_ids.iter().all(|id| node_ok(*id)) {
            return Err(invalid(
                w,
                format!("component {:?} has invalid node ids", c.kind),
            ));
        }
        check_positive(w, "component.intrusion_fail_m", c.intrusion_fail_m)?;
        check_positive(w, "component.intrusion_total_m", c.intrusion_total_m)?;
    }
    for s in &def.skin_binding {
        if !s.node_ids.iter().all(|id| node_ok(*id)) {
            return Err(invalid(
                w,
                format!("skin vertex {} has invalid node ids", s.vertex_index),
            ));
        }
        let sum: f32 = s.weights.iter().sum();
        if (sum - 1.0).abs() > 1e-3 {
            return Err(invalid(
                w,
                format!("skin vertex {} weights sum to {sum}", s.vertex_index),
            ));
        }
    }
    Ok(())
}

/// Validate a mode table.
pub fn validate_mode_table(t: &ModeTable) -> Result<(), LoadError> {
    let what = format!("mode table {:?}", t.mode);
    let w = what.as_str();
    for (field, v) in [
        ("impulse_to_lattice_scale", t.impulse_to_lattice_scale),
        ("plasticity_scale", t.plasticity_scale),
        ("beam_break_scale", t.beam_break_scale),
        ("component_damage_scale", t.component_damage_scale),
        (
            "component_fail_threshold_scale",
            t.component_fail_threshold_scale,
        ),
        (
            "cabin_intrusion_total_loss_m",
            t.cabin_intrusion_total_loss_m,
        ),
        ("repair_cost_scale", t.repair_cost_scale),
        ("wheel_detach_scale", t.wheel_detach_scale),
        ("racer_damage_scale", t.racer_damage_scale),
        ("ai_racer_damage_mult", t.ai_racer_damage_mult),
        ("police_damage_scale", t.police_damage_scale),
    ] {
        check_positive(w, field, v)?;
    }
    check_range(w, "total_loss_health", t.total_loss_health, 0.0, 1.0)?;
    Ok(())
}
