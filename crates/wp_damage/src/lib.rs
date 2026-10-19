//! # wp_damage
//!
//! Semi-soft-body damage (owned by workstream 03). **Current state: stub.**
//!
//! [`DamageModel`] implements [`wp_contracts::DamageSim`]: it holds the lattice rest pose and
//! returns neutral [`HandlingEffects`]. Prompt 03 implements deformation, detachment,
//! component damage and repair estimates behind the same surface.
//! [`box_lattice`] generates the debug box lattices in `data/lattices/`.

#![warn(missing_docs)]

pub mod box_lattice;

use std::collections::BTreeMap;

use wp_contracts::{ContactImpulse, DamageSim, HandlingEffects, LatticeDef, MaterialDef, SimEvent};
pub use wp_contracts::{ModeTable, PriceTable, RepairEstimate};

/// Damage state for one vehicle.
#[derive(Debug, Clone)]
pub struct DamageModel {
    lattice: LatticeDef,
    materials: BTreeMap<String, MaterialDef>,
    node_positions: Vec<[f32; 3]>,
    health: f32,
}

impl DamageModel {
    /// Build from a validated lattice and the materials it references
    /// (e.g. from [`wp_contracts::LoadedCar`]).
    pub fn new(lattice: &LatticeDef, materials: &BTreeMap<String, MaterialDef>) -> Self {
        Self {
            node_positions: lattice.nodes.iter().map(|n| n.pos_m).collect(),
            lattice: lattice.clone(),
            materials: materials.clone(),
            health: 1.0,
        }
    }

    /// Global health 0..=1 (1 = pristine).
    pub fn health(&self) -> f32 {
        self.health
    }

    /// The lattice definition (rest pose).
    pub fn lattice(&self) -> &LatticeDef {
        &self.lattice
    }

    /// Materials used by the lattice.
    pub fn materials(&self) -> &BTreeMap<String, MaterialDef> {
        &self.materials
    }

    /// Restore the pristine state (full repair).
    pub fn reset(&mut self) {
        self.node_positions = self.lattice.nodes.iter().map(|n| n.pos_m).collect();
        self.health = 1.0;
    }
}

impl DamageSim for DamageModel {
    fn apply_contacts(
        &mut self,
        _contacts: &[ContactImpulse],
        _mode: &ModeTable,
        _dt: f32,
        _out: &mut Vec<SimEvent>,
    ) {
        // STUB: no deformation yet (prompt 03).
    }

    fn handling_effects(&self) -> HandlingEffects {
        HandlingEffects::NEUTRAL
    }

    fn node_positions(&self) -> &[[f32; 3]] {
        &self.node_positions
    }

    fn repair_estimate(&self, _mode: &ModeTable, _price_table: &PriceTable) -> RepairEstimate {
        RepairEstimate::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_is_neutral_and_exposes_nodes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        );
        let car = wp_contracts::load_car(path).expect("debug_sedan loads");
        let dm = DamageModel::new(&car.lattice, &car.materials);
        assert_eq!(dm.node_positions().len(), car.lattice.nodes.len());
        assert_eq!(dm.handling_effects(), HandlingEffects::NEUTRAL);
        assert_eq!(dm.health(), 1.0);
    }
}
