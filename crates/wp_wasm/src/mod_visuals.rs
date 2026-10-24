//! Mods you can see: the catalog for a car and what a build looks like on it.
//!
//! Installing mods is a scenario field (`EntitySpec::mods`, simcli format v2): the entity is
//! spawned as the modified car (`simcli::data::load_modded_car`), so the physics, the lattice
//! and `vehicle_info` (wheel width, mass, …) all follow the build. These calls give the
//! renderer the other half: which parts to draw and where ([`wp_economy::visuals`]).
//!
//! | function | returns |
//! |---|---|
//! | `mod_catalog_json(car_id)` | `[{...ModDef, parts: [[PartKind, style]]}]`: every mod the car's class may fit, catalog order |
//! | `build_visuals_json(car_id, mods_json)` | [`wp_economy::BuildVisuals`] for a legal set (error text for an illegal one) |
//! | `entity_visuals_json(entity)` | [`wp_economy::BuildVisuals`] of a live entity's installed mods |

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_contracts::EntityId;
use wp_economy::{BuildVisuals, ModDef, PartKind};

use crate::garage::with_economy;
use crate::{data, err, json, with};

#[derive(Serialize)]
struct CatalogEntry<'a> {
    #[serde(flatten)]
    def: &'a ModDef,
    /// Parts the mod draws (`wp_economy::visuals::mod_parts`).
    parts: Vec<(PartKind, String)>,
}

/// Catalog entries a car may fit, as JSON.
pub fn mod_catalog(car_id: &str) -> Result<String, String> {
    let car = data::load_car(car_id)?;
    Ok(with_economy(|e| {
        let entries: Vec<CatalogEntry> = e
            .catalog
            .iter()
            .filter(|m| m.allows_class(car.def.class))
            .map(|m| CatalogEntry {
                def: m,
                parts: wp_economy::visuals::mod_parts(m),
            })
            .collect();
        json(&entries)
    }))
}

/// What `mods` look like on `car_id`.
pub fn build_visuals(car_id: &str, mods: &[String]) -> Result<BuildVisuals, String> {
    let car = data::load_car(car_id)?;
    with_economy(|e| e.build_visuals(&car.def, &car.lattice, mods)).map_err(|e| e.to_string())
}

/// Every mod `car_id`'s class may fit, with the parts each draws (JSON array; `ModDef` fields
/// plus `parts: [[kind, style], …]`).
#[wasm_bindgen]
pub fn mod_catalog_json(car_id: &str) -> Result<String, JsValue> {
    mod_catalog(car_id).map_err(err)
}

/// [`wp_economy::BuildVisuals`] for `mods_json` (a JSON array of mod ids) on `car_id`.
#[wasm_bindgen]
pub fn build_visuals_json(car_id: &str, mods_json: &str) -> Result<String, JsValue> {
    let mods: Vec<String> = serde_json::from_str(mods_json).map_err(err)?;
    build_visuals(car_id, &mods).map(|v| json(&v)).map_err(err)
}

/// [`wp_economy::BuildVisuals`] of a live entity (its scenario `mods`; stock = no parts).
#[wasm_bindgen]
pub fn entity_visuals_json(entity: u32) -> Result<String, JsValue> {
    with(|s| {
        let id = EntityId(entity);
        let car_id = s
            .vehicle(id)
            .map(|v| v.car_id.clone())
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        let mods = s.entity_mods(id).to_vec();
        build_visuals(&car_id, &mods).map(|v| json(&v))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::free_drive;
    use crate::session::Session;
    use wp_contracts::DamageMode;

    fn build() -> Vec<String> {
        [
            "wheels_forged",
            "tires_wide",
            "aero_wing_1",
            "susp_springs_1",
            "engine_ecu_1",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn modded_entity_drives_the_modified_car() {
        let mut sc = free_drive("kestrel_ts", DamageMode::Arcade, false);
        sc.entities[0].mods = build();
        let s = Session::new(sc, false).unwrap();
        let stock =
            Session::new(free_drive("kestrel_ts", DamageMode::Arcade, false), false).unwrap();
        let id = s.entities()[0];
        let (a, b) = (s.car(id).unwrap(), stock.car(id).unwrap());
        // Wider tires, more power, lower CG, the wing's drag and downforce.
        for (w, w0) in a.def.wheels.iter().zip(&b.def.wheels) {
            assert!((w.width_m - w0.width_m - 0.02).abs() < 1e-6);
        }
        assert!(a.def.cg_height_m < b.def.cg_height_m);
        assert!(a.def.aero_downforce_coeffs[1] > b.def.aero_downforce_coeffs[1]);
        assert_eq!(s.entity_mods(id), build().as_slice());
        assert!(stock.entity_mods(id).is_empty());
    }

    #[test]
    fn catalog_and_visuals() {
        let cat: Vec<serde_json::Value> =
            serde_json::from_str(&mod_catalog("kestrel_ts").unwrap()).unwrap();
        let wing = cat.iter().find(|m| m["id"] == "aero_wing_2").unwrap();
        assert_eq!(wing["parts"][0][0], "RearWing");
        assert_eq!(wing["parts"][0][1], "gt");
        // Class-limited mods are left out (push bar: police only).
        assert!(cat.iter().all(|m| m["id"] != "prot_push_bar"));
        let v = build_visuals("kestrel_ts", &build()).unwrap();
        assert_eq!(v.style(PartKind::Rims), Some("forged_5spoke"));
        assert!(v.ride_height_m < 0.0);
        // Missing requirement is refused (GT wing needs the splitter).
        assert!(build_visuals("kestrel_ts", &["aero_wing_2".to_string()]).is_err());
    }
}
