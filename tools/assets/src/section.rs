//! Per-model section specs (painted-car pipeline): the body shape of a real car family as
//! measured curves, so the loft follows the car's own side view, plan view and sections instead
//! of the generic lattice-driven knobs.
//!
//! A family file `data/panels/models/<family>.json` ([`SectionSpec`]) holds, in chassis metres
//! (+Y forward, +Z up, origin mid-wheelbase on the ground, the left half):
//! - `anchors`: nose / tail, windshield base, roof front / rear, rear-glass base (deck start),
//!   bumper tops;
//! - `lines` (side view, `[y, z]`): `sill` (the underside edge: chin, rocker, rear valance),
//!   `belt` (the top of the side: nose leading edge, fender / hood edge, door top, deck edge,
//!   tail top), `greenhouse` (the centre-line profile from the windshield base over the roof to
//!   the rear-glass base);
//! - `plan` (plan view, `[y, half width]`): `half_width` (the body side before flares),
//!   `haunch` (blistered fenders: extra outward offset of the side under the shoulder) and
//!   `roof_half_width` (the greenhouse at roof height);
//! - `keyframes`: per-station section knobs (sill tuck, shoulder rounding, crease height /
//!   depth, glass inset), each interpolated along y over the keyframes that set it;
//! - `trims`: per-trim deltas (curve overlays, ride height, flares, wing, wheels, paint,
//!   texture layers), read by the model style ([`crate::styles::models`]).
//!
//! Curves are monotone cubic (Fritsch–Carlson) through their points, held flat past the ends,
//! so they never overshoot near arches or ends. The profile ([`crate::profile::BodyProfile`])
//! consults the spec first when [`crate::style::BodyStyle::sections`] is set; every other car
//! is untouched. [`adaptive_stations`] places loft stations by curvature.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A curve through `[x, y]` points (x strictly increasing), evaluated as a monotone cubic.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Curve(pub Vec<[f32; 2]>);

impl Curve {
    /// True when the curve has no points.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// x range (first, last), or `None` when empty.
    pub fn span(&self) -> Option<(f32, f32)> {
        Some((self.0.first()?[0], self.0.last()?[0]))
    }

    /// Largest y of the points (0 when empty).
    pub fn max_y(&self) -> f32 {
        self.0
            .iter()
            .map(|q| q[1])
            .fold(f32::MIN, f32::max)
            .max(0.0)
    }

    /// Smallest y of the points (0 when empty).
    pub fn min_y(&self) -> f32 {
        if self.0.is_empty() {
            return 0.0;
        }
        self.0.iter().map(|q| q[1]).fold(f32::MAX, f32::min)
    }

    /// Value at `x`: monotone cubic Hermite between points (Fritsch–Carlson tangents), the end
    /// values past either end, 0 for an empty curve. Never panics (NaN-free for finite input).
    pub fn eval(&self, x: f32) -> f32 {
        let p = &self.0;
        match p.len() {
            0 => return 0.0,
            1 => return p[0][1],
            _ => {}
        }
        if x <= p[0][0] {
            return p[0][1];
        }
        let n = p.len();
        if x >= p[n - 1][0] {
            return p[n - 1][1];
        }
        // Segment k with p[k].x <= x < p[k + 1].x.
        let mut k = 0;
        while k + 2 < n && x >= p[k + 1][0] {
            k += 1;
        }
        let slope = |i: usize| {
            let dx = (p[i + 1][0] - p[i][0]).max(1e-6);
            (p[i + 1][1] - p[i][1]) / dx
        };
        // Fritsch–Carlson tangent at point i.
        let tangent = |i: usize| -> f32 {
            if i == 0 {
                return slope(0);
            }
            if i == n - 1 {
                return slope(n - 2);
            }
            let (d0, d1) = (slope(i - 1), slope(i));
            if d0 * d1 <= 0.0 {
                return 0.0;
            }
            // Weighted harmonic mean (Fritsch–Butland), always inside the monotone region.
            let h0 = p[i][0] - p[i - 1][0];
            let h1 = p[i + 1][0] - p[i][0];
            let w1 = 2.0 * h1 + h0;
            let w2 = h1 + 2.0 * h0;
            (w1 + w2) / (w1 / d0 + w2 / d1)
        };
        let (x0, y0) = (p[k][0], p[k][1]);
        let (x1, y1) = (p[k + 1][0], p[k + 1][1]);
        let h = (x1 - x0).max(1e-6);
        let d = slope(k);
        let mut m0 = tangent(k);
        let mut m1 = tangent(k + 1);
        // End tangents and flat segments: keep the cubic inside the segment's range.
        if d == 0.0 {
            m0 = 0.0;
            m1 = 0.0;
        } else {
            let (a, b) = (m0 / d, m1 / d);
            if a < 0.0 {
                m0 = 0.0;
            }
            if b < 0.0 {
                m1 = 0.0;
            }
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m0 = t * a * d;
                m1 = t * b * d;
            }
        }
        let t = (x - x0) / h;
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * y0 + h10 * h * m0 + h01 * y1 + h11 * h * m1
    }

    /// The curve with `over`'s points replacing every point inside `over`'s x span (a trim
    /// overlay, e.g. a deeper front bumper chin). An empty overlay returns the curve unchanged.
    pub fn overlaid(&self, over: &Curve) -> Curve {
        let Some((a, b)) = over.span() else {
            return self.clone();
        };
        let mut pts: Vec<[f32; 2]> = self
            .0
            .iter()
            .copied()
            .filter(|q| q[0] < a - 1e-4 || q[0] > b + 1e-4)
            .collect();
        pts.extend(over.0.iter().copied());
        pts.sort_by(|p, q| p[0].total_cmp(&q[0]));
        Curve(pts)
    }

    /// The curve moved by `dx` along x and `dy` along y.
    pub fn shifted(&self, dx: f32, dy: f32) -> Curve {
        Curve(self.0.iter().map(|q| [q[0] + dx, q[1] + dy]).collect())
    }

    /// Problems with the points (fewer than 2, x not strictly increasing, non-finite).
    pub fn check(&self, name: &str, errs: &mut Vec<String>) {
        if self.0.len() < 2 {
            errs.push(format!("{name}: needs at least 2 points"));
        }
        for w in self.0.windows(2) {
            if w[1][0] <= w[0][0] {
                errs.push(format!("{name}: x not strictly increasing at {}", w[1][0]));
            }
        }
        if self.0.iter().flatten().any(|v| !v.is_finite()) {
            errs.push(format!("{name}: non-finite value"));
        }
    }
}

/// Key stations and heights of a section spec (chassis metres).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchors {
    /// Nose (front face) y.
    pub y_nose: f32,
    /// Tail (rear face) y.
    pub y_tail: f32,
    /// Windshield base (hood rear edge / cowl) y.
    pub y_windshield_base: f32,
    /// Roof front edge (windshield top) y.
    pub y_roof_front: f32,
    /// Roof rear edge (rear-glass top) y.
    pub y_roof_rear: f32,
    /// Rear-glass base = trunk lid front edge y.
    pub y_deck: f32,
    /// Front, rear bumper cover tops (z): above them the nose / tail faces are fender / hood /
    /// trunk.
    pub z_bumper_top: [f32; 2],
    /// Door skin bottom (z): below it the side is the sill. `None` = the lattice value.
    #[serde(default)]
    pub z_door_bottom: Option<f32>,
    /// Quarter (rear side) window y span (rear end, front end) on cars whose quarter glass sits
    /// under the sloping C-pillar behind the door. `None` = the generic placement.
    #[serde(default)]
    pub quarter_glass_y: Option<[f32; 2]>,
}

/// Side-view lines of a section spec (`[y, z]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lines {
    /// Underside edge (chin, rocker, rear valance bottom).
    pub sill: Curve,
    /// Top of the side: nose leading edge, fender / hood edge, door top, deck edge, tail top.
    pub belt: Curve,
    /// Centre-line greenhouse profile from the windshield base to the rear-glass base
    /// (windshield, roof, rear glass). Outside its span the top is the belt (+ crowns).
    pub greenhouse: Curve,
}

/// Plan-view curves of a section spec (`[y, half width]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Body half width (the door / bumper side, before flares).
    pub half_width: Curve,
    /// Extra outward offset of the side under the shoulder (blistered fenders), m. Empty = the
    /// style's `haunch` / `front_haunch` knobs.
    #[serde(default)]
    pub haunch: Curve,
    /// Greenhouse half width at roof height.
    pub roof_half_width: Curve,
}

/// Section knobs at one station (each optional: a knob is interpolated over the keyframes that
/// set it; with none, the style's value is used everywhere).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    /// Station y.
    pub y_m: f32,
    /// Underside chamfer, m (`BodyStyle::sill_tuck`).
    #[serde(default)]
    pub sill_tuck_m: Option<f32>,
    /// Shoulder rounding at the belt, m (`BodyStyle::shoulder_round`).
    #[serde(default)]
    pub shoulder_round_m: Option<f32>,
    /// Character crease height above the ground, m (absolute, unlike `BodyStyle::crease_z`).
    #[serde(default)]
    pub crease_z_m: Option<f32>,
    /// Character crease depth, m (`BodyStyle::crease_depth`).
    #[serde(default)]
    pub crease_depth_m: Option<f32>,
    /// Side-glass base inset from the body side, m (`BodyStyle::glass_inset`).
    #[serde(default)]
    pub glass_inset_m: Option<f32>,
}

/// Rear wing of a trim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WingKind {
    /// No wing.
    #[default]
    None,
    /// Small lip on the trunk lid's trailing edge.
    Lip,
    /// Single element on two mid-height pedestals.
    Pedestal,
    /// Two-element wing on tall stalks (the adjustable lower blade under the main plane).
    TwoElement,
}

/// Painted wheel design of a trim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrimWheel {
    /// Spoke pattern painted on the rim face: `"five_double"` (5 pairs), `"five"`, `"six"`,
    /// `"multi"` (10 thin).
    pub pattern: String,
    /// Rim diameter, inches.
    pub diameter_in: f32,
    /// Rim finish (sRGB).
    pub rgb: [u8; 3],
}

/// Per-trim deltas of a family (curve overlays and the trim's own kit / paint).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trim {
    /// Display label (docs only).
    #[serde(default)]
    pub label: String,
    /// Side-view overlays: points replacing the base curve inside their own y span.
    #[serde(default)]
    pub sill: Curve,
    /// Belt overlay.
    #[serde(default)]
    pub belt: Curve,
    /// Greenhouse overlay.
    #[serde(default)]
    pub greenhouse: Curve,
    /// Half-width overlay.
    #[serde(default)]
    pub half_width: Curve,
    /// Haunch (fender blister) overlay.
    #[serde(default)]
    pub haunch: Curve,
    /// Ride height offset, m (every z moves; − = lower).
    #[serde(default)]
    pub ride_dz_m: f32,
    /// Wheel-arch lip flare, m (`BodyStyle::flare`). `None` = the family default.
    #[serde(default)]
    pub flare_m: Option<f32>,
    /// Arch clearance over the tire, m. `None` = the family default.
    #[serde(default)]
    pub arch_gap_m: Option<f32>,
    /// Rear wing.
    #[serde(default)]
    pub wing: WingKind,
    /// NACA duct on the bonnet (painted).
    #[serde(default)]
    pub hood_duct: bool,
    /// Painted wheel design.
    #[serde(default)]
    pub wheel: Option<TrimWheel>,
    /// Body paint (sRGB).
    #[serde(default)]
    pub paint_rgb: Option<[u8; 3]>,
    /// Named texture layers the painter draws for this trim (badges, intakes, vents …; see
    /// `paint::features`).
    #[serde(default)]
    pub texture_layers: Vec<String>,
}

/// A family's section spec (see the module docs). Load with [`SectionSpec::load`], pick a trim
/// and fit it to a car with [`SectionSpec::for_car`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionSpec {
    /// Family id (= file stem).
    pub family: String,
    /// Free notes (sources, what is measured / estimated).
    #[serde(default)]
    pub notes: Vec<String>,
    /// Front and rear axle y of the coordinates below (the spec is moved so they meet the
    /// car's axles).
    pub axles_y_m: [f32; 2],
    /// Key stations.
    pub anchors: Anchors,
    /// Side-view lines.
    pub lines: Lines,
    /// Plan-view curves.
    pub plan: Plan,
    /// Section knobs along y.
    #[serde(default)]
    pub keyframes: Vec<Keyframe>,
    /// Trims by key.
    #[serde(default)]
    pub trims: BTreeMap<String, Trim>,
    /// The trim this spec was fitted for ([`SectionSpec::for_car`] callers set it; empty in a
    /// family file). Not serialised.
    #[serde(skip)]
    pub active_trim: String,
}

/// Section knobs resolved at one station ([`SectionSpec::knobs_at`]; `None` = the style's).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Knobs {
    /// Underside chamfer, m.
    pub sill_tuck: Option<f32>,
    /// Shoulder rounding, m.
    pub shoulder_round: Option<f32>,
    /// Crease height above ground, m.
    pub crease_z_m: Option<f32>,
    /// Crease depth, m.
    pub crease_depth: Option<f32>,
    /// Glass inset, m.
    pub glass_inset: Option<f32>,
}

/// Path of a family's section spec.
pub fn family_path(root: &Path, family: &str) -> std::path::PathBuf {
    root.join(format!("data/panels/models/{family}.json"))
}

impl SectionSpec {
    /// Load and check `data/panels/models/<family>.json`.
    pub fn load(root: &Path, family: &str) -> Result<SectionSpec, String> {
        let path = family_path(root, family);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let spec: SectionSpec =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let errs = spec.validate();
        if !errs.is_empty() {
            return Err(format!("{}: {}", path.display(), errs.join("; ")));
        }
        Ok(spec)
    }

    /// Every problem with the spec (empty = valid): curve points, anchor order, coverage.
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        self.lines.sill.check("lines.sill", &mut errs);
        self.lines.belt.check("lines.belt", &mut errs);
        self.lines.greenhouse.check("lines.greenhouse", &mut errs);
        self.plan.half_width.check("plan.half_width", &mut errs);
        self.plan
            .roof_half_width
            .check("plan.roof_half_width", &mut errs);
        if !self.plan.haunch.is_empty() {
            self.plan.haunch.check("plan.haunch", &mut errs);
        }
        let a = &self.anchors;
        let order = [
            ("y_tail", a.y_tail),
            ("y_deck", a.y_deck),
            ("y_roof_rear", a.y_roof_rear),
            ("y_roof_front", a.y_roof_front),
            ("y_windshield_base", a.y_windshield_base),
            ("y_nose", a.y_nose),
        ];
        for w in order.windows(2) {
            if w[1].1 <= w[0].1 + 0.05 {
                errs.push(format!(
                    "anchors: {} ({}) must lie ≥ 5 cm ahead of {} ({})",
                    w[1].0, w[1].1, w[0].0, w[0].1
                ));
            }
        }
        if self.axles_y_m[0] <= self.axles_y_m[1] + 1.0 {
            errs.push("axles_y_m: front axle must be > 1 m ahead of the rear".into());
        }
        if let Some((g0, g1)) = self.lines.greenhouse.span() {
            if g0 > a.y_deck + 1e-3 || g1 < a.y_windshield_base - 1e-3 {
                errs.push("lines.greenhouse must span y_deck .. y_windshield_base".into());
            }
        }
        for (name, c) in [
            ("lines.sill", &self.lines.sill),
            ("lines.belt", &self.lines.belt),
            ("plan.half_width", &self.plan.half_width),
        ] {
            if let Some((c0, c1)) = c.span() {
                if c0 > a.y_tail + 1e-3 || c1 < a.y_nose - 1e-3 {
                    errs.push(format!("{name} must span y_tail .. y_nose"));
                }
            }
        }
        // Heights: the side keeps ≥ 12 cm between the underside and the belt.
        let n = 40;
        for i in 0..=n {
            let y = a.y_tail + (a.y_nose - a.y_tail) * i as f32 / n as f32;
            let (s, b) = (self.lines.sill.eval(y), self.lines.belt.eval(y));
            if b - s < 0.12 {
                errs.push(format!("belt − sill < 0.12 m at y = {y:.2}"));
                break;
            }
        }
        for k in &self.keyframes {
            if !k.y_m.is_finite() {
                errs.push("keyframe y not finite".into());
            }
        }
        errs
    }

    /// The spec of trim `trim` (its overlays and ride height applied; an unknown or empty key
    /// gives the base), moved along y so its front axle lands on `front_axle_y` (the car's).
    pub fn for_car(&self, trim: &str, front_axle_y: f32) -> SectionSpec {
        let mut s = self.clone();
        if let Some(t) = self.trims.get(trim) {
            s.lines.sill = s.lines.sill.overlaid(&t.sill);
            s.lines.belt = s.lines.belt.overlaid(&t.belt);
            s.lines.greenhouse = s.lines.greenhouse.overlaid(&t.greenhouse);
            s.plan.half_width = s.plan.half_width.overlaid(&t.half_width);
            s.plan.haunch = s.plan.haunch.overlaid(&t.haunch);
            let dz = t.ride_dz_m.clamp(-0.08, 0.08);
            if dz != 0.0 {
                s.lines.sill = s.lines.sill.shifted(0.0, dz);
                s.lines.belt = s.lines.belt.shifted(0.0, dz);
                s.lines.greenhouse = s.lines.greenhouse.shifted(0.0, dz);
                s.anchors.z_bumper_top = s.anchors.z_bumper_top.map(|z| z + dz);
                s.anchors.z_door_bottom = s.anchors.z_door_bottom.map(|z| z + dz);
            }
        }
        let dy = front_axle_y - self.axles_y_m[0];
        if dy != 0.0 {
            s.axles_y_m = s.axles_y_m.map(|y| y + dy);
            let a = &mut s.anchors;
            for v in [
                &mut a.y_nose,
                &mut a.y_tail,
                &mut a.y_windshield_base,
                &mut a.y_roof_front,
                &mut a.y_roof_rear,
                &mut a.y_deck,
            ] {
                *v += dy;
            }
            if let Some(q) = &mut a.quarter_glass_y {
                *q = q.map(|y| y + dy);
            }
            for c in [
                &mut s.lines.sill,
                &mut s.lines.belt,
                &mut s.lines.greenhouse,
                &mut s.plan.half_width,
                &mut s.plan.haunch,
                &mut s.plan.roof_half_width,
            ] {
                *c = c.shifted(dy, 0.0);
            }
            for k in &mut s.keyframes {
                k.y_m += dy;
            }
        }
        s
    }

    /// The trim `key` (default when missing).
    pub fn trim(&self, key: &str) -> Trim {
        self.trims.get(key).cloned().unwrap_or_default()
    }

    /// Section knobs at `y`: each interpolated (monotone cubic) over the keyframes that set it.
    pub fn knobs_at(&self, y: f32) -> Knobs {
        let pick = |f: fn(&Keyframe) -> Option<f32>| -> Option<f32> {
            let mut pts: Vec<[f32; 2]> = self
                .keyframes
                .iter()
                .filter_map(|k| f(k).map(|v| [k.y_m, v]))
                .filter(|q| q[0].is_finite() && q[1].is_finite())
                .collect();
            if pts.is_empty() {
                return None;
            }
            pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
            pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-4);
            Some(Curve(pts).eval(y))
        };
        Knobs {
            sill_tuck: pick(|k| k.sill_tuck_m),
            shoulder_round: pick(|k| k.shoulder_round_m),
            crease_z_m: pick(|k| k.crease_z_m),
            crease_depth: pick(|k| k.crease_depth_m),
            glass_inset: pick(|k| k.glass_inset_m),
        }
    }

    /// Largest body half width (half_width + haunch), m.
    pub fn max_half_width(&self) -> f32 {
        let a = &self.anchors;
        let n = 200;
        (0..=n)
            .map(|i| {
                let y = a.y_tail + (a.y_nose - a.y_tail) * i as f32 / n as f32;
                self.plan.half_width.eval(y) + self.plan.haunch.eval(y)
            })
            .fold(0.0, f32::max)
    }
}

/// Station limits of [`adaptive_stations`]: (largest step on flat spans, near the ends and
/// arches, on the crumpling sheets: hood, trunk lid, fenders), m.
pub const ADAPTIVE_STEPS: (f32, f32, f32) = (0.15, 0.06, 0.12);
/// Deviation of the loft from the spec curves between two stations allowed by
/// [`adaptive_stations`], m.
pub const ADAPTIVE_TOL_M: f32 = 0.003;

/// Loft stations from `y0` to `y1` (ascending, both ends included) placed by curvature: each
/// step is the longest whose chord stays within `tol` of every sampled curve (`curves(y)`
/// returns the values to follow at `y`, the same count everywhere, NaN where a curve is
/// absent), capped by `max_step(y)`. Sampled every 5 mm.
pub fn adaptive_stations(
    y0: f32,
    y1: f32,
    tol: f32,
    max_step: impl Fn(f32) -> f32,
    curves: impl Fn(f32) -> Vec<f32>,
) -> Vec<f32> {
    let h = 0.005f32;
    let n = ((y1 - y0) / h).ceil().max(1.0) as usize;
    let grid: Vec<f32> = (0..=n)
        .map(|i| (y0 + (y1 - y0) * i as f32 / n as f32).min(y1))
        .collect();
    let vals: Vec<Vec<f32>> = grid.iter().map(|&y| curves(y)).collect();
    let mut out = vec![grid[0]];
    let mut i = 0;
    while i < n {
        let cap = max_step(grid[i]).max(2.0 * h);
        let mut best = i + 1;
        let mut j = i + 1;
        while j <= n && grid[j] - grid[i] <= cap + 1e-6 {
            // Chord i → j against every sample between.
            let ok = (i + 1..j).all(|k| {
                let t = (grid[k] - grid[i]) / (grid[j] - grid[i]).max(1e-6);
                vals[k].iter().enumerate().all(|(c, &v)| {
                    let a = vals[i].get(c).copied().unwrap_or(v);
                    let b = vals[j].get(c).copied().unwrap_or(v);
                    // A curve absent at either end or here (NaN) does not constrain the step.
                    if !(a.is_finite() && b.is_finite() && v.is_finite()) {
                        return true;
                    }
                    (a + (b - a) * t - v).abs() <= tol
                })
            });
            if ok {
                best = j;
            } else {
                break;
            }
            j += 1;
        }
        i = best;
        out.push(grid[i]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotone_cubic_never_overshoots() {
        let c = Curve(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.1, 1.0],
            [2.0, 1.0],
            [3.0, 0.5],
        ]);
        let mut x = -0.5;
        while x < 3.5 {
            let v = c.eval(x);
            assert!((-1e-5..=1.0 + 1e-5).contains(&v), "{x}: {v}");
            x += 0.01;
        }
        for q in &c.0 {
            assert!((c.eval(q[0]) - q[1]).abs() < 1e-5);
        }
    }

    #[test]
    fn overlays_replace_their_span() {
        let c = Curve(vec![[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]);
        let o = Curve(vec![[0.9, 5.0], [2.1, 5.0]]);
        let r = c.overlaid(&o);
        assert_eq!(r.0, vec![[0.0, 0.0], [0.9, 5.0], [2.1, 5.0], [3.0, 3.0]]);
    }

    #[test]
    fn adaptive_stations_are_finer_where_curved() {
        let ys = adaptive_stations(
            0.0,
            4.0,
            0.002,
            |_| 0.5,
            |y| vec![if y > 3.0 { (y - 3.0).powi(2) } else { 0.0 }],
        );
        // Flat span: the cap; curved span: much finer.
        for w in ys.windows(2) {
            let step = w[1] - w[0];
            if w[1] <= 3.0 {
                assert!((step - 0.5).abs() < 0.01, "{ys:?}");
            } else if w[0] >= 3.0 {
                assert!(step < 0.12, "{ys:?}");
            }
        }
        assert!(ys
            .windows(2)
            .all(|w| w[1] > w[0] && w[1] - w[0] <= 0.5 + 1e-4));
        assert_eq!(*ys.last().unwrap(), 4.0);
    }
}
