//! Live police readout for the testbed's chase overlay (workstream 16, UI kit).
//!
//! Reads `Sim::police()` (lead run 5) for one racer and returns the fields the UI kit's
//! `PoliceReadout` expects. Read-only.

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_contracts::{DayPhase, WantedLevel};
use wp_police::heat::escape_time_s;
use wp_police::PursuitStatus;

use crate::with;

/// JSON shape of `PoliceReadout` (apps/ui-kit/src/live/LiveModel.ts).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Readout {
    level: WantedLevel,
    /// Heat points, 0..=max (100).
    heat: f32,
    status: PursuitStatus,
    bounty: f32,
    /// Seconds since any unit or camera last saw the racer (`None` = never).
    staleness_s: Option<f32>,
    /// Seconds since line of sight was lost (the escape timer while Searching).
    search_s: f32,
    /// Escape time for the current level and day phase, s.
    escape_s: f32,
    phase: DayPhase,
    units_assigned: u32,
}

/// Police readout for `entity` as JSON, or `"null"` if the police have no record of it yet.
#[wasm_bindgen]
pub fn police_status_json(entity: u32) -> Result<String, JsValue> {
    with(|s| {
        let police = s.sim().police();
        let Some(rec) = police.racer(wp_contracts::EntityId(entity)) else {
            return Ok("null".to_string());
        };
        let heat_cfg = &police.config().heat;
        let sighting = police
            .captain_view()
            .racers
            .into_iter()
            .find(|r| r.racer.0 == entity);
        let out = Readout {
            level: rec.level,
            heat: rec.heat,
            status: rec.status,
            bounty: rec.bounty.bounty(heat_cfg),
            staleness_s: sighting.and_then(|r| r.staleness_s),
            search_s: rec.los_lost_s,
            escape_s: escape_time_s(rec.level, police.phase(), heat_cfg),
            phase: police.phase(),
            units_assigned: sighting.map(|r| r.units_assigned).unwrap_or(0),
        };
        serde_json::to_string(&out).map_err(|e| e.to_string())
    })
}
