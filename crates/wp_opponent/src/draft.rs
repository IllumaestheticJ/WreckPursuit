//! Slipstream (draft) model — moved to [`wp_vehicle::draft`] when the sim started applying it
//! to every vehicle (`wp_sim::Sim::step_fixed`, owner approval 2026-10-31). Re-exported here
//! so existing callers keep compiling; the tests live with the model in `wp_vehicle`.
//!
//! The AI's tow tactics ([`crate::RaceConfig::draft`]) are the only part that stays in this
//! crate. The sim applies the drag whether or not they are on.

pub use wp_vehicle::draft::{
    drag_scales, drag_scales_all_pairs, drag_scales_into, reduction, DraftBody, DraftParams,
    DraftScratch,
};

/// A [`DraftBody`] from the field's per-vehicle observation (was `DraftBody::from_obs`
/// before the model moved to `wp_vehicle`).
pub fn body_from_obs(o: &crate::field::VehicleObs) -> DraftBody {
    DraftBody {
        pos: [o.state.pos[0], o.state.pos[1]],
        vel: [o.state.lin_vel[0], o.state.lin_vel[1]],
        width_m: o.size_m[0],
        length_m: o.size_m[1],
    }
}
