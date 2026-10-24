//! Greybox parts (ashpeak-port, 2026-10-24): Port Vesper ships once. The parts add up to the
//! monolithic greyboxes, each part fits the preview, and the shipped parts are current.

use std::collections::BTreeMap;
use std::path::Path;

use city_import::dedup::drawn;
use city_import::parts::{build_parts_from, manifest_json, MAX_TILE_GLB_BYTES};
use wp_world::vesper::ashpeak;
use wp_world::vesper::{self, VesperConfig};
use wp_world::CityMap;

/// Items of a drawn set, counted per (material, exact key).
fn count(items: &city_import::dedup::Drawn) -> BTreeMap<(String, [[u32; 3]; 3]), usize> {
    let mut c = BTreeMap::new();
    for (m, list) in items {
        for (k, _) in list {
            *c.entry((m.clone(), *k)).or_default() += 1;
        }
    }
    c
}

#[test]
fn parts_add_up_fit_the_preview_and_are_shipped_current() {
    let v = CityMap::new(vesper::generate(&VesperConfig::default()));
    let a = CityMap::new(ashpeak::generate(&ashpeak::default_config()));
    let gp = build_parts_from(&v, &a);

    // Manifests: vesper_v2 = shared + own; ashpeak = the same shared part + county tiles.
    let (vid, vparts) = &gp.manifests[0];
    let (aid, aparts) = &gp.manifests[1];
    assert_eq!(vid, "vesper_v2");
    assert_eq!(aid, "ashpeak");
    assert_eq!(vparts, &["vesper_v2.shared", "vesper_v2.own"]);
    assert_eq!(aparts[0], "vesper_v2.shared");
    assert!(aparts.len() >= 2 && aparts[1..].iter().all(|s| s.starts_with("ashpeak.county")));

    // Sizes: every part fits the preview (≤ 9 MB glb ≈ 12 MB base64), except vesper_v2's own
    // shared part, which is smaller than today's monolithic file anyway.
    let mut total_ashpeak = 0;
    for p in &gp.parts {
        println!("{:24} {:>6.2} MB glb", p.stem, p.glb.len() as f64 / 1e6);
        if p.stem.starts_with("ashpeak.county") {
            assert!(
                p.glb.len() <= MAX_TILE_GLB_BYTES,
                "{} is {} bytes",
                p.stem,
                p.glb.len()
            );
        }
        assert!(
            p.glb.len() * 4 / 3 < 14_000_000,
            "{} is over the preview split",
            p.stem
        );
        if aparts.contains(&p.stem) {
            total_ashpeak += p.glb.len();
        }
    }
    println!("ashpeak download: {:.2} MB glb", total_ashpeak as f64 / 1e6);

    // The parts add up, item for item: shared + own = vesper_v2, shared + county = ashpeak.
    // (Checked on the drawn items the parts are cut from: dedup::drawn keys every triangle and
    // instance by exact bits; the parts keep triangles whole, so the cut is a partition.)
    let p = city_import::dedup::partition(
        &drawn(&v),
        &drawn(&a),
        ashpeak::plan::VESPER_LO,
        ashpeak::plan::VESPER_HI,
    );
    let (dv, da) = (drawn(&v), drawn(&a));
    let split = |d: &city_import::dedup::Drawn, county: bool| {
        let mut out: city_import::dedup::Drawn = BTreeMap::new();
        for (m, list) in d {
            for (k, c) in list {
                if p.county_owns(m, *c) == county {
                    out.entry(m.clone()).or_default().push((*k, *c));
                }
            }
        }
        out
    };
    let shared = split(&dv, false);
    let mut v_sum = count(&shared);
    for (k, n) in count(&split(&dv, true)) {
        *v_sum.entry(k).or_default() += n;
    }
    assert_eq!(v_sum, count(&dv), "vesper_v2 shared + own != vesper_v2");
    let mut a_sum = count(&shared);
    for (k, n) in count(&split(&da, true)) {
        *a_sum.entry(k).or_default() += n;
    }
    assert_eq!(
        a_sum,
        count(&da),
        "vesper_v2 shared + ashpeak county != ashpeak"
    );

    // Shipped files are current.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/city");
    for part in &gp.parts {
        let glb = std::fs::read(dir.join(format!("{}.greybox.glb", part.stem)))
            .unwrap_or_else(|_| panic!("{}.greybox.glb missing: run `just city-v2`", part.stem));
        assert!(
            glb == part.glb,
            "{}.greybox.glb is stale: run `just city-v2`",
            part.stem
        );
        let b64 =
            std::fs::read_to_string(dir.join(format!("{}.greybox.b64.txt", part.stem))).unwrap();
        assert!(
            b64 == city_import::base64(&glb),
            "{}.greybox.b64.txt does not match",
            part.stem
        );
    }
    for (id, stems) in &gp.manifests {
        let m = std::fs::read_to_string(dir.join(format!("{id}.greybox.parts.json"))).unwrap();
        assert_eq!(m, manifest_json(stems), "{id}.greybox.parts.json is stale");
    }
    // No county tile left over from an earlier, larger split.
    let tiles = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("ashpeak.county")
                && e.file_name().to_string_lossy().ends_with(".glb")
        })
        .count();
    assert_eq!(tiles, aparts.len() - 1, "stray county tiles in data/city");
}
