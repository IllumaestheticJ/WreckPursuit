//! `data/stats/cars.json` (the menu stat bars) matches the current physics and roster.

use std::collections::BTreeMap;
use std::path::PathBuf;

use simcli::car_stats;
use simcli::FsData;

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

#[test]
fn car_stats_table_is_current() {
    let root = data_root();
    let data = FsData::new(&root);
    let ids = simcli::data::car_ids(&root);
    let rows: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = ids
            .iter()
            .map(|id| {
                let data = &data;
                s.spawn(move || car_stats::measure_car(data, id).unwrap())
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let fresh = car_stats::table(ids.into_iter().zip(rows).collect::<BTreeMap<_, _>>());
    let text = std::fs::read_to_string(root.join(car_stats::TABLE_PATH)).unwrap();
    let diff = car_stats::compare(&car_stats::parse(&text).unwrap(), &fresh);
    assert!(
        diff.is_empty(),
        "data/stats/cars.json is out of date; run `just car-stats`:\n{}",
        diff.join("\n")
    );
}
