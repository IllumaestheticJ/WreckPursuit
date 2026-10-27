//! Body profile: the shape a car's procedural mesh is lofted from. Anchors are measured from
//! the car's `VehicleDef` (width, wheels) and its lattice (where the hood, doors, roof, glass
//! and trunk sit), then adjusted by the car's [`BodyStyle`] (silhouette, cabin shift, nose,
//! hood, greenhouse, sides, arches, tail). Panel zones stay on the lattice's zone boundaries,
//! so every panel mesh lines up with the lattice region it is skinned to and hinged panels
//! pivot on their real hinge nodes (the cabin knobs `cabin_shift_m` / `hood_length` move the
//! visual hood / trunk edges on purpose; see [`BodyProfile::lattice_y_fw`]).
//!
//! The loft cross-section is [`BodyProfile::ring`]: [`RING_POINTS`] points of the left half,
//! bottom → centre top. [`BodyProfile::side_point`] / [`BodyProfile::top_point`] query the
//! surface for details (`builder::surface_patch`).

use wp_contracts::{BodyRegion, LatticeDef, VehicleDef};

use crate::style::{
    ArchShape, BodyStyle, Silhouette, OVERHANG_RANGE, PLAN_SCALE_RANGE, SCULPT_DX_RANGE,
    TUMBLEHOME_M_RANGE,
};

/// Allowed range of [`BodyStyle::cabin_shift_m`], m (− rearward, + forward / cab-forward).
pub const CABIN_SHIFT_RANGE: (f32, f32) = (-0.60, 0.60);

/// Points per half cross-section ring (see [`BodyProfile::ring`]).
pub const RING_POINTS: usize = 18;
/// Ring index of the shoulder point (top of the side, at the belt).
pub const RING_SHOULDER: usize = 7;
/// Station offset of the paired stations that make vertical pickup-bed walls, m.
pub const BED_WALL_EPS: f32 = 0.004;
/// Greenhouse height over the flat top above which a ring takes the greenhouse layout, m.
pub const GH_MIN_M: f32 = 0.002;
/// Greenhouse height over which a ring blends from the flat-top to the greenhouse layout, m.
pub const GH_BLEND_M: f32 = 0.05;
/// Largest corner drop of the windshield / rear-glass plan wrap, m (see
/// [`BodyProfile::greenhouse_crown`]).
pub const WRAP_DROP_MAX_M: f32 = 0.035;
/// Ring index of the side-glass base (greenhouse layout).
pub const RING_GLASS_BASE: usize = 8;
/// Ring index of the side-glass top edge (greenhouse layout): the A-pillar's lower edge at the
/// windshield, the roof rail at the roof.
pub const RING_GLASS_TOP: usize = 10;
/// Ring index of the windshield / rear-glass edge: points 10…13 are the pillar (or roof rail),
/// 13…17 the glass (or roof).
pub const RING_PILLAR_IN: usize = 13;

/// What the face from ring point `j` to `j + 1` is (the loft classifies parts from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seg {
    /// Side panel below the belt.
    Lower,
    /// Belt ledge between the shoulder and the glass base (door top).
    Ledge,
    /// Greenhouse side (windows, pillars).
    Glass,
    /// Top surfaces (hood, roof, glass, deck).
    Top,
    /// Inner wall of an open pickup bed.
    BedWall,
    /// Floor of an open pickup bed.
    BedFloor,
}

/// One ring point: position (left half, x ≤ 0) and the kind of the segment to the next point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingPt {
    /// Position, chassis space.
    pub p: [f32; 3],
    /// Segment from this point to the next (the last point's value is unused).
    pub seg: Seg,
}

/// Measured + styled anchors (chassis space, m). `y` grows forward. All fields are the
/// *visual* (styled) values unless named `lattice_*`.
#[derive(Debug, Clone)]
pub struct BodyProfile {
    /// The style the profile was built with.
    pub style: BodyStyle,
    /// Body half width (before flares / haunches).
    pub half_w: f32,
    /// Mesh nose y.
    pub y_nose: f32,
    /// Mesh tail y.
    pub y_tail: f32,
    /// Underside height.
    pub z_floor: f32,
    /// Firewall = hood rear edge / windshield base y (visual).
    pub y_fw: f32,
    /// Lattice hood hinge line y (`y_fw` before `cabin_shift_m` / `hood_length`).
    pub lattice_y_fw: f32,
    /// Hood front edge y; ahead of it (below the belt) is the front bumper cover. The lattice
    /// hood front, unless a short front overhang ([`BodyStyle::overhang_trim`]) moved it back to
    /// 10 cm behind the nose.
    pub y_bf: f32,
    /// Hood height at the firewall / at its front edge.
    pub z_hood: (f32, f32),
    /// Nose leading-edge height (top of the front face).
    pub z_nose: f32,
    /// Cabin belt line (door tops).
    pub z_belt: f32,
    /// Door bottoms (below: sills).
    pub z_door_bottom: f32,
    /// Roof rear y, front y, height, lattice roof half width.
    pub roof: (f32, f32, f32, f32),
    /// Deck (trunk lid / bed cover) start y. Rear glass runs roof → deck start.
    pub y_deck: f32,
    /// Lattice deck start (trunk hinge) y, before `cabin_shift_m`.
    pub lattice_y_deck: f32,
    /// Deck height.
    pub z_deck: f32,
    /// Trunk is a lid (true) or a tailgate on the rear face (false).
    pub trunk_lid: bool,
    /// Rear glass exists (else the greenhouse ends in a vertical cab-back wall).
    pub rear_glass: bool,
    /// Rear quarters start (cabin rear) / end (rear bumper begins) y (the end at least 10 cm
    /// ahead of the tail, see `y_bf`).
    pub y_quarter: (f32, f32),
    /// Front door hinge y (door front edge).
    pub y_fd_hinge: f32,
    /// Rear door hinge y and rear edge y (4-door cars).
    pub rear_door: Option<(f32, f32)>,
    /// 2-door cars: the front door's rear edge (start of the side strip region).
    pub y_fd_rear: f32,
    /// Bumper cover tops (front, rear).
    pub z_bumper_top: (f32, f32),
    /// Wheels: (y, z centre, arch radius).
    pub arches: Vec<(f32, f32, f32)>,
    /// Per wheel (same order as `arches`): |x| of the tire's outer face, m. The plan-width
    /// curve never narrows the body inside it ([`BodyStyle::plan_width`]).
    pub tire_outer_x: Vec<f32>,
    /// Open pickup bed: (y from, y to, floor z), when the silhouette and lattice allow one.
    pub bed: Option<(f32, f32, f32)>,
    /// Roof half width at the roof (before the A-pillar taper).
    pub roof_half: f32,
    /// Side-view length over which the nose top falls to `z_nose`.
    pub nose_run: f32,
}

fn region_nodes(l: &LatticeDef, r: BodyRegion) -> Vec<[f32; 3]> {
    l.nodes
        .iter()
        .filter(|n| n.region == r)
        .map(|n| n.pos_m)
        .collect()
}

fn range(ps: &[[f32; 3]], k: usize) -> (f32, f32) {
    let lo = ps.iter().map(|p| p[k]).fold(f32::MAX, f32::min);
    let hi = ps.iter().map(|p| p[k]).fold(f32::MIN, f32::max);
    (lo, hi)
}

/// Cubic smoothstep of `x` clamped to 0..1.
pub fn smoothstep(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `x` limited to `lo..=hi` without panicking when the bounds cross (`lo > hi` gives `hi`).
/// Style knobs at their extremes can push measured bounds past each other; the upper bound
/// wins so the body stays inside the car.
pub fn clamp_soft(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// Linear interpolation with `t` clamped to 0..1.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn tan_deg(d: f32) -> f32 {
    let (s, c) = wp_math::sin_cos(d.to_radians());
    s / c
}

impl BodyProfile {
    /// Measure a profile with the default style. Panics only on lattices without the
    /// standard regions (every `gen-lattice` body has them).
    pub fn measure(def: &VehicleDef, l: &LatticeDef) -> BodyProfile {
        Self::styled(def, l, &BodyStyle::default())
    }

    /// Measure a profile and apply `style`.
    pub fn styled(def: &VehicleDef, l: &LatticeDef, style: &BodyStyle) -> BodyProfile {
        let st = style.clone();
        let all: Vec<[f32; 3]> = l.nodes.iter().map(|n| n.pos_m).collect();
        let half_w = range(&all, 0).1.max(-range(&all, 0).0);
        let fb = region_nodes(l, BodyRegion::FrontBumper);
        let rb = region_nodes(l, BodyRegion::RearBumper);
        let hood = region_nodes(l, BodyRegion::Hood);
        let (y_fw, y_bf) = range(&hood, 1);
        let hood_z_at = |y: f32| {
            hood.iter()
                .filter(|p| (p[1] - y).abs() < 0.02)
                .map(|p| p[2])
                .fold(f32::MIN, f32::max)
        };
        let door = region_nodes(l, BodyRegion::LeftDoorF);
        let (z_door_bottom, z_belt) = range(&door, 2);
        let roof_n = region_nodes(l, BodyRegion::Roof);
        let (ry0, ry1) = range(&roof_n, 1);
        let roof_z = range(&roof_n, 2).1;
        let roof_hw = range(&roof_n, 0).1;
        let trunk = region_nodes(l, BodyRegion::Trunk);
        let (ty0, ty1) = range(&trunk, 1);
        let trunk_lid = ty1 - ty0 >= 0.15;
        let rear_glass = !region_nodes(l, BodyRegion::RearGlass).is_empty();
        let cabin = region_nodes(l, BodyRegion::Cabin);
        let quarter = region_nodes(l, BodyRegion::RearLeftQuarter);
        let (qy0, qy1) = range(&quarter, 1);
        let z_quarter_top = range(&quarter, 2).1;
        let y_deck = if trunk_lid {
            ty1
        } else if rear_glass {
            // Hatch: the rear glass runs down to the tailgate.
            ty1.max(range(&region_nodes(l, BodyRegion::RearGlass), 1).0)
        } else {
            // Pickup: vertical cab-back wall at the cabin's rear.
            range(&cabin, 1).0
        };
        let z_deck = if trunk_lid {
            range(&trunk, 2).1
        } else {
            z_quarter_top.max(range(&trunk, 2).1)
        };
        let fd_hinge = l
            .detachables
            .iter()
            .find(|d| d.region == BodyRegion::LeftDoorF)
            .map_or(range(&door, 1).1, |d| {
                l.nodes[d.hinge_nodes[0] as usize].pos_m[1]
            });
        let four_door = l
            .detachables
            .iter()
            .any(|d| d.region == BodyRegion::LeftDoorR);
        let rdoor = region_nodes(l, BodyRegion::LeftDoorR);
        let rear_door = if four_door {
            let (rmin, rmax) = range(&rdoor, 1);
            Some((rmax, 0.5 * (rmin + qy1)))
        } else {
            None
        };
        let y_fd_rear = match rear_door {
            Some((hinge, _)) => hinge - 0.01,
            None => range(&rdoor, 1).1 + 0.03,
        };
        let arch_gap = st.arch_gap.clamp(0.02, 0.15);
        let arches: Vec<(f32, f32, f32)> = def
            .wheels
            .iter()
            .map(|w| (w.position_m[1], w.position_m[2], w.radius_m + arch_gap))
            .collect();

        // --- Style: stance, overhangs, belt ---
        let dz = st.ride_dz.clamp(-0.05, 0.08);
        let belt_dz = st.belt_dz.clamp(-0.06, 0.08);
        let oh = |v: f32| v.clamp(OVERHANG_RANGE.0, OVERHANG_RANGE.1);
        // Short overhangs stop 6 cm short of the tires; past the hood front / quarter end the
        // visual hood front / quarter end move back with them, so the bumper panels always keep
        // a 10 cm strip (their lattice nodes then sit a little ahead of / behind the mesh).
        let (front_tire, rear_tire) = def.wheels.iter().fold((f32::MIN, f32::MAX), |(f, r), w| {
            (
                f.max(w.position_m[1] + w.radius_m),
                r.min(w.position_m[1] - w.radius_m),
            )
        });
        let y_nose = (range(&fb, 1).1 + 0.05 + oh(st.overhang_trim.0)).max(front_tire + 0.06);
        let y_tail = (range(&rb, 1).0 - 0.05 - oh(st.overhang_trim.1)).min(rear_tire - 0.06);
        let lattice_y_bf = y_bf;
        let y_bf = y_bf.min(y_nose - 0.10);
        let qy0 = qy0.max(y_tail + 0.10);
        let z_hood = (
            hood_z_at(y_fw) + dz + belt_dz,
            hood_z_at(lattice_y_bf) + dz + 0.3 * belt_dz,
        );
        let z_belt = z_belt + dz + belt_dz;
        let z_deck = z_deck + dz + belt_dz;
        let z_floor = range(&all, 2).0 + dz;
        let z_door_bottom = z_door_bottom + dz;
        let rz = roof_z + dz + st.roof_dz.clamp(-0.12, 0.10);

        // --- Cabin placement ---
        let shift = st
            .cabin_shift_m
            .clamp(CABIN_SHIFT_RANGE.0, CABIN_SHIFT_RANGE.1);
        let mut v_fw = (y_fw + shift).min(y_bf - 0.30);
        let mut r1 = ry1 + shift;
        let mut r0 = ry0 + shift;
        let v_deck = clamp_soft(y_deck + shift, y_tail + 0.25, r0 - 0.05);
        if let Some(len) = st.hood_length {
            let want = clamp_soft(y_nose - len, r0 + 0.6, y_bf - 0.30);
            r1 += want - v_fw;
            v_fw = want;
        }
        let cowl_z = z_hood.0 + st.cowl_h.clamp(-0.05, 0.25) + 0.015;
        if let Some(a) = st.windshield_rake_deg {
            let a = a.clamp(30.0, 75.0);
            r1 = v_fw - (rz - cowl_z).max(0.1) * tan_deg(a);
        }
        let roof_to_tail = matches!(
            st.silhouette,
            Silhouette::Wagon | Silhouette::Suv | Silhouette::Van
        );
        if roof_to_tail {
            r0 = r0.min(v_deck + 0.12);
        }
        if let Some(a) = st.rear_glass_rake_deg {
            let a = a.clamp(15.0, 85.0);
            r0 = v_deck + (rz - z_deck).max(0.05) * tan_deg(a);
        }
        r0 -= st.roof_len_trim.clamp(-0.25, 0.15);
        // Roof front stays behind the windshield base; roof rear ahead of the deck start;
        // the flat roof keeps ≥ 0.25 m where there is room (≥ 0.05 m always).
        r1 = clamp_soft(r1, r0 + 0.25, v_fw - 0.10);
        r0 = clamp_soft(r0.min(r1 - 0.25), v_deck + 0.06, r1 - 0.05);

        let roof_half = match (st.tumblehome_m, st.tumblehome) {
            (Some(t), _) => half_w - t.clamp(TUMBLEHOME_M_RANGE.0, TUMBLEHOME_M_RANGE.1),
            (None, Some(t)) => half_w - t.clamp(0.10, 0.30),
            (None, None) => (roof_hw + 0.05).min(half_w - 0.12),
        };
        let nose_h = st.nose_h.map_or(z_hood.1 - 0.10, |h| h + dz);
        let z_nose = clamp_soft(nose_h, z_floor + 0.15, z_hood.1);
        let nose_run = (0.35
            + (z_hood.1 - z_nose).max(0.0) * tan_deg(st.nose_rake_deg.clamp(0.0, 60.0)))
        .clamp(0.10, 1.2);

        let mut p = BodyProfile {
            style: st,
            half_w,
            y_nose,
            y_tail,
            z_floor,
            y_fw: v_fw,
            lattice_y_fw: y_fw,
            y_bf,
            z_hood,
            z_nose,
            z_belt,
            z_door_bottom,
            roof: (r0, r1, rz, roof_hw),
            y_deck: v_deck,
            lattice_y_deck: y_deck,
            z_deck,
            trunk_lid,
            rear_glass,
            y_quarter: (qy1, qy0),
            y_fd_hinge: fd_hinge,
            rear_door,
            y_fd_rear,
            z_bumper_top: (range(&fb, 2).1 + dz, range(&rb, 2).1 + dz),
            arches,
            tire_outer_x: def
                .wheels
                .iter()
                .map(|w| w.position_m[0].abs() + 0.5 * w.width_m)
                .collect(),
            bed: None,
            roof_half,
            nose_run,
        };
        if p.style.silhouette == Silhouette::Pickup && !trunk_lid && !rear_glass {
            let y0 = p.y_tail + 0.07;
            let y1 = p.y_deck - 0.04;
            if y1 - y0 > 0.6 {
                let floor = z_deck - p.style.bed_depth.clamp(0.2, 0.6);
                p.bed = Some((y0, y1, floor.max(p.z_floor + 0.25)));
            }
        }
        p
    }

    // ------------------------------------------------------------------ side view

    /// 0 → 1 toward the nose inside the nose fall zone.
    fn nose_t(&self, y: f32) -> f32 {
        smoothstep(1.0 - (self.y_nose - y) / self.nose_run)
    }

    /// 0 → 1 toward the tail inside the tail rounding zone.
    fn tail_t(&self, y: f32) -> f32 {
        smoothstep(1.0 - (y - self.y_tail) / self.style.tail_round.clamp(0.05, 0.6))
    }

    /// Plan-view rounding factor (0 → 1 toward either end).
    fn plan_t(&self, y: f32) -> (f32, f32) {
        let n = self.style.nose_plan_round.clamp(0.05, 0.45);
        let t = self.style.tail_plan_round.clamp(0.05, 0.45);
        (
            smoothstep(1.0 - (self.y_nose - y) / n),
            smoothstep(1.0 - (y - self.y_tail) / t),
        )
    }

    /// Body half width at `y` (rounded in plan view at the ends), before flares/haunches.
    pub fn half_width(&self, y: f32) -> f32 {
        let (n, t) = self.plan_t(y);
        let ni = (0.23 * self.style.nose_plan_round).clamp(0.01, 0.12);
        let ti = (0.23 * self.style.tail_plan_round).clamp(0.01, 0.12);
        let w = self.half_w - ni * n - ti * t;
        let bow = self.end_bow(y);
        if self.style.plan_width.is_empty() && bow >= 1.0 {
            return w;
        }
        let ws = w * bow * self.plan_scale(y);
        if ws >= w {
            return ws;
        }
        // Over a wheel the body stays outside the tire's outer face (+1 cm).
        let floor = self
            .arches
            .iter()
            .zip(&self.tire_outer_x)
            .map(|(&(wy, _, r), &tx)| {
                let d = (y - wy).abs() - r;
                let t = if d <= 0.0 {
                    1.0
                } else {
                    smoothstep(1.0 - d / 0.15)
                };
                lerp(ws, (tx + 0.01).min(w), t)
            })
            .fold(ws, f32::max);
        ws.max(floor)
    }

    /// Plan-view bow factor at `y` ([`BodyStyle::nose_plan_bow_m`] / `tail_plan_bow_m`): 1
    /// outside the bow, an elliptical arc down to the tip width at the end.
    pub fn end_bow(&self, y: f32) -> f32 {
        let s = &self.style;
        let arc = |bow: f32, tip: f32, d: f32| -> f32 {
            let bow = bow.clamp(0.0, 0.5);
            if bow <= 0.0 || d >= bow {
                return 1.0;
            }
            let t = 1.0 - d.max(0.0) / bow;
            let f0 = tip.clamp(0.1, 1.0);
            f0 + (1.0 - f0) * (1.0 - t * t).max(0.0).sqrt()
        };
        arc(s.nose_plan_bow_m, s.nose_tip_w, self.y_nose - y)
            * arc(s.tail_plan_bow_m, s.tail_tip_w, y - self.y_tail)
    }

    /// Depth (along y) of the nose / tail face profile from the face rake and chin knobs, m
    /// (0 when they are off).
    pub fn face_depth(&self, front: bool) -> f32 {
        let (rake, chin, zb, zt) = self.face_params(front);
        tan_deg(rake.abs()) * (zt - zb).max(0.0) + chin
    }

    /// (rake °, chin m, face bottom z, face top z) of an end. The face never reaches past the
    /// windshield base − 0.10 m (nose) or the deck / rear-glass start + 0.03 m (tail): a deeper
    /// setting is scaled down to fit (those stations are lattice zone boundaries); with less than
    /// 4 cm of room the face stays vertical.
    fn face_params(&self, front: bool) -> (f32, f32, f32, f32) {
        let s = &self.style;
        let (rake, chin, zb, zt, room) = if front {
            (
                s.nose_face_rake_deg.clamp(-15.0, 60.0),
                s.nose_chin_m.clamp(0.0, 0.25),
                self.sill(self.y_nose),
                self.z_nose,
                self.y_nose - self.y_fw - 0.10,
            )
        } else {
            (
                s.tail_face_rake_deg.clamp(-15.0, 60.0),
                s.tail_chin_m.clamp(0.0, 0.25),
                self.sill(self.y_tail),
                self.belt(self.y_tail),
                self.y_deck - self.y_tail - 0.03,
            )
        };
        if rake == 0.0 && chin == 0.0 {
            return (rake, chin, zb, zt);
        }
        // No room for a face (a tail trimmed past the deck start): keep it vertical.
        if room < 0.04 {
            return (0.0, 0.0, zb, zt);
        }
        let r = tan_deg(rake.abs());
        let depth = r * (zt - zb).max(0.0) + chin;
        if depth <= room {
            return (rake, chin, zb, zt);
        }
        let k = room / depth;
        let rake = (r * k).atan().to_degrees() * rake.signum();
        (rake, chin * k, zb, zt)
    }

    /// Allowed height band (lo, hi) of the loft at `y` from the end face profiles, or `None`
    /// where no face knob reaches. A point at height z is behind the face when its set-back
    /// (rake: from the face bottom up, or from the top down when leaning forward; chin: the
    /// lower 40 % sweeping back) is at most the distance to the end.
    fn face_band(&self, y: f32) -> Option<(f32, f32)> {
        let mut band: Option<(f32, f32)> = None;
        for front in [true, false] {
            let depth = self.face_depth(front);
            let d = if front {
                self.y_nose - y
            } else {
                y - self.y_tail
            };
            if depth <= 1e-4 || d >= depth {
                continue;
            }
            let (rake, chin, zb, zt) = self.face_params(front);
            let r = tan_deg(rake);
            let zc = zb + 0.4 * (zt - zb).max(0.05);
            let setback = |z: f32| -> f32 {
                let rk = if r >= 0.0 {
                    r * (z - zb).max(0.0)
                } else {
                    -r * (zt - z).max(0.0)
                };
                rk + chin * smoothstep((zc - z) / (zc - zb).max(0.02))
            };
            // Scan heights in 1 cm steps for the allowed run, then refine both ends.
            // From the face bottom (the underside there) up: below it the chin would read as
            // a flat set-back and pull points under the car.
            let (z0, z1) = (zb.min(zt - 0.05), self.z_max() + 0.2);
            let steps = ((z1 - z0) / 0.01).ceil() as usize;
            let at = |i: usize| z0 + (z1 - z0) * i as f32 / steps as f32;
            let ok: Vec<bool> = (0..=steps).map(|i| setback(at(i)) <= d).collect();
            let (lo, hi) = match (ok.iter().position(|&o| o), ok.iter().rposition(|&o| o)) {
                (Some(a), Some(b)) => {
                    let refine = |mut out: f32, mut inn: f32| {
                        for _ in 0..12 {
                            let m = 0.5 * (out + inn);
                            if setback(m) <= d {
                                inn = m;
                            } else {
                                out = m;
                            }
                        }
                        inn
                    };
                    let lo = if a == 0 {
                        at(0)
                    } else {
                        refine(at(a - 1), at(a))
                    };
                    let hi = if b == steps {
                        at(steps)
                    } else {
                        refine(at(b + 1), at(b))
                    };
                    (lo, hi)
                }
                _ => {
                    // Ahead of the whole face: collapse onto its foremost height.
                    let zm = (0..=steps)
                        .map(at)
                        .min_by(|a, b| setback(*a).total_cmp(&setback(*b)))
                        .unwrap_or(zb);
                    (zm, zm)
                }
            };
            band = Some(match band {
                Some((l, h)) => (l.max(lo), h.min(hi).max(l.max(lo))),
                None => (lo, hi),
            });
        }
        band
    }

    /// Plan-width scale at `y` from [`BodyStyle::plan_width`] (1 when empty): smoothstep
    /// between control points sorted by y, held past the ends, each clamped to
    /// [`PLAN_SCALE_RANGE`].
    pub fn plan_scale(&self, y: f32) -> f32 {
        let mut pts: Vec<[f32; 2]> = self
            .style
            .plan_width
            .iter()
            .map(|q| [q[0], q[1].clamp(PLAN_SCALE_RANGE.0, PLAN_SCALE_RANGE.1)])
            .filter(|q| q[0].is_finite() && q[1].is_finite())
            .collect();
        if pts.is_empty() {
            return 1.0;
        }
        pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        if y <= pts[0][0] {
            return pts[0][1];
        }
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if y <= b[0] {
                let t = smoothstep((y - a[0]) / (b[0] - a[0]).max(1e-4));
                return a[1] + (b[1] - a[1]) * t;
            }
        }
        pts[pts.len() - 1][1]
    }

    /// Fender-top rise at `y` ([`BodyStyle::front_fender_rise_m`] /
    /// [`BodyStyle::rear_fender_rise_m`]), m, before the band across the width.
    pub fn fender_rise(&self, y: f32) -> f32 {
        let s = &self.style;
        let fr = s.front_fender_rise_m.clamp(0.0, 0.12);
        let rr = s.rear_fender_rise_m.clamp(0.0, 0.12);
        if fr <= 0.0 && rr <= 0.0 {
            return 0.0;
        }
        let mut front = f32::MIN;
        let mut rear = f32::MAX;
        for &(wy, _, r) in &self.arches {
            front = front.max(wy - r);
            rear = rear.min(wy + r);
        }
        if front == f32::MIN {
            return 0.0;
        }
        let f = fr * smoothstep((y - (front - 0.3)) / 0.3);
        let b = rr * smoothstep(((rear + 0.3) - y) / 0.3);
        f.max(b)
    }

    /// Fender-top rise of a top point at (y, x), m: [`Self::fender_rise`] times the band
    /// across the width. The band is measured on the body's full half width (not the local one,
    /// which narrows toward the ends), so at a given x the rise never grows toward the nose or
    /// tail: no lip in front of lamps.
    pub fn fender_top(&self, y: f32, x: f32) -> f32 {
        let rise = self.fender_rise(y);
        if rise <= 0.0 {
            return 0.0;
        }
        let pw = self.style.fender_peak_w.clamp(0.5, 0.95);
        let u = x.abs() / self.half_w.max(0.1);
        rise * smoothstep((u - (pw - 0.3)) / 0.3)
    }

    /// Side-wall offset (m, + outward) of a side ring point at (y, z): sculpts plus the side
    /// bulge (`f` = height fraction sill → belt).
    fn side_offset(&self, y: f32, z: f32, f: f32, fc: f32) -> f32 {
        let s = &self.style;
        let mut dx = 0.0;
        let lean = s.lower_lean_in_m.clamp(0.0, 0.08);
        if lean > 0.0 && f < fc {
            dx -= lean * (1.0 - f.max(0.0) / fc.max(0.05));
        }
        if !s.side_sculpt.is_empty() {
            let sum: f32 = s.side_sculpt.iter().map(|c| c.dx_at(y, z)).sum();
            dx += sum.clamp(SCULPT_DX_RANGE.0, SCULPT_DX_RANGE.1);
        }
        let bulge = s.side_bulge_m.clamp(0.0, 0.06);
        if bulge > 0.0 {
            let zb = s.side_bulge_z.clamp(0.2, 0.8);
            let f = f.clamp(0.0, 1.0);
            let t = if f < zb {
                smoothstep(f / zb)
            } else {
                smoothstep((1.0 - f) / (1.0 - zb))
            };
            dx += bulge * t;
        }
        dx
    }

    /// Underside height at `y` (rises toward the ends: approach/departure chamfer).
    pub fn sill(&self, y: f32) -> f32 {
        let (n, t) = self.plan_t(y);
        self.z_floor + 0.06 * n.max(t)
    }

    /// Hood edge line at `y ≥ y_fw` (before the nose fall).
    fn hood_line(&self, y: f32) -> f32 {
        let cowl = self.z_hood.0 + self.style.cowl_h.clamp(-0.05, 0.25);
        if self.style.silhouette == Silhouette::Wedge {
            // One straight line from the cowl to the nose lip.
            return lerp(
                cowl,
                self.z_nose,
                (y - self.y_fw) / (self.y_nose - self.y_fw),
            );
        }
        let span = (self.y_bf - self.y_fw).max(0.1);
        lerp(cowl, self.z_hood.1, (y - self.y_fw) / span)
    }

    /// Belt rise toward the rear (belt rake + kick), m.
    fn rear_rise(&self, y: f32) -> f32 {
        let s = &self.style;
        let rake = s.belt_rake.clamp(-0.10, 0.15)
            * ((self.y_fd_hinge - y) / (self.y_fd_hinge - self.y_tail)).clamp(0.0, 1.0);
        let kick = s.belt_kick.clamp(0.0, 0.08) * smoothstep((self.roof.0 + 0.35 - y) / 0.3);
        rake + kick
    }

    /// Shoulder (top of the side panels) at `y`.
    pub fn belt(&self, y: f32) -> f32 {
        let base = if y >= self.y_fw {
            self.hood_line(y)
        } else if y >= self.y_quarter.0 {
            self.z_belt + self.rear_rise(y)
        } else {
            // Blend from the belt to the deck over the first 0.3 m of the quarter.
            lerp(self.z_belt, self.z_deck, (self.y_quarter.0 - y) / 0.3) + self.rear_rise(y)
        };
        let nose_fall = if self.style.silhouette == Silhouette::Wedge {
            0.0
        } else {
            (self.z_hood.1 - self.z_nose).max(0.0)
        };
        base - nose_fall * self.nose_t(y) - self.style.tail_drop.clamp(0.0, 0.25) * self.tail_t(y)
    }

    /// Fastback / hatch rear line: (start y, z) at the roof rear → (end y, z) at the tail.
    fn rear_line(&self) -> Option<((f32, f32), (f32, f32))> {
        let s = &self.style;
        let end_y = self.y_tail + 0.5 * s.tail_round.clamp(0.05, 0.6);
        let end_z = match s.silhouette {
            Silhouette::Fastback => self.z_deck + 0.015 - s.deck_drop.clamp(-0.05, 0.25),
            Silhouette::Hatch => self.z_deck + 0.06 - s.deck_drop.clamp(-0.05, 0.25),
            _ => return None,
        };
        Some(((self.roof.0, self.roof.2), (end_y, end_z)))
    }

    /// Ducktail rise at `y` (deck trailing edge).
    fn ducktail(&self, y: f32) -> f32 {
        let start = self.y_tail + self.style.tail_round.clamp(0.05, 0.6) + 0.30;
        self.style.ducktail_h.clamp(-0.03, 0.08) * smoothstep((start - y) / 0.30)
    }

    /// Deck top centre line at `y ≤ y_deck`.
    fn deck_top(&self, y: f32) -> f32 {
        let flat = self.belt(y) + 0.015;
        if let Some(((y0, z0), (y1, z1))) = self.rear_line() {
            let z = lerp(z0, z1, (y0 - y) / (y0 - y1).max(0.05));
            return z.max(flat) + self.ducktail(y);
        }
        let t = ((self.y_deck - y) / (self.y_deck - self.y_tail).max(0.1)).clamp(0.0, 1.0);
        flat - self.style.deck_drop.clamp(-0.05, 0.25) * t + self.ducktail(y)
    }

    /// Top of the body at `y` on the centre line, before crowns: the roof, glass slopes, the
    /// deck, or just above the belt (hood). [`BodyStyle::roof_arc`] blends the greenhouse
    /// toward one smooth arc.
    pub fn top(&self, y: f32) -> f32 {
        let lin = self.top_straight(y);
        match self.arc_top(y) {
            Some(arc) => {
                let k = self.style.roof_arc.clamp(0.0, 1.0);
                lerp(lin, arc, k).max(self.belt(y) + 0.015)
            }
            None => lin,
        }
    }

    /// The greenhouse arc of [`BodyStyle::roof_arc`] at `y`: two parabolas meeting flat at the
    /// roof middle, one down to the windshield base, one down to the rear glass base (or the
    /// fastback / hatch line's end at the tail). `None` outside that span or when the knob is 0.
    fn arc_top(&self, y: f32) -> Option<f32> {
        if self.style.roof_arc <= 0.0 || y >= self.y_fw {
            return None;
        }
        let (r0, r1, rz, _) = self.roof;
        let yc = 0.5 * (r0 + r1);
        let (ye, ze) = match self.rear_line() {
            Some((_, end)) => end,
            None => (self.y_deck, self.deck_top(self.y_deck)),
        };
        if y <= ye {
            return None;
        }
        let z = if y >= yc {
            let zf = self.belt(self.y_fw) + 0.015;
            let t = (y - yc) / (self.y_fw - yc).max(0.05);
            rz - (rz - zf) * t * t
        } else {
            let t = (yc - y) / (yc - ye).max(0.05);
            rz - (rz - ze) * t * t
        };
        Some(z)
    }

    /// [`Self::top`] with the straight greenhouse (windshield line, flat roof, rear glass).
    fn top_straight(&self, y: f32) -> f32 {
        let (r0, r1, rz, _) = self.roof;
        if y >= self.y_fw {
            self.belt(y) + 0.015
        } else if y > r1 {
            lerp(
                rz,
                self.belt(self.y_fw) + 0.015,
                (y - r1) / (self.y_fw - r1),
            )
        } else if y >= r0 {
            rz
        } else if y > self.y_deck {
            let t = ((r0 - y) / (r0 - self.y_deck).max(0.05)).clamp(0.0, 1.0);
            let base = match self.rear_line() {
                Some(((y0, z0), (y1, z1))) => lerp(z0, z1, (y0 - y) / (y0 - y1).max(0.05)),
                None => lerp(rz, self.deck_top(self.y_deck), t),
            };
            base + self.style.rear_glass_bow.clamp(-0.03, 0.10) * 4.0 * t * (1.0 - t)
        } else {
            self.deck_top(y)
        }
    }

    /// How far the greenhouse rises over the flat top (belt + 1.5 cm) at `y`, m: 0 on the
    /// hood, the deck and a ducktail lip, the cabin height over the belt elsewhere.
    pub fn greenhouse_excess(&self, y: f32) -> f32 {
        if y >= self.y_fw {
            return 0.0;
        }
        let duck = if y < self.y_deck {
            self.ducktail(y)
        } else {
            0.0
        };
        (self.top(y) - duck - (self.belt(y) + 0.015)).max(0.0)
    }

    /// Greenhouse weight of the ring at `y`, 0…1: 0 = flat top (hood / deck), 1 = full
    /// greenhouse (side glass, roof rail, pillars). Ramps over the first [`GH_BLEND_M`] of
    /// greenhouse height, so the cross-section changes smoothly at the windshield base and
    /// where a rear glass or fastback line meets the deck (no fans of thin faces there).
    pub fn greenhouse_weight(&self, y: f32) -> f32 {
        smoothstep(self.greenhouse_excess(y) / GH_BLEND_M)
    }

    /// True where the ring at `y` has the greenhouse layout (side glass, rail, pillars).
    pub fn has_greenhouse(&self, y: f32) -> bool {
        self.greenhouse_excess(y) > GH_MIN_M
    }

    /// Front door window y span (rear edge, front edge): half the B-pillar width
    /// ([`BodyStyle::b_pillar_w_m`]) ahead of the front door's rear edge, forward under the
    /// A-pillar to the sail at its foot ([`BodyStyle::a_sail_len_m`]); at most 0.45 m past the
    /// door hinge line.
    pub fn front_window(&self) -> (f32, f32) {
        let mb = 0.5 * self.style.b_pillar_w_m.clamp(0.05, 0.25);
        let rear = match self.rear_door {
            Some((rh, _)) => rh + mb,
            None => self.y_fd_rear + mb,
        };
        let sail = self.style.a_sail_len_m.clamp(0.04, 0.40);
        (
            rear,
            clamp_soft(self.y_fw - sail, rear + 0.2, self.y_fd_hinge + 0.45),
        )
    }

    /// The sail (mirror patch) at the A-pillar foot, ahead of the front door glass: (rear y,
    /// front y). Door mirrors sit on it.
    pub fn sail(&self) -> (f32, f32) {
        let (_, f) = self.front_window();
        let sail = self.style.a_sail_len_m.clamp(0.04, 0.40);
        (f, self.y_fw.min(f + sail).max(f + 0.02))
    }

    /// Pillar width seen from ahead / behind at `y`, m: [`BodyStyle::a_pillar_w_m`] ahead of
    /// the roof front, [`BodyStyle::c_pillar_w_m`] behind the roof rear, blended along the roof.
    pub fn pillar_w(&self, y: f32) -> f32 {
        let a = self.style.a_pillar_w_m.clamp(0.04, 0.14);
        let c = self.style.c_pillar_w_m.clamp(0.04, 0.30);
        let (r0, r1, _, _) = self.roof;
        lerp(c, a, (y - r0) / (r1 - r0).max(0.05))
    }

    /// Windshield slope over the ground, dz/dy (≥ 0), and rear-glass slope.
    fn glass_slopes(&self) -> (f32, f32) {
        let (r0, r1, rz, _) = self.roof;
        let zf = self.belt(self.y_fw) + 0.015;
        let front = ((rz - zf) / (self.y_fw - r1).max(0.05)).clamp(0.0, 3.0);
        let zd = self.top(self.y_deck);
        let rear = ((rz - zd) / (r0 - self.y_deck).max(0.05)).clamp(0.0, 3.0);
        (front, rear)
    }

    /// Cross-section crown of the greenhouse top at `y` for `u` = |x| / roof half width: the
    /// roof crown (the hood crown near the windshield base and the deck), plus the plan-view wrap
    /// of the windshield and rear glass ([`BodyStyle::windshield_wrap_m`] /
    /// [`BodyStyle::rear_glass_wrap_m`]): on a raked glass, corners set back in plan are corners
    /// lower in the cross-section. The wrap fades out over the roof.
    pub fn greenhouse_crown(&self, y: f32, u: f32) -> f32 {
        let s = &self.style;
        let u = u.clamp(0.0, 1.0);
        let (r0, r1, _, _) = self.roof;
        let flat = s.hood_crown.clamp(0.0, 0.05) * (1.0 - u * u);
        let roof = s.roof_crown.clamp(0.0, 0.05) * (1.0 - u * u);
        let tf = smoothstep((self.y_fw - y) / 0.25);
        let tr = smoothstep((y - self.y_deck) / 0.25);
        let crown = lerp(flat, roof, tf.min(tr));
        let fade = (0.45 * (r1 - r0)).clamp(0.05, 0.30);
        let gf = if y >= r1 {
            1.0
        } else {
            smoothstep(1.0 - (r1 - y) / fade)
        };
        let gr = if y <= r0 {
            smoothstep(1.0 - (self.y_deck - y).max(0.0) / 0.3)
        } else {
            smoothstep(1.0 - (y - r0) / fade)
        };
        // The corner drop is capped: on near-vertical glass (vans, SUV tailgates) a plan wrap
        // is a set-back, not a drop.
        let (kf, kr) = self.glass_slopes();
        let wrap = (s.windshield_wrap_m.clamp(0.0, 0.20) * kf).min(WRAP_DROP_MAX_M) * gf
            + (s.rear_glass_wrap_m.clamp(0.0, 0.20) * kr).min(WRAP_DROP_MAX_M) * gr;
        crown - wrap * u * u
    }

    /// x (positive half widths) of ring points 8…17 at `y`, shared by the greenhouse and the
    /// flat-top layouts so neighbouring rings of either kind always line up. `xs` is the
    /// shoulder point's |x|, `gb` the side-glass base.
    fn top_xs(&self, y: f32, xs: f32, gb: f32) -> [f32; 10] {
        let rd = self.style.roof_rail_drop_m.clamp(0.02, 0.10);
        let gb = gb.min(xs - 0.01).max(0.05);
        let rw = self.roof_half_w(y.min(self.y_fw)).min(gb - 0.02).max(0.04);
        let aw = self.pillar_w(y);
        let x11 = rw - 0.25 * rd;
        let x12 = rw - (0.6 * rd).min(0.5 * aw);
        let x13 = (rw - aw).max(0.25 * rw).min(x12 - 0.005);
        [
            gb,
            0.5 * (gb + rw) + 0.008,
            rw,
            x11,
            x12,
            x13,
            0.66 * x13,
            0.40 * x13,
            0.18 * x13,
            0.0,
        ]
    }

    /// Greenhouse half width at roof height at `y` (A-pillar plan taper applied).
    pub fn roof_half_w(&self, y: f32) -> f32 {
        let taper = tan_deg(self.style.a_pillar_rake_deg.clamp(0.0, 25.0))
            * (y - (self.roof.1 - 0.2)).max(0.0);
        let taper_rear = tan_deg(self.style.c_pillar_rake_deg.clamp(0.0, 25.0))
            * ((self.roof.0 + 0.2) - y).max(0.0);
        let mut w = (self.roof_half - taper - taper_rear).max(0.45 * self.roof_half);
        let pt = self.style.roof_plan_taper.clamp(0.0, 0.3);
        if pt > 0.0 {
            let (r0, r1, _, _) = self.roof;
            let t = ((r1 - y) / (r1 - r0).max(0.05)).clamp(0.0, 1.0);
            w = (w * (1.0 - pt * t)).max(0.45 * self.roof_half);
        }
        w
    }

    /// True inside the open pickup bed (strictly between its end walls).
    pub fn bed_open(&self, y: f32) -> bool {
        matches!(self.bed, Some((y0, y1, _)) if y > y0 + 0.5 * BED_WALL_EPS && y < y1 - 0.5 * BED_WALL_EPS)
    }

    // ------------------------------------------------------------------ arches / sides

    /// Lowest z of the side at `y`: the wheel arch where there is one.
    pub fn arch_floor(&self, y: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for &(wy, wz, r) in &self.arches {
            let dy = (y - wy).abs();
            if dy < r {
                let z = match self.style.flare_shape {
                    ArchShape::Round => wz + (r * r - dy * dy).sqrt(),
                    ArchShape::Box => {
                        if dy < 0.6 * r {
                            wz + 0.95 * r
                        } else {
                            wz + 0.95 * r - (dy - 0.6 * r) * 1.6
                        }
                    }
                };
                best = Some(best.map_or(z, |b: f32| b.max(z)));
            }
        }
        best
    }

    /// Flare factor 0..1 around the arches at `y` (1 over the arch, fading 0.15 m past it).
    pub fn arch_t(&self, y: f32) -> f32 {
        self.arches
            .iter()
            .map(|&(wy, _, r)| {
                let d = (y - wy).abs() - r;
                if d <= 0.0 {
                    1.0
                } else {
                    smoothstep(1.0 - d / 0.15)
                }
            })
            .fold(0.0, f32::max)
    }

    /// Fender swell (haunch) at `y`, m.
    pub fn haunch(&self, y: f32) -> f32 {
        self.arches
            .iter()
            .map(|&(wy, _, r)| {
                let amount = if wy < 0.0 {
                    self.style.haunch
                } else {
                    self.style.front_haunch
                };
                amount.clamp(0.0, 0.08) * smoothstep(1.0 - (y - wy).abs() / (r + 0.45))
            })
            .fold(0.0, f32::max)
    }

    /// Hood / deck crown + bulges at `y` for a point at `u` = |x| / edge half width (0..1).
    fn top_crown(&self, y: f32, u: f32, roof: bool) -> f32 {
        let s = &self.style;
        let u = u.clamp(0.0, 1.0);
        if roof {
            return s.roof_crown.clamp(0.0, 0.05) * (1.0 - u * u);
        }
        let mut z = s.hood_crown.clamp(0.0, 0.05) * (1.0 - u * u);
        if y >= self.y_fw {
            let bw = s.hood_bulge_w.clamp(0.2, 0.7);
            let band = 1.0 - smoothstep((u - (bw - 0.08)) / 0.16);
            let span = (self.y_bf - self.y_fw).max(0.1);
            let rear = smoothstep((self.y_fw + 0.65 * span - y) / (0.35 * span));
            let fade = 1.0 - self.nose_t(y);
            z += (s.hood_bulge_h.clamp(0.0, 0.08) + s.power_bulge_h.clamp(0.0, 0.10) * rear)
                * band
                * fade;
        }
        // Valleys: the centre sinks, the outer edge (fender / haunch tops) stays.
        let valley_band = 1.0 - smoothstep((u - 0.55) / 0.35);
        if y >= self.y_fw {
            z -= s.hood_valley.clamp(0.0, 0.08) * valley_band * (1.0 - self.nose_t(y));
        } else if y <= self.y_deck {
            z -= s.deck_valley.clamp(0.0, 0.08) * valley_band * (1.0 - self.tail_t(y));
        }
        z
    }

    // ------------------------------------------------------------------ the ring

    /// Cross-section ring of the left half at station `y`, [`RING_POINTS`] points from the
    /// underside edge to the centre line top:
    /// `0` underside edge, `1` sill corner, `2` rocker, `3..=5` crease (below, on, above),
    /// `6` shoulder low, `7` shoulder ([`RING_SHOULDER`]), then either the greenhouse
    /// (`8` glass base, `9` glass mid, `10` glass top, `11` roof edge, `12..=16` roof, `17`
    /// centre) or a flat top (`8..=16` across the hood / deck, `17` centre) or an open bed
    /// (`8` rail inner edge, `9` wall bottom, `10..=16` floor, `17` centre).
    pub fn ring(&self, y: f32) -> Vec<RingPt> {
        let s = &self.style;
        let w = self.half_width(y);
        let sill = self.sill(y);
        let belt = self.belt(y).max(sill + 0.12);
        let top = self.top(y).max(belt + 0.015);
        let gh = !self.bed_open(y) && self.has_greenhouse(y);
        let h = belt - sill;
        let tuck = s.sill_tuck.clamp(0.01, 0.12).min(0.3 * h);
        let ch = s.shoulder_round.clamp(0.01, 0.12).min(0.3 * h);
        let height = self.roof.2.max(belt);
        let zc = clamp_soft(
            s.crease_z.clamp(0.25, 0.70) * height,
            sill + tuck + 0.06,
            belt - ch - 0.06,
        );
        let cd = s.crease_depth.clamp(0.0, 0.03);
        let fl = s.flare.clamp(0.0, 0.08) * self.arch_t(y);
        let hn = self.haunch(y);
        let lower = |x: f32, z: f32| RingPt {
            p: [-x, y, z],
            seg: Seg::Lower,
        };
        let mut pts = vec![
            lower(w - tuck, sill),
            lower(w + 0.3 * hn + fl, sill + tuck),
            lower(w + 0.5 * hn + fl, sill + 0.22 * h),
            lower(w + hn + 0.8 * fl - cd, zc - 0.05),
            lower(w + hn + 0.5 * fl, zc),
            lower(w + hn - cd, zc + 0.05),
            lower(w + hn, belt - ch),
            RingPt {
                p: [-(w + hn - 0.5 * ch), y, belt],
                seg: if gh { Seg::Ledge } else { Seg::Top },
            },
        ];
        // Wheel arches: side points below the arch move up onto it (flare lip outward).
        if let Some(arch) = self.arch_floor(y) {
            let arch = arch.min(belt - ch - 0.03);
            for pt in pts.iter_mut().take(6) {
                if pt.p[2] < arch {
                    pt.p[2] = arch;
                    pt.p[0] = -(w + hn + fl);
                }
            }
        }
        let xs = w + hn - 0.5 * ch;
        let gb = (w + 0.5 * hn - s.glass_inset.clamp(0.04, 0.20)).max(0.3);
        // Section shape (phase 3): sill tuck-in, side bulge, side sculpts.
        let ri = s.rocker_in_m.clamp(0.0, 0.08);
        if ri > 0.0 {
            pts[1].p[0] += ri;
            pts[2].p[0] += 0.6 * ri;
            pts[0].p[0] = pts[0].p[0].max(pts[1].p[0] + 0.005).min(-0.2);
        }
        if !s.side_sculpt.is_empty() || s.side_bulge_m > 0.0 || s.lower_lean_in_m > 0.0 {
            let fc = (zc - sill) / h.max(0.05);
            for (j, pt) in pts.iter_mut().enumerate() {
                let z = pt.p[2];
                let dx = self.side_offset(y, z, (z - sill) / h.max(0.05), fc);
                // The shoulder never crosses inside the glass base.
                let lim = if j == RING_SHOULDER {
                    -(gb + 0.01)
                } else {
                    -0.2
                };
                pt.p[0] = (pt.p[0] - dx).min(lim);
            }
        }
        let gh_k = if gh { self.greenhouse_weight(y) } else { 0.0 };
        let rise = self.fender_rise(y) * (1.0 - gh_k);
        if rise > 0.0 {
            pts[RING_SHOULDER - 1].p[2] += rise;
            pts[RING_SHOULDER].p[2] += rise;
        }
        let tx = self.top_xs(y, xs, gb);
        if gh {
            // Greenhouse: side glass 8→10, roof rail / pillar 10→13, roof / glass 13→17.
            // Blended toward the flat top while the greenhouse is low (windshield base, deck).
            let k = self.greenhouse_weight(y);
            let hg = (top - belt).max(0.0);
            let rw = tx[2].max(0.04);
            let rd = s.roof_rail_drop_m.clamp(0.02, 0.10).min(0.45 * hg);
            let gc = |x: f32| self.greenhouse_crown(y, x / rw);
            let fc =
                |x: f32| top + self.top_crown(y, (x / xs).min(1.0), false) + self.fender_top(y, x);
            let z8 = belt + 0.01f32.min(0.25 * hg);
            let z10 = (top - rd + gc(tx[2])).max(z8);
            let mut z = [
                z8,
                0.5 * (z8 + z10),
                z10,
                top - 0.35 * rd + gc(tx[3]),
                top - 0.08 * rd + gc(tx[4]),
                top + gc(tx[5]),
                top + gc(tx[6]),
                top + gc(tx[7]),
                top + gc(tx[8]),
                top + gc(0.0),
            ];
            for i in 1..6 {
                z[i] = z[i].max(z[i - 1]);
            }
            for (i, (&x, &zg)) in tx.iter().zip(z.iter()).enumerate() {
                pts.push(RingPt {
                    p: [-x, y, lerp(fc(x), zg, k)],
                    seg: if i < 2 { Seg::Glass } else { Seg::Top },
                });
            }
        } else if self.bed_open(y) {
            let (_, _, floor0) = self.bed.unwrap_or((0.0, 0.0, belt - 0.4));
            let floor = floor0.min(top - 0.1);
            let xr = xs - 0.07;
            let arch = self.arch_floor(y).map(|a| a + 0.03);
            let fz = |x: f32| match arch {
                Some(a) if x > xr - 0.32 => floor.max(a),
                _ => floor,
            };
            pts.push(RingPt {
                p: [-xr, y, top + rise],
                seg: Seg::BedWall,
            });
            pts.push(RingPt {
                p: [-xr, y, fz(xr)],
                seg: Seg::BedFloor,
            });
            for f in [0.86, 0.72, 0.58, 0.45, 0.32, 0.20, 0.08] {
                pts.push(RingPt {
                    p: [-f * xr, y, fz(f * xr)],
                    seg: Seg::BedFloor,
                });
            }
            pts.push(RingPt {
                p: [0.0, y, floor],
                seg: Seg::BedFloor,
            });
        } else {
            // Flat top (hood / deck) at the greenhouse's x positions.
            for &x in &tx {
                pts.push(RingPt {
                    p: [
                        -x,
                        y,
                        top + self.top_crown(y, (x / xs).min(1.0), false) + self.fender_top(y, x),
                    ],
                    seg: Seg::Top,
                });
            }
        }
        // End face profiles (phase 3): points ahead of a raked / undercut face move onto it.
        if let Some((lo, hi)) = self.face_band(y) {
            for pt in pts.iter_mut() {
                pt.p[2] = pt.p[2].max(lo).min(hi);
            }
        }
        debug_assert_eq!(pts.len(), RING_POINTS);
        pts
    }

    /// Point on the LEFT side surface at station `y`, height `z` (searching the side and
    /// greenhouse, ring points 0..=10), and its outward normal (in the x–z plane).
    pub fn side_point(&self, y: f32, z: f32) -> ([f32; 3], [f32; 3]) {
        let ring = self.ring(y);
        let n = 11.min(ring.len());
        for j in 0..n - 1 {
            let a = ring[j].p;
            let b = ring[j + 1].p;
            if (a[2] <= z && z <= b[2]) || (b[2] <= z && z <= a[2]) {
                let t = if (b[2] - a[2]).abs() < 1e-6 {
                    0.0
                } else {
                    (z - a[2]) / (b[2] - a[2])
                };
                return ([a[0] + (b[0] - a[0]) * t, y, z], seg_normal(a, b));
            }
        }
        let a = ring[RING_SHOULDER - 1].p;
        let b = ring[RING_SHOULDER].p;
        ([b[0], y, z], seg_normal(a, b))
    }

    /// Point on the top surface at station `y` above chassis `x` (either sign), and its
    /// outward normal. Searches from the shoulder to the centre.
    pub fn top_point(&self, y: f32, x: f32) -> ([f32; 3], [f32; 3]) {
        let ring = self.ring(y);
        let ax = x.abs();
        for j in RING_SHOULDER..ring.len() - 1 {
            let a = ring[j].p;
            let b = ring[j + 1].p;
            let (xa, xb) = (-a[0], -b[0]);
            if xa >= ax && ax >= xb && xa - xb > 1e-6 {
                let t = (xa - ax) / (xa - xb);
                let z = a[2] + (b[2] - a[2]) * t;
                let nrm = seg_normal(a, b);
                let nx = if x < 0.0 { nrm[0] } else { -nrm[0] };
                return ([x, y, z], [nx, 0.0, nrm[2]]);
            }
        }
        let c = ring[ring.len() - 1].p;
        ([x, y, c[2]], [0.0, 0.0, 1.0])
    }

    /// Highest point of the loft (roof top + crown), m.
    pub fn z_max(&self) -> f32 {
        self.roof.2.max(self.z_belt) + self.style.roof_crown.max(0.0) + 0.03
    }
}

/// Outward normal of a left-side ring segment `a → b` (x–z plane).
fn seg_normal(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let dx = b[0] - a[0];
    let dz = b[2] - a[2];
    let l = (dx * dx + dz * dz).sqrt().max(1e-6);
    [-dz / l, 0.0, dx / l]
}
