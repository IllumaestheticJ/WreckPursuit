//! Route 7, Route 12 and the Crosstown Expressway with their interchanges, the Talon Tunnel and the Silver Gorge Bridge.

use super::super::*;

/// Lay out this district's roads.
pub(super) fn layout(_g: &mut Gen) {}

/// Covered roads (tunnels) on this district's roads.
pub(super) fn covers() -> Vec<CoverSpec> {
    Vec::new()
}

/// This district's zones.
pub(super) fn zones() -> Vec<Zone> {
    Vec::new()
}

/// Whether the generic street-front building pass may build at `p`.
pub(super) fn generic_buildings_at(_p: V2) -> bool {
    true
}

/// Hand-placed buildings and decor.
pub(super) fn extra_buildings(_site: &mut Site) -> (Vec<Building>, Vec<Decor>) {
    (Vec::new(), Vec::new())
}

/// Points of interest.
pub(super) fn pois(_site: &mut Site) -> Vec<Poi> {
    Vec::new()
}

#[cfg(test)]
mod probe {
    use super::super::plan;
    #[test]
    fn probe_ground() {
        let f = super::super::terrain::natural(1);
        for (name, line) in [
            ("R7", plan::ROUTE_7),
            ("R12", plan::ROUTE_12),
            ("CT", plan::CROSSTOWN),
        ] {
            println!("== {name}");
            let mut acc = 0.0;
            for w in line.windows(2) {
                let l = crate::geom::dist(w[0], w[1]);
                let n = (l / 50.0).ceil() as usize;
                for k in 0..n {
                    let t = k as f32 / n as f32;
                    let p = [
                        w[0][0] + (w[1][0] - w[0][0]) * t,
                        w[0][1] + (w[1][1] - w[0][1]) * t,
                    ];
                    println!(
                        "s {:6.0} p [{:6.0},{:6.0}] z {:6.1}",
                        acc + l * t,
                        p[0],
                        p[1],
                        f(p, 20.0)
                    );
                }
                acc += l;
            }
        }
    }
}
