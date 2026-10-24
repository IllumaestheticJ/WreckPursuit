//! Data access for the runner: the same loading code path for the filesystem (native) and an
//! in-memory bundle (WASM, where there is no filesystem).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wp_contracts::{DamageMode, LoadError, LoadedCar, ModeTable};

use crate::RunError;

/// Somewhere data files (`cars/x.json`, `modes/arcade.toml`, …) can be read from.
pub trait DataSource {
    /// Read a file by its path relative to the data root, using `/` separators.
    fn read(&self, rel: &str) -> Result<String, RunError>;

    /// Read a binary file (default: the text from [`DataSource::read`] as bytes).
    fn read_bytes(&self, rel: &str) -> Result<Vec<u8>, RunError> {
        self.read(rel).map(String::into_bytes)
    }
}

/// A data root on disk (normally the repo's `data/`).
#[derive(Debug, Clone)]
pub struct FsData {
    /// Root directory.
    pub root: PathBuf,
}

impl FsData {
    /// Data root at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl DataSource for FsData {
    fn read(&self, rel: &str) -> Result<String, RunError> {
        let path = self.root.join(rel);
        std::fs::read_to_string(&path).map_err(|source| {
            RunError::Load(LoadError::Io {
                path: path.clone(),
                source,
            })
        })
    }

    fn read_bytes(&self, rel: &str) -> Result<Vec<u8>, RunError> {
        let path = self.root.join(rel);
        std::fs::read(&path).map_err(|source| {
            RunError::Load(LoadError::Io {
                path: path.clone(),
                source,
            })
        })
    }
}

/// An in-memory data root: relative path → file text. Serialized as a JSON object for the WASM
/// determinism runner.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemData {
    /// Files by relative path.
    pub files: BTreeMap<String, String>,
}

impl MemData {
    /// Snapshot every `.json`/`.toml` file under `root` (recursively).
    pub fn from_dir(root: &Path) -> Result<Self, RunError> {
        fn walk(
            root: &Path,
            dir: &Path,
            out: &mut BTreeMap<String, String>,
        ) -> std::io::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let p = entry?.path();
                if p.is_dir() {
                    walk(root, &p, out)?;
                } else if matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("json" | "toml")
                ) {
                    let rel = p.strip_prefix(root).expect("under root");
                    let rel = rel
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    out.insert(rel, std::fs::read_to_string(&p)?);
                }
            }
            Ok(())
        }
        let mut files = BTreeMap::new();
        walk(root, root, &mut files).map_err(|source| {
            RunError::Load(LoadError::Io {
                path: root.to_path_buf(),
                source,
            })
        })?;
        Ok(Self { files })
    }
}

impl DataSource for MemData {
    fn read(&self, rel: &str) -> Result<String, RunError> {
        self.files.get(rel).cloned().ok_or_else(|| {
            RunError::Load(LoadError::Io {
                path: PathBuf::from(rel),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "not in data bundle"),
            })
        })
    }
}

fn check_id(rel: &str, id: &str) -> Result<(), RunError> {
    let stem = rel.rsplit('/').next().unwrap_or(rel);
    let stem = stem.split('.').next().unwrap_or(stem);
    if stem != id {
        return Err(RunError::Load(LoadError::IdMismatch {
            path: PathBuf::from(rel),
            expected: stem.to_string(),
            found: id.to_string(),
        }));
    }
    Ok(())
}

/// Load a car and everything it references (tires, lattice, materials) through `src`, with the
/// same validation as [`wp_contracts::load_car`].
pub fn load_car(src: &dyn DataSource, car_id: &str) -> Result<LoadedCar, RunError> {
    let rel = format!("cars/{car_id}.json");
    let def = wp_contracts::parse_vehicle_def(&rel, &src.read(&rel)?)?;
    check_id(&rel, &def.id)?;
    let rel = format!("lattices/{}.json", def.lattice_id);
    let lattice = wp_contracts::parse_lattice_def(&rel, &src.read(&rel)?)?;
    check_id(&rel, &lattice.id)?;
    resolve(src, def, lattice)
}

/// Resolve a definition and its lattice against the tires and materials they reference.
fn resolve(
    src: &dyn DataSource,
    def: wp_contracts::VehicleDef,
    lattice: wp_contracts::LatticeDef,
) -> Result<LoadedCar, RunError> {
    let mut tires = BTreeMap::new();
    for wheel in &def.wheels {
        if !tires.contains_key(&wheel.tire_id) {
            let rel = format!("tires/{}.json", wheel.tire_id);
            let tire = wp_contracts::parse_tire_def(&rel, &src.read(&rel)?)?;
            check_id(&rel, &tire.id)?;
            tires.insert(tire.id.clone(), tire);
        }
    }
    let mut materials = BTreeMap::new();
    for beam in &lattice.beams {
        if !materials.contains_key(&beam.material_id) {
            let rel = format!("materials/{}.json", beam.material_id);
            let mat = wp_contracts::parse_material_def(&rel, &src.read(&rel)?)?;
            check_id(&rel, &mat.id)?;
            materials.insert(mat.id.clone(), mat);
        }
    }
    Ok(wp_contracts::resolve_car(
        def,
        &tires,
        &[lattice],
        &materials,
    )?)
}

/// The mod catalog from `mods/<pack>.json` (the packs in `wp_economy::data::MOD_PACKS`).
pub fn load_mod_catalog(src: &dyn DataSource) -> Result<wp_economy::ModCatalog, RunError> {
    let mut packs = Vec::new();
    for stem in wp_economy::data::MOD_PACKS {
        let rel = format!("mods/{stem}.json");
        let pack: wp_economy::mods::ModPack = serde_json::from_str(&src.read(&rel)?)
            .map_err(|e| RunError::Scenario(rel.clone(), e.to_string()))?;
        packs.push(pack);
    }
    wp_economy::ModCatalog::from_packs(packs)
        .map_err(|e| RunError::Scenario("mods".into(), e.to_string()))
}

/// Load a car with `mods` installed (`wp_economy::mods::apply_mods`: a legal set for its class;
/// only mods active in `mode` change the physics). The result is the car the sim drives: tuned
/// engine, fitted tires, stiffer springs, reinforced lattice. An empty set is [`load_car`].
pub fn load_modded_car(
    src: &dyn DataSource,
    catalog: &wp_economy::ModCatalog,
    car_id: &str,
    mods: &[String],
    mode: DamageMode,
) -> Result<LoadedCar, RunError> {
    let base = load_car(src, car_id)?;
    if mods.is_empty() {
        return Ok(base);
    }
    let what = format!("{car_id} mods");
    let set = catalog
        .validate_set(base.def.class, mods)
        .map_err(|e| RunError::Scenario(what.clone(), e.to_string()))?;
    let applied = wp_economy::mods::apply_mods(&base.def, &base.lattice, &set, Some(mode))
        .map_err(|e| RunError::Scenario(what, e.to_string()))?;
    resolve(src, applied.vehicle, applied.lattice)
}

/// Load the mode table for a damage mode.
pub fn load_mode(src: &dyn DataSource, mode: DamageMode) -> Result<ModeTable, RunError> {
    let rel = match mode {
        DamageMode::Arcade => "modes/arcade.toml",
        DamageMode::Simulation => "modes/sim.toml",
        DamageMode::Extreme => "modes/extreme.toml",
    };
    Ok(wp_contracts::parse_mode_table(rel, &src.read(rel)?)?)
}

/// Build the world for a scenario's city. `Flat` gives `None` (the sim's default world).
/// Traffic collision diagnostics are always on so `world_metric(civilian_collisions)` works.
pub fn load_world(
    src: &dyn DataSource,
    city: &crate::scenario::CitySpec,
    cfg: wp_world::WorldConfig,
) -> Result<Option<wp_world::World>, RunError> {
    use crate::scenario::CitySpec;
    let road =
        |rel: &str, e: wp_world::RoadError| RunError::Scenario(rel.to_string(), e.to_string());
    Ok(match city {
        CitySpec::Flat => None,
        CitySpec::Procedural {
            seed,
            blocks_x,
            blocks_y,
            block_m,
        } => {
            let pg = wp_world::procgen::ProcGenConfig {
                nx: *blocks_x as usize,
                ny: *blocks_y as usize,
                spacing_m: *block_m,
                seed: *seed,
                ..Default::default()
            };
            Some(wp_world::World::procedural(&pg, cfg))
        }
        CitySpec::Preset { name, .. } if wp_world::touge::is_touge(name) => {
            Some(wp_world::World::touge(name, cfg).expect("built-in touge pass generates"))
        }
        CitySpec::Preset { name, seed } => {
            if name == wp_world::ashpeak::CITY_ID {
                let vc = wp_world::vesper::VesperConfig {
                    seed: *seed,
                    ..wp_world::ashpeak::default_config()
                };
                return Ok(Some(wp_world::World::ashpeak(&vc, cfg)));
            }
            if name != wp_world::vesper::CITY_ID {
                return Err(RunError::Scenario(
                    name.clone(),
                    format!(
                        "unknown city preset (known: {}, {}, {})",
                        wp_world::vesper::CITY_ID,
                        wp_world::ashpeak::CITY_ID,
                        wp_world::touge::TOUGE_IDS.join(", ")
                    ),
                ));
            }
            let vc = wp_world::vesper::VesperConfig {
                seed: *seed,
                ..Default::default()
            };
            Some(wp_world::World::vesper(&vc, cfg))
        }
        CitySpec::File(rel) => {
            let net = if rel.ends_with(".json") {
                wp_world::RoadNetwork::from_json(&src.read(rel)?).map_err(|e| road(rel, e))?
            } else {
                wp_world::RoadNetwork::from_bin(&src.read_bytes(rel)?).map_err(|e| road(rel, e))?
            };
            Some(wp_world::World::new(net, cfg))
        }
    })
}

/// Car ids available under `cars/` in an on-disk data root, sorted.
pub fn car_ids(root: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(root.join("cars"))
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn fs_and_memory_load_identically() {
        let fs = FsData::new(root());
        let mem = MemData::from_dir(&root()).unwrap();
        let a = load_car(&fs, "debug_sedan").unwrap();
        let b = load_car(&mem, "debug_sedan").unwrap();
        assert_eq!(a.def, b.def);
        assert_eq!(a.lattice, b.lattice);
        let reference = wp_contracts::load_car(root().join("cars/debug_sedan.json")).unwrap();
        assert_eq!(a.def, reference.def);
        assert!(car_ids(&root()).contains(&"debug_coupe".to_string()));
    }

    #[test]
    fn modded_car_is_the_applied_build() {
        let fs = FsData::new(root());
        let catalog = load_mod_catalog(&fs).unwrap();
        let stock = load_car(&fs, "kestrel_ts").unwrap();
        let mods: Vec<String> = ["tires_semi_slick", "tires_wide", "prot_cage_1"]
            .map(String::from)
            .to_vec();
        let car = load_modded_car(&fs, &catalog, "kestrel_ts", &mods, DamageMode::Arcade).unwrap();
        // The fitted compound is resolved, the tires are wider, the cage adds lattice beams.
        assert!(car.tires.contains_key("semi_slick"));
        assert!(car.def.wheels.iter().all(|w| w.tire_id == "semi_slick"));
        assert!(car.def.wheels[0].width_m > stock.def.wheels[0].width_m);
        assert!(car.lattice.beams.len() > stock.lattice.beams.len());
        // Illegal sets are refused with the economy's reason.
        let err = load_modded_car(
            &fs,
            &catalog,
            "kestrel_ts",
            &["aero_wing_2".to_string()],
            DamageMode::Arcade,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("requires"), "{err}");
    }
}
