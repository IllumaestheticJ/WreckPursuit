//! Civilian traffic styles (owner: wave-2 `style-traffic`). See `docs/design/vehicles/PLAN.md`
//! "Foundation API (for wave 2)" and the worked example in [`super::performance`].

use wp_contracts::VehicleDef;

use crate::style::CarStyle;

/// Style for a traffic car id, or `None` if this module does not own it.
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    let _ = def;
    None
}
