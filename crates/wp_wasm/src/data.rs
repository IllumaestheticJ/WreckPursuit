//! Embedded game data. The browser has no filesystem, so `build.rs` embeds every file under
//! `data/{cars,lattices,tires,materials,modes}` and this module parses/validates them with the
//! same `wp_contracts::parse_*` + `resolve_car` path the native loaders use.

use std::collections::BTreeMap;

use wp_contracts::{DamageMode, LoadedCar, MaterialDef, ModeTable, TireDef};

include!(concat!(env!("OUT_DIR"), "/embedded_data.rs"));

/// The embedded files' text, inflated once from `EMBEDDED_Z` (see `build.rs`).
fn blob() -> &'static str {
    static BLOB: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BLOB.get_or_init(|| {
        let raw = miniz_oxide::inflate::decompress_to_vec(EMBEDDED_Z)
            .expect("embedded data blob inflates");
        String::from_utf8(raw).expect("embedded data is UTF-8")
    })
}

/// `(kind, id, file text)` for every embedded data file.
fn embedded() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    let b = blob();
    EMBEDDED_INDEX
        .iter()
        .map(move |(kind, id, off, len)| (*kind, *id, &b[*off..*off + *len]))
}

fn files(kind: &'static str) -> impl Iterator<Item = (&'static str, &'static str)> {
    embedded()
        .filter(move |(k, _, _)| *k == kind)
        .map(|(_, id, text)| (id, text))
}

/// Every embedded file as a simcli data bundle (`cars/<id>.json`, `modes/arcade.toml`, …), so
/// scenarios are built by exactly the code `simcli` uses.
pub fn mem_data() -> &'static simcli::MemData {
    static DATA: std::sync::OnceLock<simcli::MemData> = std::sync::OnceLock::new();
    DATA.get_or_init(|| {
        let files = embedded()
            .map(|(kind, id, text)| {
                let ext = if kind == "modes" { "toml" } else { "json" };
                // Parity cars load like any car (`cars/<id>.json`); only the roster lists skip them.
                let dir = if kind == "parity" { "cars" } else { kind };
                (format!("{dir}/{id}.{ext}"), text.to_string())
            })
            .collect();
        simcli::MemData { files }
    })
}

/// Ids of every embedded car (`data/cars/*.json`), sorted.
pub fn car_ids() -> Vec<String> {
    files("cars").map(|(id, _)| id.to_string()).collect()
}

/// Ids of the Drift Lab W reference-parity test cars (`data/cars/parity/*.json`), sorted.
/// Not in [`car_ids`] (the roster), but [`load_car`] and scenarios load them by id.
pub fn parity_car_ids() -> Vec<String> {
    files("parity").map(|(id, _)| id.to_string()).collect()
}

/// Parse and resolve an embedded car (definition + tires + lattice + materials); roster or
/// parity car.
pub fn load_car(car_id: &str) -> Result<LoadedCar, String> {
    let (_, text) = files("cars")
        .chain(files("parity"))
        .find(|(id, _)| *id == car_id)
        .ok_or_else(|| format!("unknown car {car_id:?} (known: {:?})", car_ids()))?;
    let def = wp_contracts::parse_vehicle_def(car_id, text).map_err(|e| e.to_string())?;
    let lattices = files("lattices")
        .filter(|(id, _)| *id == def.lattice_id)
        .map(|(id, text)| wp_contracts::parse_lattice_def(id, text).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    resolve_with(def, &lattices)
}

/// Resolve a (possibly modified) definition and lattice against every embedded tire and
/// material, e.g. a car with mods applied (mods may switch to tires the stock car doesn't use).
pub fn resolve_with(
    def: wp_contracts::VehicleDef,
    lattices: &[wp_contracts::LatticeDef],
) -> Result<LoadedCar, String> {
    let mut tires: BTreeMap<String, TireDef> = BTreeMap::new();
    for (id, text) in files("tires") {
        let t = wp_contracts::parse_tire_def(id, text).map_err(|e| e.to_string())?;
        tires.insert(t.id.clone(), t);
    }
    let mut materials: BTreeMap<String, MaterialDef> = BTreeMap::new();
    for (id, text) in files("materials") {
        let m = wp_contracts::parse_material_def(id, text).map_err(|e| e.to_string())?;
        materials.insert(m.id.clone(), m);
    }
    wp_contracts::resolve_car(def, &tires, lattices, &materials).map_err(|e| e.to_string())
}

/// Menu stat bars for every embedded car: the embedded `data/stats/cars.json`, with any car
/// missing from it measured here on the `wp_vehicle` rig (slow, once; `simcli car-stats --check`
/// keeps the table complete).
pub fn car_stats() -> Result<&'static simcli::car_stats::CarStatsTable, String> {
    static TABLE: std::sync::OnceLock<Result<simcli::car_stats::CarStatsTable, String>> =
        std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mut table = match files("stats").find(|(id, _)| *id == "cars") {
                Some((_, text)) => simcli::car_stats::parse(text)?,
                None => simcli::car_stats::table(BTreeMap::new()),
            };
            for id in car_ids() {
                if let std::collections::btree_map::Entry::Vacant(slot) = table.cars.entry(id) {
                    let row = simcli::car_stats::measure_car(mem_data(), slot.key())
                        .map_err(|e| e.to_string())?;
                    slot.insert(row);
                }
            }
            Ok(table)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Parse the embedded mode table for `mode` (`data/modes/arcade.toml` or `sim.toml`).
pub fn mode_table(mode: DamageMode) -> Result<ModeTable, String> {
    let stem = match mode {
        DamageMode::Arcade => "arcade",
        DamageMode::Simulation => "sim",
    };
    let (_, text) = files("modes")
        .find(|(id, _)| *id == stem)
        .ok_or_else(|| format!("missing data/modes/{stem}.toml"))?;
    wp_contracts::parse_mode_table(stem, text).map_err(|e| e.to_string())
}

/// Parse a damage mode name: `"Arcade"`/`"arcade"` or `"Simulation"`/`"Sim"`/`"sim"`.
pub fn parse_mode(name: &str) -> Result<DamageMode, String> {
    match name.to_ascii_lowercase().as_str() {
        "arcade" => Ok(DamageMode::Arcade),
        "simulation" | "sim" => Ok(DamageMode::Simulation),
        other => Err(format!(
            "unknown damage mode {other:?} (Arcade | Simulation)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_car_resolves() {
        let ids = car_ids();
        assert!(ids.contains(&"debug_sedan".to_string()));
        for id in ids {
            load_car(&id).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }

    #[test]
    fn car_stats_cover_every_embedded_car() {
        let table = car_stats().unwrap();
        assert_eq!(table.cars.keys().cloned().collect::<Vec<_>>(), car_ids());
        for (id, s) in &table.cars {
            let b = s.bars;
            for v in [b.acceleration, b.top_speed, b.cornering] {
                assert!((0.0..=1.0).contains(&v), "{id}: {b:?}");
            }
        }
    }

    #[test]
    fn parity_cars_load_but_are_not_in_the_roster() {
        let parity = parity_car_ids();
        // a, b, d, fwd_c (Drift Lab W) + e, f (the owner's keyboard reference clips, row 142).
        assert_eq!(parity.len(), 6, "{parity:?}");
        let roster = car_ids();
        for id in &parity {
            assert!(!roster.contains(id), "{id} is in the roster");
            load_car(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            simcli::data::load_car(mem_data(), id).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }

    #[test]
    fn mem_data_loads_through_simcli() {
        let car = simcli::data::load_car(mem_data(), "debug_coupe").unwrap();
        assert_eq!(car.def.id, "debug_coupe");
        simcli::data::load_mode(mem_data(), DamageMode::Simulation).unwrap();
    }

    #[test]
    fn both_mode_tables_parse() {
        assert_eq!(
            mode_table(DamageMode::Arcade).unwrap().mode,
            DamageMode::Arcade
        );
        assert_eq!(
            mode_table(DamageMode::Simulation).unwrap().mode,
            DamageMode::Simulation
        );
    }
}

#[cfg(test)]
mod parity_session_tests {
    use super::*;

    /// Drift Lab W in the browser: a free-drive session with each parity car starts (car,
    /// panels and all), as `?drift=w` does.
    #[test]
    fn parity_cars_start_a_free_drive_session() {
        for id in parity_car_ids() {
            let sc = crate::scenario::free_drive(&id, DamageMode::Arcade, false);
            let mut s =
                crate::session::Session::new(sc, false).unwrap_or_else(|e| panic!("{id}: {e}"));
            s.step(1.0 / 60.0);
        }
    }
}
