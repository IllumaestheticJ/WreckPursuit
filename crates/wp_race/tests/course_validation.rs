//! Every picker race's course is well formed on its own city (races audit P3,
//! `wp_race::validate`): plans at its advertised length, gates in order, ≤ 230 m apart, on
//! the road and as wide as it, grid slots in the lanes, a run-off past a sprint's finish,
//! circuits closed. Fast (no simulation): planning only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use simcli::{load_scenario, FsData};
use wp_race::catalog;
use wp_race::validate::validate_course;
use wp_world::World;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_picker_race_course_is_well_formed() {
    let data = FsData::new(repo().join("data"));
    let mut worlds: BTreeMap<&str, World> = BTreeMap::new();
    let mut problems = Vec::new();
    for def in catalog::races() {
        let city = def.city.id();
        let world = worlds.entry(city).or_insert_with(|| {
            // The race's own picker scenario says how its city is built.
            let path = repo().join(format!("apps/simcli/scenarios/race/pick_{}.ron", def.id));
            let sc = load_scenario(&path).unwrap();
            simcli::data::load_world(&data, &sc.city, simcli::run::world_config(&sc))
                .unwrap()
                .expect("a city")
        });
        match validate_course(world, &def) {
            Ok(r) => eprintln!(
                "{:<28} {:>6.0} m, {:>2} gates, gaps {:.0}..{:.0} m, narrowest {:.1} m, {} in junctions",
                r.id, r.length_m, r.gates, r.gate_gap_m.0, r.gate_gap_m.1, r.min_gate_width_m, r.gates_in_junctions
            ),
            Err(e) => problems.extend(e),
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
