//! Loop 9 vertical profile: grade changes per lane chain (owner 2026-10-27: "right after one of
//! the bumps, I think I'm clipping into the ground").
use wp_world::geom;
use wp_world::vesper::ashpeak;

fn main() {
    let (net, report) = ashpeak::generate_with_report(&ashpeak::default_config());
    let names: Vec<String> = report.road_names.iter().map(|n| n.to_string()).collect();
    for (i, l) in net.lanes.iter().enumerate() {
        if names.get(l.road as usize).map(|s| s.as_str()) != Some("Loop 9")
            || l.index_from_left != 0
        {
            continue;
        }
        let cum = geom::cumulative(&l.polyline);
        let len = cum[cum.len() - 1];
        let step = 2.0;
        let mut s = step;
        while s < len - step {
            let z = |s: f32| geom::point_at(&l.polyline, &cum, s).0[2];
            let g0 = (z(s) - z(s - step)) / step;
            let g1 = (z(s + step) - z(s)) / step;
            if (g1 - g0).abs() > 0.01 {
                let p = geom::point_at(&l.polyline, &cum, s).0;
                println!("lane {i} s={s:.0} at ({:.0},{:.0},{:.2}) grade {:.3} -> {:.3} (change {:+.3} per {step} m)", p[0], p[1], p[2], g0, g1, g1 - g0);
            }
            s += step;
        }
    }
}
