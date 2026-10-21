//! Vesper Central: an elevated terminus (the passenger line arrives over the Central Rail
//! Bridge): a deck on piers, a throat fanning the double track out to six platform tracks,
//! seven platforms under a train-shed roof, passenger trains, and the station building at
//! street level facing the gate.

use wp_math::Rng;

use super::super::ctx::{box_on, cylinder, obb, prism, Ctx};
use super::super::ground::inside;
use super::super::plan::SiteKind;
use super::align::{RailAlignment, Support};
use super::cars::{self, CarKind};
use super::geo::{self, piece, Paving};
use super::track::{buffer_stop, lay_track, Output};
use crate::geom::{self, V2, V3};
use crate::network::{FeatureKind, Material};

/// Platform tracks.
const TRACKS: usize = 6;
/// Spacing of platform tracks (track + platform), m.
const PITCH_M: f32 = 10.5;
/// Platform width, m.
const PLATFORM_W_M: f32 = 6.0;
/// Platform height above the rail bed, m.
const PLATFORM_H_M: f32 = 0.9;
/// Length of the throat (double track → platform tracks), m.
const THROAT_M: f32 = 75.0;

/// Build every station whose line arrives on an elevated station deck.
pub fn build_station(
    ctx: &Ctx,
    lines: &[RailAlignment],
    paving: &Paving,
    rng: &mut Rng,
    out: &mut Output,
) {
    for site in ctx.plan.sites_of(SiteKind::Station) {
        let Some(a) = lines.iter().find(|a| {
            a.pts
                .iter()
                .zip(&a.support)
                .any(|(p, s)| *s == Support::StationDeck && inside(&site.polygon, geom::xy(*p)))
        }) else {
            continue;
        };
        let n = a.pts.len();
        let k0 = (0..n)
            .find(|&i| a.support[i] == Support::StationDeck)
            .unwrap_or(0);
        let (s0, s_end) = (a.cum[k0], a.length());
        let z = a.pts[k0][2];
        let p0 = geom::xy(a.pts[k0]);
        let dir = geom::normalize(geom::sub(geom::xy(a.pts[n - 1]), p0));
        let nrm = geom::right_of(dir);
        let yaw = geom::yaw_of(dir);
        let at = |s: f32, o: f32| -> V3 {
            let q = geo::along(geo::along(p0, dir, s - s0), nrm, o);
            [q[0], q[1], z]
        };
        let off = |k: usize| (k as f32 - (TRACKS as f32 - 1.0) * 0.5) * PITCH_M;
        let deck_half = (TRACKS as f32 * 0.5) * PITCH_M + 4.5;
        let inner = (a.tracks.max(1) as f32 - 1.0) * super::align::TRACK_SPACING_M * 0.5;

        // Deck slabs, parapets on the open edges, piers below.
        let mut s = s0 - 2.0;
        while s < s_end + 3.0 {
            let s2 = (s + 20.0).min(s_end + 3.0);
            out.add(piece(
                FeatureKind::RailBridge,
                Material::Concrete,
                at(s, 0.0),
                at(s2, 0.0),
                deck_half * 2.0,
                -1.2,
                0.0,
                0.05,
                true,
            ));
            for side in [-1.0f32, 1.0] {
                out.add(piece(
                    FeatureKind::RailBridge,
                    Material::Concrete,
                    at(s, side * (deck_half - 0.2)),
                    at(s2, side * (deck_half - 0.2)),
                    0.4,
                    0.0,
                    1.1,
                    0.1,
                    true,
                ));
            }
            for o in [-0.75f32, -0.25, 0.25, 0.75] {
                let c = geom::xy(at(s + 10.0, o * deck_half * 1.6));
                let g = ctx.ground_z(c);
                let fp = obb(c, [1.2, 1.2], 0.0);
                if z - 1.2 - g > 1.0 && !paving.hits_any(&fp, 1.0) {
                    out.add(cylinder(
                        FeatureKind::RailBridge,
                        Material::Concrete,
                        [c[0], c[1], g - 0.5],
                        0.9,
                        z - 1.2 - g + 0.5,
                        true,
                    ));
                }
            }
            s = s2;
        }
        // End wall and the start edge beside the arriving viaduct.
        out.add(box_on(
            FeatureKind::RailBridge,
            Material::Concrete,
            geom::xy(at(s_end + 2.8, 0.0)),
            z,
            [deck_half * 2.0, 0.4, 1.1],
            yaw,
            true,
        ));
        let via_half = a.half_span_m() + 0.7;
        for side in [-1.0f32, 1.0] {
            let w = deck_half - via_half;
            out.add(box_on(
                FeatureKind::RailBridge,
                Material::Concrete,
                geom::xy(at(s0 - 1.8, side * (via_half + w * 0.5))),
                z,
                [w, 0.4, 1.1],
                yaw,
                true,
            ));
        }

        // Platform tracks through the throat; buffer stops; trains in three of them.
        let mut used = [false; TRACKS];
        let mut picks = 0;
        while picks < 3 {
            let k = rng.below(TRACKS as u32) as usize;
            if !used[k] {
                used[k] = true;
                picks += 1;
            }
        }
        for (k, busy) in used.iter().enumerate() {
            let o = off(k);
            let start = at(s0, o.signum() * inner);
            let fan = at(s0 + THROAT_M, o);
            let end = at(s_end - 2.0, o);
            let mut line = sample(start, fan);
            line.extend(sample(fan, end).into_iter().skip(1));
            lay_track(out, &line, &vec![true; line.len()]);
            buffer_stop(out, paving, end, dir);
            if *busy {
                let train = [
                    CarKind::PowerCar,
                    CarKind::Coach,
                    CarKind::Coach,
                    CarKind::Coach,
                    CarKind::Coach,
                    CarKind::Coach,
                    CarKind::PowerCar,
                ];
                let cum = geom::cumulative(&line);
                let len = *cum.last().unwrap_or(&0.0);
                let start_s = len - 4.0 - cars::consist_length(&train);
                cars::park(
                    &line,
                    &cum,
                    start_s.max(THROAT_M),
                    &train,
                    rng,
                    &mut out.features,
                );
            }
        }

        // Platforms, shed columns and roof.
        let (ps0, ps1) = (s0 + THROAT_M + 5.0, s_end - 3.0);
        for k in 0..=TRACKS {
            let o = off(k) - PITCH_M * 0.5;
            out.add(piece(
                FeatureKind::Platform,
                Material::Concrete,
                at(ps0, o),
                at(ps1, o),
                PLATFORM_W_M,
                0.0,
                PLATFORM_H_M,
                0.0,
                true,
            ));
            let mut s = ps0 + 6.0;
            while s < ps1 {
                let c = geom::xy(at(s, o));
                out.add(cylinder(
                    FeatureKind::Platform,
                    Material::Steel,
                    [c[0], c[1], z + PLATFORM_H_M],
                    0.35,
                    9.6 - PLATFORM_H_M,
                    true,
                ));
                s += 30.0;
            }
        }
        let mut s = ps0;
        while s < ps1 {
            let s2 = (s + 30.0).min(ps1);
            let c = geom::xy(at((s + s2) * 0.5, 0.0));
            out.add(box_on(
                FeatureKind::Platform,
                Material::Glass,
                c,
                z + 9.6,
                [deck_half * 2.0 - 4.0, s2 - s, 0.4],
                yaw,
                false,
            ));
            out.add(box_on(
                FeatureKind::Platform,
                Material::Steel,
                c,
                z + 10.0,
                [deck_half * 2.0 - 4.0, 1.0, 1.2],
                yaw,
                false,
            ));
            s = s2;
        }

        // Station building at street level on the gate side.
        let gate = site
            .gates
            .first()
            .map(|g| g.pos)
            .unwrap_or(geom::xy(at(s0, 1.0)));
        let gs = if geom::dot(geom::sub(gate, p0), nrm) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let rect = |sa: f32, sb: f32, oa: f32, ob: f32| -> Vec<V2> {
            vec![
                geom::xy(at(sa, gs * oa)),
                geom::xy(at(sb, gs * oa)),
                geom::xy(at(sb, gs * ob)),
                geom::xy(at(sa, gs * ob)),
            ]
        };
        let gate_d = geom::dot(geom::sub(gate, p0), nrm).abs();
        let front = (gate_d - 14.0).min(deck_half + 46.0);
        let back = deck_half + 6.0;
        let len = s_end - s0;
        let parts: [(f32, f32, f32, f32, f32, Material); 5] = [
            // (along from, along to, lateral from, lateral to, height, material)
            (0.30, 0.80, back, front, 15.0, Material::Brick),
            (0.18, 0.30, back, front - 6.0, 10.0, Material::Brick),
            (0.80, 0.92, back, front - 6.0, 10.0, Material::Brick),
            (
                0.48,
                0.62,
                front - 1.0,
                front + 5.0,
                9.0,
                Material::Concrete,
            ),
            (0.24, 0.30, front - 12.0, front - 2.0, 32.0, Material::Brick),
        ];
        if front > back + 12.0 {
            for (fa, fb, la, lb, h, m) in parts {
                let fp = rect(s0 + len * fa, s0 + len * fb, la, lb);
                if paving.hits_any(&fp, 1.0) || !fp.iter().all(|p| inside(&site.polygon, *p)) {
                    continue;
                }
                let (lo, _) = ctx.ground_range(&fp);
                out.add(prism(FeatureKind::Landmark, m, fp, lo - 0.3, lo + h, true));
            }
        }
    }
}

/// Sample a straight track every 5 m.
fn sample(a: V3, b: V3) -> Vec<V3> {
    let n =
        ((geom::dist(geom::xy(a), geom::xy(b)) / super::align::SAMPLE_M).ceil() as usize).max(1);
    (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        })
        .collect()
}
