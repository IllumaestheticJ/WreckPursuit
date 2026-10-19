//! JSON Schema generation for every data file type. Used by the `gen-schemas` binary and by
//! the schema validation tests.

use schemars::{schema_for, Schema};

use crate::{LatticeDef, MaterialDef, ModeTable, TireDef, VehicleDef};

/// `(file name in schemas/, schema)` for every data file type.
pub fn all_schemas() -> Vec<(&'static str, Schema)> {
    vec![
        ("vehicle_def.schema.json", schema_for!(VehicleDef)),
        ("tire_def.schema.json", schema_for!(TireDef)),
        ("material_def.schema.json", schema_for!(MaterialDef)),
        ("lattice_def.schema.json", schema_for!(LatticeDef)),
        ("mode_table.schema.json", schema_for!(ModeTable)),
    ]
}

/// Pretty JSON text of a schema, with a trailing newline (the exact bytes written to disk).
pub fn schema_text(schema: &Schema) -> String {
    let mut s = serde_json::to_string_pretty(schema).expect("schema serializes");
    s.push('\n');
    s
}
