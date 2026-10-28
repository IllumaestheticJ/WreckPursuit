//! Probe: physics ground heights around every covered span (tunnels) of the combined world,
//! for finding drops beside decks, portals and walls (owner playtest 2026-10-27, "massive
//! drop on the side of the Loop 9 tunnel").
//! `cargo run --release -p wp_sim --example tunnel_drop_probe` prints, per span, the cells
//! where the ground falls more than 3 m within 2 m (bucketed by arc length / lateral offset)
//! and writes a 1 m JSON height grid per span to `OUT` (default the system temp dir).
//! `SPAN="Hollins Tunnel"` limits it to one span; `SECTIONS=1` prints cross-sections (raw
//! terrain / collision terrain / physics); `POKES=1` prints the worst terrain poke into each
//! tube.
use std::fmt::Write as _;
use wp_sim::PhysicsWorld;
use wp_world::covered::CoverShape;
use wp_world::World;

fn main() {
    let out = std::env::var("OUT")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    let only = std::env::var("SPAN").ok();
    let w = World::ashpeak(
        &wp_world::ashpeak::default_config(),
        wp_world::WorldConfig::default(),
    );
    let map = w.map();
    let mut phys = PhysicsWorld::new();
    phys.set_city_ground(map);
    if std::env::var("POKES").is_ok() {
        pokes(map);
        return;
    }
    for span in &map.net.covered {
        if only.as_deref().is_some_and(|n| n != span.name) {
            continue;
        }
        if std::env::var("SECTIONS").is_ok() {
            sections(map, &phys, span);
        }
        let sh = CoverShape::new(span);
        let pad = 120.0;
        let (lo, hi) = (sh.bounds[0], sh.bounds[1]);
        let (x0, y0, x1, y1) = (lo[0] - pad, lo[1] - pad, hi[0] + pad, hi[1] + pad);
        let cell = 1.0f32;
        let nx = ((x1 - x0) / cell) as usize;
        let ny = ((y1 - y0) / cell) as usize;
        let ztop = sh.z_bounds[1] + 200.0;
        let mut z = vec![f32::NAN; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                let p = [x0 + i as f32 * cell, y0 + j as f32 * cell];
                if let Some(h) = phys.static_ground_z(p, ztop, 600.0) {
                    z[j * nx + i] = h;
                }
            }
        }
        // Drops: max neighbour height difference within 2 m.
        let mut worst: Vec<(f32, f32, f32, f32)> = Vec::new();
        for j in 2..ny - 2 {
            for i in 2..nx - 2 {
                let a = z[j * nx + i];
                let mut d = 0.0f32;
                for (di, dj) in [(2i32, 0i32), (0, 2), (-2, 0), (0, -2)] {
                    let b = z[(j as i32 + dj) as usize * nx + (i as i32 + di) as usize];
                    d = d.max(a - b);
                }
                if d > 3.0 {
                    let p = [x0 + i as f32 * cell, y0 + j as f32 * cell];
                    let q = sh.locate(p);
                    worst.push((d, q.s, q.lateral, a - q.road_z));
                }
            }
        }
        println!(
            "{}: road {} len {:.0} half {:.1} clr {:.1}; {} drop cells >3 m",
            span.name,
            span.road,
            sh.length(),
            span.half_width_m,
            span.clearance_m,
            worst.len()
        );
        let mut s = String::new();
        let _ = write!(s, "{{\"name\":\"{}\",\"x0\":{x0},\"y0\":{y0},\"cell\":{cell},\"nx\":{nx},\"ny\":{ny},\"z\":[", span.name);
        for (k, v) in z.iter().enumerate() {
            if k > 0 {
                s.push(',');
            }
            if v.is_nan() {
                s.push_str("null");
            } else {
                let _ = write!(s, "{v:.2}");
            }
        }
        s.push_str("],\"centre\":[");
        for (k, p) in span.centre.iter().enumerate() {
            if k > 0 {
                s.push(',');
            }
            let _ = write!(s, "[{:.1},{:.1},{:.2}]", p[0], p[1], p[2]);
        }
        s.push_str("],\"lanes\":[");
        let mut first = true;
        for sf in &map.net.surfaces {
            let c = &sf.centre;
            if !c
                .iter()
                .any(|p| p[0] > x0 && p[0] < x1 && p[1] > y0 && p[1] < y1)
            {
                continue;
            }
            if !first {
                s.push(',');
            }
            first = false;
            let _ = write!(s, "[{:.1},{:.1},", sf.left_m, sf.right_m);
            for (k, p) in c.iter().enumerate() {
                if k > 0 {
                    s.push(',');
                }
                let _ = write!(s, "[{:.1},{:.1}]", p[0], p[1]);
            }
            s.push(']');
        }
        s.push_str("]}");
        let f = format!("{out}/span_{}.json", span.name.replace(' ', "_"));
        std::fs::write(&f, s).unwrap();
        // Bucket drops by (s, lateral) coarse.
        let mut buckets: std::collections::BTreeMap<(i32, i32), (usize, f32, f32)> =
            Default::default();
        for (d, s, l, rz) in &worst {
            let e = buckets
                .entry(((s / 20.0).floor() as i32, (l / 10.0).floor() as i32))
                .or_insert((0, 0.0, 0.0));
            e.0 += 1;
            e.1 = e.1.max(*d);
            e.2 = e.2.max(*rz);
        }
        for ((bs, bl), (n, d, rz)) in buckets {
            println!("  s {:>5}..{:<5} lat {:>4}..{:<4}: {n:>4} cells, max drop {d:.1} m, top {rz:.1} m above road", bs * 20, bs * 20 + 20, bl * 10, bl * 10 + 10);
        }
    }
}

/// Cross-sections: fine terrain vs collision terrain vs physics.
#[allow(dead_code)]
pub fn sections(
    map: &wp_world::CityMap,
    phys: &PhysicsWorld,
    span: &wp_world::network::CoveredSpan,
) {
    let col = wp_world::surface::collision_terrain(map).unwrap();
    let fine = map.net.terrain.as_ref().unwrap();
    println!(
        "cells fine {} coarse spacing {}",
        fine.cell_m,
        wp_world::surface::drawn_spacing(map, fine)
    );
    for k in [5usize, 25, 50] {
        let (a, b) = (span.centre[k], span.centre[k + 1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt();
        let r = [dy / l, -dx / l];
        let mut line = format!("k{k}:");
        for lat in (-60..=60).step_by(4) {
            let p = [a[0] + r[0] * lat as f32, a[1] + r[1] * lat as f32];
            let f = fine.height_at(p) - a[2];
            let c = col.height_at(p) - a[2];
            let ph = phys
                .static_ground_z(p, a[2] + 200.0, 400.0)
                .unwrap_or(f32::NAN)
                - a[2];
            line += &format!(" {lat}:{f:.1}/{c:.1}/{ph:.1}");
        }
        println!("{line}");
    }
}

/// Worst terrain poke into each tube (collision terrain above the road surface level inside
/// the walls), sampled every 1 m.
#[allow(dead_code)]
pub fn pokes(map: &wp_world::CityMap) {
    let col = wp_world::surface::collision_terrain(map).unwrap();
    let drawn = wp_world::surface::drawn_terrain_mesh(map).unwrap();
    for span in &map.net.covered {
        let sh = CoverShape::new(span);
        let (mut worst_c, mut worst_d) = ((0.0f32, 0.0, 0.0), (0.0f32, 0.0, 0.0));
        let mut s = 0.0;
        while s <= sh.length() {
            let mut lat = -span.half_width_m;
            while lat <= span.half_width_m {
                let (pp, d) = wp_world::geom::point_at(&span.centre, &sh.cum, s);
                let r = [d[1], -d[0]];
                let x = [pp[0] + r[0] * lat, pp[1] + r[1] * lat];
                let c = col.height_at(x) - pp[2];
                let dr = drawn.height_at(x) - pp[2];
                if c > worst_c.0 {
                    worst_c = (c, s, lat);
                }
                if dr > worst_d.0 {
                    worst_d = (dr, s, lat);
                }
                lat += 1.0;
            }
            s += 1.0;
        }
        println!(
            "{}: solid poke {:.2} m at s {:.0} lat {:.0}; drawn poke {:.2} m at s {:.0} lat {:.0}",
            span.name, worst_c.0, worst_c.1, worst_c.2, worst_d.0, worst_d.1, worst_d.2
        );
    }
}
