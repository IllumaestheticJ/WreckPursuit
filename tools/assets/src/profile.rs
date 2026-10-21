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

use crate::style::{ArchShape, BodyStyle, Silhouette};

/// Points per half cross-section ring (see [`BodyProfile::ring`]).
pub const RING_POINTS: usize = 18;
/// Ring index of the shoulder point (top of the side, at the belt).
pub const RING_SHOULDER: usize = 7;
/// Station offset of the paired stations that make vertical pickup-bed walls, m.
pub const BED_WALL_EPS: f32 = 0.004;

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
    /// Hood front edge y; ahead of it (below the belt) is the front bumper cover.
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
    /// Rear quarters start (cabin rear) / end (rear bumper begins) y.
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
        let y_nose = range(&fb, 1).1 + 0.05 + st.overhang_trim.0.clamp(-0.15, 0.10);
        let y_tail = range(&rb, 1).0 - 0.05 - st.overhang_trim.1.clamp(-0.15, 0.10);
        let z_hood = (
            hood_z_at(y_fw) + dz + belt_dz,
            hood_z_at(y_bf) + dz + 0.3 * belt_dz,
        );
        let z_belt = z_belt + dz + belt_dz;
        let z_deck = z_deck + dz + belt_dz;
        let z_floor = range(&all, 2).0 + dz;
        let z_door_bottom = z_door_bottom + dz;
        let rz = roof_z + dz + st.roof_dz.clamp(-0.12, 0.10);

        // --- Cabin placement ---
        let shift = st.cabin_shift_m.clamp(-0.55, 0.20);
        let mut v_fw = (y_fw + shift).min(y_bf - 0.30);
        let mut r1 = ry1 + shift;
        let mut r0 = ry0 + shift;
        let v_deck = (y_deck + shift).clamp(y_tail + 0.25, r0 - 0.05);
        if let Some(len) = st.hood_length {
            let want = (y_nose - len).clamp(r0 + 0.6, y_bf - 0.30);
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
        r1 = r1.clamp(r0 + 0.25, v_fw - 0.10);
        r0 = r0.clamp(v_deck + 0.06, r1 - 0.25);

        let roof_half = match st.tumblehome {
            Some(t) => half_w - t.clamp(0.10, 0.30),
            None => (roof_hw + 0.05).min(half_w - 0.12),
        };
        let nose_h = st.nose_h.map_or(z_hood.1 - 0.10, |h| h + dz);
        let z_nose = nose_h.clamp(z_floor + 0.15, z_hood.1);
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
        self.half_w - ni * n - ti * t
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
    /// deck, or just above the belt (hood).
    pub fn top(&self, y: f32) -> f32 {
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

    /// True where the greenhouse (cabin top) rises above the belt.
    pub fn has_greenhouse(&self, y: f32) -> bool {
        self.top(y) > self.belt(y) + 0.06
    }

    /// Greenhouse half width at roof height at `y` (A-pillar plan taper applied).
    pub fn roof_half_w(&self, y: f32) -> f32 {
        let taper = tan_deg(self.style.a_pillar_rake_deg.clamp(0.0, 25.0))
            * (y - (self.roof.1 - 0.2)).max(0.0);
        (self.roof_half - taper).max(0.45 * self.roof_half)
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
        let gh = top > belt + 0.06;
        let h = belt - sill;
        let tuck = s.sill_tuck.clamp(0.01, 0.12).min(0.3 * h);
        let ch = s.shoulder_round.clamp(0.01, 0.12).min(0.3 * h);
        let height = self.roof.2.max(belt);
        let zc =
            (s.crease_z.clamp(0.25, 0.70) * height).clamp(sill + tuck + 0.06, belt - ch - 0.06);
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
        if gh {
            let gb = (w + 0.5 * hn - s.glass_inset.clamp(0.04, 0.20)).max(0.3);
            let rw = self.roof_half_w(y).min(gb - 0.02);
            let rch = ch.min(0.5 * (top - belt));
            let gmid = 0.5 * (gb + rw) + 0.008;
            pts.push(RingPt {
                p: [-gb, y, belt + 0.01],
                seg: Seg::Glass,
            });
            pts.push(RingPt {
                p: [-gmid, y, 0.5 * (belt + 0.01 + top - rch)],
                seg: Seg::Glass,
            });
            pts.push(RingPt {
                p: [-rw, y, top - rch],
                seg: Seg::Top,
            });
            pts.push(RingPt {
                p: [-(rw - rch), y, top + self.top_crown(y, 0.9, true)],
                seg: Seg::Top,
            });
            for f in [0.80, 0.62, 0.45, 0.28, 0.12] {
                pts.push(RingPt {
                    p: [-f * rw, y, top + self.top_crown(y, f, true)],
                    seg: Seg::Top,
                });
            }
            pts.push(RingPt {
                p: [0.0, y, top + self.top_crown(y, 0.0, true)],
                seg: Seg::Top,
            });
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
                p: [-xr, y, top],
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
            for f in [0.93, 0.85, 0.76, 0.66, 0.54, 0.42, 0.30, 0.18, 0.08] {
                pts.push(RingPt {
                    p: [-f * xs, y, top + self.top_crown(y, f, false)],
                    seg: Seg::Top,
                });
            }
            pts.push(RingPt {
                p: [0.0, y, top + self.top_crown(y, 0.0, false)],
                seg: Seg::Top,
            });
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
