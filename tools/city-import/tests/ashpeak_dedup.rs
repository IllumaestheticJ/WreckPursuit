//! Ashpeak greybox de-dup (ashpeak-port; lead, 2026-10-23): Port Vesper inside `ashpeak` looks
//! the same as `vesper_v2`, so `ashpeak` can ship `vesper_v2`'s greybox once plus county
//! tiles. Outside the county's share (`city_import::dedup`), the two maps draw exactly the
//! same triangles and decor instances, and the county's share inside Port Vesper is small (the
//! tie-ins at its north and west edges).

use city_import::dedup::{drawn, partition, PARTITION_CELL_M};
use wp_world::vesper::ashpeak::{self, plan};
use wp_world::vesper::{self, VesperConfig};
use wp_world::CityMap;

#[test]
fn vesper_inside_ashpeak_draws_the_same_greybox() {
    let v = CityMap::new(vesper::generate(&VesperConfig::default()));
    let a = CityMap::new(ashpeak::generate(&ashpeak::default_config()));
    let (dv, da) = (drawn(&v), drawn(&a));
    let p = partition(&dv, &da, plan::VESPER_LO, plan::VESPER_HI);
    let shared = p.check(&dv, &da).expect("the split adds up");
    let total: usize = dv.values().map(Vec::len).sum();
    let cells_inside = ((plan::VESPER_HI[0] - plan::VESPER_LO[0]) / PARTITION_CELL_M).ceil()
        * ((plan::VESPER_HI[1] - plan::VESPER_LO[1]) / PARTITION_CELL_M).ceil();
    println!(
        "vesper_v2 items {total}, shared with ashpeak {shared} ({:.1} %); county cells inside Port Vesper {} of {cells_inside}",
        100.0 * shared as f32 / total as f32,
        p.county_cells.len()
    );
    for (m, items) in &dv {
        let kept = items.iter().filter(|(_, c)| !p.county_owns(m, *c)).count();
        println!("  {m:22} {kept:>7} / {:>7} shared", items.len());
    }
    // Port Vesper looks the same: nearly all of its buildings and decor are shared.
    for m in [
        "building",
        "roof",
        "garage",
        "inst:trees",
        "inst:fence",
        "landmark",
    ] {
        let items = &dv[m];
        let kept = items.iter().filter(|(_, c)| !p.county_owns(m, *c)).count();
        assert!(
            kept as f32 >= 0.85 * items.len() as f32,
            "{m}: only {kept} of {} shared",
            items.len()
        );
    }
    assert!(
        (p.county_cells.len() as f32) < 0.15 * cells_inside,
        "{} county cells",
        p.county_cells.len()
    );
}
