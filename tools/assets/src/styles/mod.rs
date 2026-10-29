//! Per-car style modules. Each returns `Some(CarStyle)` for the car ids it owns and `None`
//! otherwise; [`crate::style::style_for`] asks them in the order performance, traffic,
//! police and falls back to [`crate::style::class_default`].
//!
//! | Module | Owner (wave 2) | Cars |
//! |---|---|---|
//! | [`models`] | painted-car pipeline | real car families from section specs (asked first) |
//! | [`performance`] | style-performance | muscle, hot hatch, tuner, supercar, exotic, `debug_coupe` |
//! | [`traffic`] | style-traffic | civilian traffic cars |
//! | [`police`] | style-police | `sentinel_*` |

pub mod models;
pub mod performance;
pub mod police;
pub mod traffic;
