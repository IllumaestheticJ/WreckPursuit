//! Every mod has a visual entry in `data/cosmetics/visuals.json` (drawn by the testbed's
//! `render/cosmetics.ts`): a list of pieces, or `{"none": "<why it cannot be seen>"}`.

mod common;

use common::*;
use serde_json::Value;
use std::collections::BTreeSet;
use wp_economy::mods::ModEffect;

fn visuals() -> serde_json::Map<String, Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/cosmetics/visuals.json"
    );
    let text = std::fs::read_to_string(path).expect("data/cosmetics/visuals.json");
    let v: Value = serde_json::from_str(&text).expect("visuals.json parses");
    assert_eq!(v["format"], 1, "visuals.json format");
    v["mods"].as_object().expect("visuals.json mods").clone()
}

#[test]
fn every_mod_has_a_visual_entry_and_every_entry_is_a_mod() {
    let e = econ();
    let map = visuals();
    let ids: BTreeSet<&str> = e.catalog.iter().map(|m| m.id.as_str()).collect();
    let keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
    let missing: Vec<_> = ids.difference(&keys).collect();
    let unknown: Vec<_> = keys.difference(&ids).collect();
    assert!(
        missing.is_empty(),
        "mods without a visual entry: {missing:?}"
    );
    assert!(
        unknown.is_empty(),
        "visual entries for unknown mods: {unknown:?}"
    );
}

#[test]
fn entries_are_pieces_or_a_reason() {
    for (id, v) in visuals() {
        match &v {
            Value::Array(pieces) => {
                assert!(
                    !pieces.is_empty(),
                    "{id}: empty piece list (use {{\"none\": ...}})"
                );
                for p in pieces {
                    let name = p["piece"].as_str().unwrap_or("");
                    assert!(!name.is_empty(), "{id}: piece without a name: {p}");
                }
            }
            Value::Object(o) => {
                let why = o.get("none").and_then(Value::as_str).unwrap_or("");
                assert!(!why.trim().is_empty(), "{id}: needs pieces or a reason");
            }
            _ => panic!("{id}: unexpected entry {v}"),
        }
    }
}

#[test]
fn cosmetic_mods_are_visible() {
    let e = econ();
    let map = visuals();
    let cosmetic = e.catalog.iter().filter(|m| {
        m.effects
            .iter()
            .any(|x| matches!(x, ModEffect::Cosmetic { .. }))
    });
    for m in cosmetic {
        assert!(
            map.get(&m.id).is_some_and(Value::is_array),
            "{} is cosmetic but has no visual pieces",
            m.id
        );
    }
}
