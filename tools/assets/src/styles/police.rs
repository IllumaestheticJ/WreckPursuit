//! Police styles (owner: wave-2 `style-police`). See `docs/design/vehicles/PLAN.md`
//! "Foundation API (for wave 2)" and the worked example in [`super::performance`].
//! Light bars: kind `Lightbar`, materials `LightbarRed` / `LightbarBlue` / `LightbarClear`;
//! the renderer gives left (x < 0) and right parts independent materials.

use wp_contracts::VehicleDef;

use crate::style::CarStyle;

/// Style for a police car id, or `None` if this module does not own it.
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    let _ = def;
    None
}
