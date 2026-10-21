//! Menu stat bars for a car **with mods installed**, measured on the physics rig.
//!
//! The stock table is precomputed (`data/stats/cars.json`), but a build is any legal set of
//! mods, so it is measured on demand: `wp_economy::Economy::preview` validates the set and
//! applies it (`apply_mods`, the same path as installing), the modified car is resolved against
//! the embedded tires and materials, and [`wp_vehicle::stats::CarStats::measure`] runs the
//! 0–100, top-speed and skidpad rigs. About a second in the browser; results are cached per
//! (car, sorted mods, mode) for the session.

use std::cell::RefCell;
use std::collections::BTreeMap;

use wasm_bindgen::prelude::*;
use wp_contracts::DamageMode;
use wp_vehicle::stats::CarStats;

use simcli::car_stats::CarStatsEntry;

use crate::garage::with_economy;
use crate::{data, err, json};

type Key = (String, Vec<String>, Option<DamageMode>);

thread_local! {
    static CACHE: RefCell<BTreeMap<Key, CarStatsEntry>> = const { RefCell::new(BTreeMap::new()) };
}

/// Measure `car_id` with `mods` applied (`mode`: only mods active in that damage mode count;
/// `None` = all). An empty set returns the stock row of the table without measuring.
pub fn build_stats(
    car_id: &str,
    mods: &[String],
    mode: Option<DamageMode>,
) -> Result<CarStatsEntry, String> {
    if mods.is_empty() {
        if let Some(row) = data::car_stats()?.cars.get(car_id) {
            return Ok(row.clone());
        }
    }
    let mut sorted = mods.to_vec();
    sorted.sort();
    let key = (car_id.to_string(), sorted, mode);
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(hit);
    }
    let car = data::load_car(car_id)?;
    let applied = with_economy(|e| e.preview(&car.def, &car.lattice, mods, mode))
        .map_err(|e| e.to_string())?
        .applied;
    let modded = data::resolve_with(applied.vehicle, &[applied.lattice])?;
    let row = CarStatsEntry::from_stats(&modded.def.display_name, &CarStats::measure(&modded));
    CACHE.with(|c| c.borrow_mut().insert(key, row.clone()));
    Ok(row)
}

/// Stat bars for a build as JSON (`simcli::car_stats::CarStatsEntry`: `{display_name,
/// zero_to_100_s, top_speed_kmh, cornering_g, bars}`). `mods_json` is a JSON array of mod ids
/// (a legal set for the car's class); `mode` is `"Arcade"`, `"Simulation"` or `""` (every mod
/// active). Slow the first time per build (~1 s, three physics rigs); cached afterwards.
#[wasm_bindgen]
pub fn car_stats_build_json(car_id: &str, mods_json: &str, mode: &str) -> Result<String, JsValue> {
    let mods: Vec<String> = serde_json::from_str(mods_json).map_err(err)?;
    let mode = if mode.is_empty() {
        None
    } else {
        Some(data::parse_mode(mode).map_err(err)?)
    };
    build_stats(car_id, &mods, mode)
        .map(|r| json(&r))
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_and_tire_mods_move_the_bars() {
        let stock = build_stats("kestrel_ts", &[], None).unwrap();
        let mods: Vec<String> = ["engine_intake_1", "engine_ecu_2", "tires_semi_slick"]
            .map(String::from)
            .to_vec();
        let build = build_stats("kestrel_ts", &mods, None).unwrap();
        println!("stock {stock:?}\nbuild {build:?}");
        assert!(build.zero_to_100_s.unwrap() < stock.zero_to_100_s.unwrap());
        assert!(build.cornering_g > stock.cornering_g);
        assert!(build.bars.acceleration > stock.bars.acceleration);
        // Cached: same result, any mod order.
        let again = build_stats(
            "kestrel_ts",
            &[mods[2].clone(), mods[0].clone(), mods[1].clone()],
            None,
        );
        assert_eq!(again.unwrap(), build);
    }

    #[test]
    fn illegal_sets_are_refused() {
        assert!(build_stats("kestrel_ts", &["engine_ecu_2".into()], None).is_err());
        assert!(build_stats("kestrel_ts", &["no_such_mod".into()], None).is_err());
    }
}
