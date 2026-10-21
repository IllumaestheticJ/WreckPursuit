//! Design checks of a generated road network (used by the tests and the stats example):
//! grades within each road's cap, clearance at every grade separation, no lane under water
//! outside tunnels, terrain never above an at-grade road.

use super::super::plan;
use super::{RoadKind, RoadsOut};
use crate::geom::{self, V2};
use crate::surface::is_elevated;
use crate::terrain::Heightfield;

/// One violated rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// Rule (`grade`, `clearance`, `water`, `terrain`).
    pub rule: &'static str,
    /// Road (spec index).
    pub road: usize,
    /// Where, m.
    pub pos: V2,
    /// Measured value (grade, clearance, height).
    pub value: f32,
}

/// Arc position of `q` along the surface of `road` (its trimmed centreline).
fn covered_at(out: &RoadsOut, road: usize, q: V2) -> bool {
    let cov = &out.info[road].covered;
    if cov.is_empty() {
        return false;
    }
    // Covered ranges are measured along the spec centreline (node to node).
    let sp = &out.specs[road];
    let mut p = vec![out.nodes[sp.a].pos];
    p.extend_from_slice(&sp.via);
    p.push(out.nodes[sp.b].pos);
    let c = super::geo::cum2(&p);
    let (s, _) = super::geo::project2(&p, &c, q);
    cov.iter().any(|(a, b)| s >= a - 30.0 && s <= b + 30.0)
}

/// Every violation of the design rules.
pub fn violations(out: &RoadsOut, terrain: &Heightfield) -> Vec<Violation> {
    let mut v = Vec::new();
    let built = &out.built;
    for s in &built.surfaces {
        let r = s.road as usize;
        let cap = out.info[r].grade;
        let cum = geom::cumulative(&s.centre);
        for k in 0..s.centre.len() - 1 {
            let ds = (cum[k + 1] - cum[k]).max(0.5);
            let g = (s.centre[k + 1][2] - s.centre[k][2]) / ds;
            if g.abs() > cap + 0.005 {
                v.push(Violation {
                    rule: "grade",
                    road: r,
                    pos: geom::xy(s.centre[k]),
                    value: g,
                });
            }
        }
        // Terrain under at-grade paving.
        for k in 0..s.centre.len() {
            if is_elevated(s, k, &|q| terrain.height_at(q), 1.0) {
                continue;
            }
            let p = s.centre[k];
            let tan = if k + 1 < s.centre.len() {
                geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), geom::xy(p)))
            } else {
                geom::normalize(geom::sub(geom::xy(p), geom::xy(s.centre[k - 1])))
            };
            let rt = geom::right_of(tan);
            for lat in [-s.left_m, 0.0, s.right_m] {
                let q = geom::add(geom::xy(p), geom::scale(rt, lat));
                let road = s.z_at(k, lat);
                let gz = terrain.height_at(q);
                if gz > road + 0.03 {
                    v.push(Violation {
                        rule: "terrain",
                        road: r,
                        pos: q,
                        value: gz - road,
                    });
                }
            }
        }
    }
    for c in &out.crossings {
        if c.clearance < 7.5 {
            v.push(Violation {
                rule: "clearance",
                road: c.lower,
                pos: c.pos,
                value: c.clearance,
            });
        }
    }
    for l in &built.lanes {
        let r = l.road as usize;
        for p in &l.polyline {
            if p[2] < plan::WATER_Z + 1.0 - 1e-3 && !covered_at(out, r, geom::xy(*p)) {
                v.push(Violation {
                    rule: "water",
                    road: r,
                    pos: geom::xy(*p),
                    value: p[2],
                });
                break;
            }
        }
    }
    let _ = RoadKind::Street;
    v
}

/// One-line summary per rule: count and the worst few with road names.
pub fn summary(out: &RoadsOut, v: &[Violation]) -> Vec<String> {
    let mut lines = Vec::new();
    for rule in ["grade", "clearance", "water", "terrain"] {
        let mut list: Vec<&Violation> = v.iter().filter(|x| x.rule == rule).collect();
        if list.is_empty() {
            continue;
        }
        list.sort_by(|a, b| b.value.abs().total_cmp(&a.value.abs()));
        let worst: Vec<String> = list
            .iter()
            .take(6)
            .map(|x| {
                format!(
                    "{} {:.2} at ({:.0},{:.0})",
                    out.names[x.road], x.value, x.pos[0], x.pos[1]
                )
            })
            .collect();
        lines.push(format!(
            "check {rule}: {} — {}",
            list.len(),
            worst.join("; ")
        ));
    }
    lines
}
