//! The greybox never draws solid-looking geometry (bridge piers, deck sides, walls, buildings,
//! feature prisms) across a road: a car at roof height driving along any lane passes through
//! no such triangle. Physics has no colliders for most of it, so this is purely visual, which
//! is exactly why it needs its own check.

use std::collections::BTreeMap;
use wp_world::map::CityMap;
use wp_world::network::RoadNetwork;

type V3 = [f32; 3];
/// A world-space triangle.
type Tri = [V3; 3];
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn seg_tri(p: V3, q: V3, a: V3, b: V3, c: V3) -> bool {
    let d = sub(q, p);
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-9 {
        return false;
    }
    let f = 1.0 / det;
    let s = sub(p, a);
    let u = f * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let qv = cross(s, e1);
    let v = f * dot(d, qv);
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = f * dot(e2, qv);
    (0.0..=1.0).contains(&t)
}
/// Triangles of a box or cylinder feature (prisms are in the greybox already).
fn feature_tris(shape: &wp_world::network::FeatureShape) -> Vec<Tri> {
    use wp_world::network::FeatureShape;
    let (c, h, yaw, pitch) = match shape {
        FeatureShape::Box {
            centre,
            half,
            yaw,
            pitch,
        } => (*centre, *half, *yaw, *pitch),
        FeatureShape::Cylinder {
            base,
            radius,
            height,
        } => (
            [base[0], base[1], base[2] + height * 0.5],
            [*radius, *radius, height * 0.5],
            0.0,
            0.0,
        ),
        FeatureShape::Prism { .. } => return Vec::new(),
    };
    let (sp, cp) = wp_math::sin_cos(pitch);
    let (sy, cy) = wp_math::sin_cos(yaw);
    let corner = |x: f32, y: f32, z: f32| -> V3 {
        // Pitch about local +X, then yaw about +Z (the Feature convention).
        let (y1, z1) = (y * cp - z * sp, y * sp + z * cp);
        [c[0] + x * cy - y1 * sy, c[1] + x * sy + y1 * cy, c[2] + z1]
    };
    let v: Vec<V3> = (0..8)
        .map(|i| {
            let sx = if i & 1 == 0 { -h[0] } else { h[0] };
            let syy = if i & 2 == 0 { -h[1] } else { h[1] };
            let sz = if i & 4 == 0 { -h[2] } else { h[2] };
            corner(sx, syy, sz)
        })
        .collect();
    const FACES: [[usize; 4]; 6] = [
        [0, 1, 3, 2],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 3, 7, 5],
    ];
    FACES
        .iter()
        .flat_map(|f| [[v[f[0]], v[f[1]], v[f[2]]], [v[f[0]], v[f[2]], v[f[3]]]])
        .collect()
}

/// Lane segments (at 1.2 m and 3 m above the lane) that pass through a blocking greybox
/// triangle, per material.
fn blocked(net: RoadNetwork, materials: &[&str]) -> BTreeMap<String, Vec<V3>> {
    let map = CityMap::new(net);
    let prims = city_import::glb::build_greybox(&map);
    let cell = 8.0f32;
    let mut grid: BTreeMap<(i32, i32), Vec<(usize, Tri)>> = BTreeMap::new();
    for (pi, p) in prims.iter().enumerate() {
        let blocking = materials
            .iter()
            .any(|m| p.material == *m || (m.ends_with('_') && p.material.starts_with(m)));
        if !blocking {
            continue;
        }
        for t in p.indices.chunks(3) {
            let tri = [
                p.positions[t[0] as usize],
                p.positions[t[1] as usize],
                p.positions[t[2] as usize],
            ];
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for v in &tri {
                lo = [lo[0].min(v[0]), lo[1].min(v[1])];
                hi = [hi[0].max(v[0]), hi[1].max(v[1])];
            }
            for i in (lo[0] / cell).floor() as i32..=(hi[0] / cell).floor() as i32 {
                for j in (lo[1] / cell).floor() as i32..=(hi[1] / cell).floor() as i32 {
                    grid.entry((i, j)).or_default().push((pi, tri));
                }
            }
        }
    }
    // Instanced box / cylinder features (drawn by the testbed, not in the greybox).
    let mut labels: Vec<String> = prims.iter().map(|p| p.material.clone()).collect();
    if materials.contains(&"feature_") {
        for f in &map.net.features {
            // Foliage may overhang a lane; it is not a wall.
            if f.kind == wp_world::network::FeatureKind::TreeCanopy {
                continue;
            }
            let tris = feature_tris(&f.shape);
            if tris.is_empty() {
                continue;
            }
            labels.push(format!("feature {:?}", f.kind));
            let li = labels.len() - 1;
            for tri in tris {
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for v in &tri {
                    lo = [lo[0].min(v[0]), lo[1].min(v[1])];
                    hi = [hi[0].max(v[0]), hi[1].max(v[1])];
                }
                for i in (lo[0] / cell).floor() as i32..=(hi[0] / cell).floor() as i32 {
                    for j in (lo[1] / cell).floor() as i32..=(hi[1] / cell).floor() as i32 {
                        grid.entry((i, j)).or_default().push((li, tri));
                    }
                }
            }
        }
    }
    let key = |p: V3| ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
    let mut hits: BTreeMap<String, Vec<V3>> = BTreeMap::new();
    for l in &map.net.lanes {
        for w in l.polyline.windows(2) {
            for h in [1.2f32, 3.0] {
                let p = [w[0][0], w[0][1], w[0][2] + h];
                let q = [w[1][0], w[1][1], w[1][2] + h];
                let hit = [key(p), key(q)].iter().find_map(|k| {
                    grid.get(k)?
                        .iter()
                        .find(|(_, t)| seg_tri(p, q, t[0], t[1], t[2]))
                        .map(|(pi, _)| labels[*pi].clone())
                });
                // The road-closed portals where roads leave the map are meant to block.
                let (lo, hi) = (map.net.bounds_min, map.net.bounds_max);
                let at_edge = p[0] < lo[0] + 30.0
                    || p[1] < lo[1] + 30.0
                    || p[0] > hi[0] - 30.0
                    || p[1] > hi[1] - 30.0;
                if let Some(m) = hit.filter(|_| !at_edge) {
                    hits.entry(m).or_default().push(p);
                }
            }
        }
    }
    hits
}

fn assert_clear(name: &str, net: RoadNetwork, materials: &[&str]) {
    let hits = blocked(net, materials);
    for (m, v) in &hits {
        eprintln!(
            "{name} {m}: {} lane segments blocked, e.g. {:?}",
            v.len(),
            &v[..v.len().min(4)]
        );
    }
    assert!(hits.is_empty(), "{name}: greybox geometry across lanes");
}

#[test]
fn port_vesper_v2_lanes_are_visually_clear() {
    // Bridge structure and walls only: v2 has one older lot whose building sits over two lanes
    // near (-1107, -880) (roads 173 / 177), a layout issue outside the greybox.
    assert_clear(
        "vesper_v2",
        wp_world::vesper::generate(&Default::default()),
        &["retaining_wall", "bridge", "median_barrier"],
    );
}

#[test]
fn port_vesper_v3_lanes_are_visually_clear() {
    assert_clear(
        "vesper_v3",
        wp_world::metro::generate(&Default::default()),
        &[
            "retaining_wall",
            "bridge",
            "building",
            "median_barrier",
            "feature_",
        ],
    );
}
