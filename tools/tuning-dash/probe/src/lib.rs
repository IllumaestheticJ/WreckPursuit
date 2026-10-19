//! # tuning-probe
//!
//! Measurement backend of `tools/tuning-dash` (the sweep runner and dashboard are Python, in
//! `tools/tuning-dash/tuning_dash`). Each `tuning-probe run` measures one sweep point:
//!
//! - [`path`]: `<file>:<field path>` parameter targets → JSON pointers.
//! - [`overrides`]: `set` / `scale` / `add` applied to an in-memory overlay of the data root
//!   (nothing on disk changes while sweeping).
//! - [`probe`]: runs a simcli v2 scenario, `wp_vehicle::metrics`, or the `wp_damage` barrier
//!   harness against the overlay and returns flat metrics, series and checks.
//! - [`pursuit`]: city scenario with the police tables from the overlay; police hits classified.
//! - [`survival`]: repeated barrier hits until total loss; single-hit total-loss speed.
//! - [`frames`]: top-down SVG frames of a run for the dashboard thumbnails.
//! - [`diff`]: field diff of two data files.
//! - [`promote`] + [`textedit`]: write chosen values back with one-line text edits, validate
//!   with the contract loaders, append a changelog line (`tune.py` uses
//!   `docs/TUNING_CHANGELOG.md`).
//!
//! Not part of the simulation: nothing here is reachable from `wp_wasm`.

#![warn(missing_docs)]

pub mod diff;
pub mod frames;
pub mod overrides;
pub mod path;
pub mod probe;
pub mod promote;
pub mod pursuit;
pub mod survival;
pub mod textedit;
