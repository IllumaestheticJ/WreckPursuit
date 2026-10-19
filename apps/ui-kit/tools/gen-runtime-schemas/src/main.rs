//! Writes JSON Schemas for the runtime `wp_contracts` types the UI kit consumes into the
//! directory given as the first argument (default `../../schemas/runtime`, relative to this
//! crate). Run via `npm run gen:types` in `apps/ui-kit`.

use std::path::PathBuf;

use schemars::{schema_for, Schema};
use wp_contracts::{
    AssistProfile, DamageMode, DayPhase, HandlingEffects, InputSource, RepairEstimate, Role,
    ServerToClient, SimEvent, TimeOfDay, VehicleInput, VehicleState, WantedLevel,
};

/// `(file name, schema)` for every runtime type the UI reads.
fn runtime_schemas() -> Vec<(&'static str, Schema)> {
    vec![
        ("sim_event.schema.json", schema_for!(SimEvent)),
        ("vehicle_state.schema.json", schema_for!(VehicleState)),
        ("vehicle_input.schema.json", schema_for!(VehicleInput)),
        ("assist_profile.schema.json", schema_for!(AssistProfile)),
        ("input_source.schema.json", schema_for!(InputSource)),
        ("handling_effects.schema.json", schema_for!(HandlingEffects)),
        ("repair_estimate.schema.json", schema_for!(RepairEstimate)),
        ("role.schema.json", schema_for!(Role)),
        ("damage_mode.schema.json", schema_for!(DamageMode)),
        ("time_of_day.schema.json", schema_for!(TimeOfDay)),
        ("day_phase.schema.json", schema_for!(DayPhase)),
        ("server_to_client.schema.json", schema_for!(ServerToClient)),
        ("wanted_level.schema.json", schema_for!(WantedLevel)),
    ]
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/runtime"));
    std::fs::create_dir_all(&out).expect("create output dir");
    for (name, schema) in runtime_schemas() {
        let mut text = serde_json::to_string_pretty(&schema).expect("schema serializes");
        text.push('\n');
        let path = out.join(name);
        std::fs::write(&path, text).expect("write schema");
        println!("wrote {}", path.display());
    }
}
