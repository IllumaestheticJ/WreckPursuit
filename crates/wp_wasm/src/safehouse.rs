//! Safehouses for the testbed (feature F39): where they are (3D marker, map) and how far the
//! player's hide has got. The rule itself is `wp_police::heat` (wanted, inside a safehouse's
//! radius, no unit within `safehouse.clear_radius_m`, for `safehouse.dwell_s`); this module
//! only reads it.

use serde::Serialize;
use wp_contracts::EntityId;
use wp_world::network::PoiKind;
use wp_world::World;

/// Radius of a safehouse, m (the `wp_contracts::PoiInfo::radius_m` every POI gets, which
/// `wp_police` tests the racer against).
pub const SAFEHOUSE_RADIUS_M: f32 = 15.0;

/// One safehouse (`safehouses_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SafehouseSite {
    /// Name from the city data.
    pub name: String,
    /// Position, m.
    pub pos: [f32; 3],
    /// Radius that counts as inside (horizontal), m.
    pub radius_m: f32,
}

/// The safehouses of a city and the hide rule (`safehouses_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Safehouses {
    /// Every `SafehouseCandidate` POI.
    pub sites: Vec<SafehouseSite>,
    /// Seconds to stay inside.
    pub dwell_s: f32,
    /// No police unit may be closer than this, m.
    pub clear_radius_m: f32,
}

/// The safehouses of `world` with the police's hide rule (`dwell_s`, `clear_radius_m`).
pub fn safehouses(world: &World, dwell_s: f32, clear_radius_m: f32) -> Safehouses {
    Safehouses {
        sites: world
            .network()
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::SafehouseCandidate)
            .map(|p| SafehouseSite {
                name: p.name.clone(),
                pos: p.pos,
                radius_m: SAFEHOUSE_RADIUS_M,
            })
            .collect(),
        dwell_s,
        clear_radius_m,
    }
}

/// Seconds the player has hidden so far (`RacerRecord::safehouse_s`; 0 when not hiding or not
/// known to the police).
pub fn hide_progress_s(police: &wp_police::PoliceSystem, player: EntityId) -> f32 {
    police.racer(player).map_or(0.0, |r| r.safehouse_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_vesper_has_safehouses() {
        use crate::data::mem_data;
        let sc = crate::scenario::free_drive_in(
            "debug_sedan",
            wp_contracts::DamageMode::Arcade,
            crate::scenario::city_spec("vesper_v2"),
        );
        let world = simcli::data::load_world(mem_data(), &sc.city, simcli::run::world_config(&sc))
            .unwrap()
            .unwrap();
        let s = safehouses(&world, 10.0, 150.0);
        assert!(s.sites.len() >= 3);
        assert!(s.sites.iter().all(|h| h.radius_m == SAFEHOUSE_RADIUS_M));
    }
}
