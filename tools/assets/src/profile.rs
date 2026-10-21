//! Body profile: the shape a car's procedural mesh is lofted from. The *anchors* (where the
//! hood, doors, roof, glass and trunk start and end) are measured from the car's lattice so
//! every panel mesh lines up with the lattice region it is skinned to and hinged panels pivot
//! on their real hinge nodes; the *shape between the anchors* (nose and tail, flares, creases,
//! hood bulge, glass curvature, beds and boxes) comes from the car's style sheet
//! ([`crate::style::BodyShape`]).

use wp_contracts::{BodyRegion, LatticeDef, VehicleDef};

use crate::style::{ArchShape, BedKind, BodyShape, CarStyle};

/// Measured anchors (chassis space, m; `y` grows forward) plus the style's shape knobs.
#[derive(Debug, Clone)]
pub struct BodyProfile {
    /// Body half width.
    pub half_w: f32,
    /// Mesh nose y.
    pub y_nose: f32,
    /// Mesh tail y.
    pub y_tail: f32,
    /// Underside height.
    pub z_floor: f32,
    /// Firewall = hood hinge line y (hood rear edge, windshield base).
    pub y_fw: f32,
    /// Hood front edge y; ahead of it (below the belt) is the front bumper cover.
    pub y_bf: f32,
    /// Hood height at the firewall / at its front edge.
    pub z_hood: (f32, f32),
    /// Cabin belt line (door tops).
    pub z_belt: f32,
    /// Door bottoms (below: sills).
    pub z_door_bottom: f32,
    /// Visible roof y range (rear, front; the lattice roof minus the style's glass rakes),
    /// height, half width.
    pub roof: (f32, f32, f32, f32),
    /// Deck (trunk lid / bed) start y. Rear glass runs roof → deck start.
    pub y_deck: f32,
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
    /// Wheel arches: (y, z centre, arch radius, front axle). Visual extra axles included.
    pub arches: Vec<(f32, f32, f32, bool)>,
    /// Lattice has an `Accessory` region (police push bar).
    pub has_accessory: bool,
    /// Shape knobs.
    pub s: BodyShape,
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

/// Hermite smoothstep on `x` clamped to 0..1.
pub fn smoothstep(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear interpolation with `t` clamped to 0..1.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// 4u(1-u): 0 at the ends, 1 in the middle.
fn hump(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    4.0 * u * (1.0 - u)
}

impl BodyProfile {
    /// Measure a profile. Panics only on lattices without the standard regions (every
    /// `gen-lattice` body has them).
    pub fn measure(def: &VehicleDef, l: &LatticeDef, style: &CarStyle) -> BodyProfile {
        let all: Vec<[f32; 3]> = l.nodes.iter().map(|n| n.pos_m).collect();
        let half_w = range(&all, 0).1.max(-range(&all, 0).0);
        let z_floor = range(&all, 2).0;
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
        // Visible roof: the style may rake the glass into the lattice roof zone.
        let s0 = &style.body;
        let vy0 = (ry0 + s0.rear_rake).min(ry1 - 0.3);
        let vy1 = (ry1 - s0.ws_rake).max(vy0 + 0.3);
        let roof = (vy0, vy1, range(&roof_n, 2).1, range(&roof_n, 0).1);
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
        let s = style.body.clone();
        let mut arches = Vec::new();
        for (i, w) in def.wheels.iter().enumerate() {
            let front = i < 2;
            let r = w.radius_m + s.arch_gap;
            if !front && !style.wheels.extra_axles.is_empty() {
                for &dy in &style.wheels.extra_axles {
                    arches.push((w.position_m[1] + dy, w.position_m[2], r, false));
                }
            } else {
                arches.push((w.position_m[1], w.position_m[2], r, front));
            }
        }
        arches.sort_by(|a, b| a.0.total_cmp(&b.0));
        arches.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-4);
        BodyProfile {
            half_w,
            y_nose: range(&fb, 1).1 + 0.05,
            y_tail: range(&rb, 1).0 - 0.05,
            z_floor,
            y_fw,
            y_bf,
            z_hood: (hood_z_at(y_fw), hood_z_at(y_bf)),
            z_belt,
            z_door_bottom,
            roof,
            y_deck,
            z_deck,
            trunk_lid,
            rear_glass,
            y_quarter: (qy1, qy0),
            y_fd_hinge: fd_hinge,
            rear_door,
            y_fd_rear,
            z_bumper_top: (range(&fb, 2).1, range(&rb, 2).1),
            arches,
            has_accessory: l.nodes.iter().any(|n| n.region == BodyRegion::Accessory),
            s,
        }
    }

    /// Body length.
    pub fn length(&self) -> f32 {
        self.y_nose - self.y_tail
    }

    /// 0 → 1 inside the nose rounding zone.
    pub fn nose_t(&self, y: f32) -> f32 {
        let d = self.y_nose - y;
        smoothstep(1.0 - d / self.s.nose_len.max(0.01)).powf(self.s.nose_sharp)
    }

    /// 0 → 1 inside the tail rounding zone.
    pub fn tail_t(&self, y: f32) -> f32 {
        let d = y - self.y_tail;
        smoothstep(1.0 - d / self.s.tail_len.max(0.01)).powf(self.s.tail_sharp)
    }

    /// True behind the cab where the style replaces the lattice deck (open bed, box, rails,
    /// tipper).
    pub fn in_bed(&self, y: f32) -> bool {
        self.s.bed != BedKind::Closed && y < self.y_deck - self.s.bed_gap
    }

    /// True in the gap between the cab back and a box/bed that starts behind it.
    pub fn in_bed_gap(&self, y: f32) -> bool {
        self.s.bed != BedKind::Closed && y < self.y_deck && y >= self.y_deck - self.s.bed_gap
    }

    /// Body half width at `y` before flares (rounded in plan view at the ends).
    pub fn half_width(&self, y: f32) -> f32 {
        let base =
            self.half_w - self.s.nose_plan * self.nose_t(y) - self.s.tail_plan * self.tail_t(y);
        if self.s.bed == BedKind::Rails && (self.in_bed(y) || self.in_bed_gap(y)) {
            return self.s.bed_half_w.max(0.2);
        }
        if matches!(self.s.bed, BedKind::Box | BedKind::Tipper)
            && self.in_bed(y)
            && self.s.bed_half_w > 0.0
        {
            return self.s.bed_half_w - self.s.tail_plan * self.tail_t(y);
        }
        base
    }

    /// Fender flare (outward) at `y`.
    pub fn flare(&self, y: f32) -> f32 {
        let mut f: f32 = 0.0;
        for &(wy, _, r, front) in &self.arches {
            let amount = if front {
                self.s.flare_f
            } else {
                self.s.flare_r
            };
            if amount == 0.0 {
                continue;
            }
            let reach = r + 0.35;
            f = f.max(amount * smoothstep(1.0 - (y - wy).abs() / reach));
        }
        if self.s.bed == BedKind::Rails && (self.in_bed(y) || self.in_bed_gap(y)) {
            return 0.0;
        }
        f
    }

    /// Underside height at `y` (rises toward the ends: approach/departure chamfer).
    pub fn sill(&self, y: f32) -> f32 {
        self.z_floor + self.s.nose_lift * self.nose_t(y) + self.s.tail_lift * self.tail_t(y)
    }

    /// Shoulder (top of the side panels) at `y`.
    pub fn belt(&self, y: f32) -> f32 {
        let s = &self.s;
        let base = if y >= self.y_fw {
            lerp(
                self.z_hood.0,
                self.z_hood.1,
                (y - self.y_fw) / (self.y_bf - self.y_fw).max(0.1),
            )
        } else if y >= self.y_quarter.0 {
            self.z_belt
        } else {
            // Blend from the belt to the deck over the first 0.3 m of the quarter.
            lerp(self.z_belt, self.z_deck, (self.y_quarter.0 - y) / 0.3)
        };
        let mut z = base - s.nose_drop * self.nose_t(y) - s.tail_drop * self.tail_t(y);
        if y < self.y_fd_hinge {
            let span = (self.y_fd_hinge - self.y_quarter.0).max(0.3);
            z += s.belt_rise * ((self.y_fd_hinge - y) / span).clamp(0.0, 1.0);
        }
        if y < self.y_quarter.0 {
            z += s.belt_kick * smoothstep((self.y_quarter.0 - y) / 0.45);
        }
        z + s.tail_kick * smoothstep(1.0 - (y - self.y_tail) / 0.7)
    }

    /// Roof height at `y` inside the roof zone (with the longitudinal bow).
    fn roof_z(&self, y: f32) -> f32 {
        let (ry0, ry1, rz, _) = self.roof;
        let u = (y - ry0) / (ry1 - ry0).max(0.05);
        rz - self.s.roof_bow * (1.0 - hump(u))
    }

    /// Top of the body at `y`: the roof, glass slopes, or just above the belt.
    pub fn top(&self, y: f32) -> f32 {
        let (ry0, ry1, _, _) = self.roof;
        let flat = self.belt(y) + 0.015;
        if self.in_bed(y) {
            return match self.s.bed {
                BedKind::Box | BedKind::Tipper => self.s.bed_height.max(flat),
                BedKind::Rails => self.s.bed_height,
                _ => flat,
            };
        }
        if y <= ry1 && y >= ry0 {
            self.roof_z(y)
        } else if y > ry1 && y < self.y_fw {
            let t = (y - ry1) / (self.y_fw - ry1);
            lerp(self.roof_z(ry1), self.belt(self.y_fw) + 0.015, t) + self.s.ws_bow * hump(t)
        } else if y < ry0 && y > self.y_deck {
            let t = (ry0 - y) / (ry0 - self.y_deck).max(0.05);
            lerp(self.roof_z(ry0), self.belt(self.y_deck) + 0.015, t) + self.s.rear_bow * hump(t)
        } else {
            flat
        }
    }

    /// Hood centre bulge at `y` (0 outside the hood).
    pub fn hood_bulge(&self, y: f32) -> f32 {
        if y < self.y_fw || self.s.hood_bulge == 0.0 {
            return 0.0;
        }
        let u = (y - self.y_fw) / (self.y_bf - self.y_fw).max(0.1);
        self.s.hood_bulge * smoothstep(u / 0.2) * smoothstep((1.0 - u) / 0.35)
    }

    /// True where the greenhouse (cabin top) rises above the belt.
    pub fn has_greenhouse(&self, y: f32) -> bool {
        !self.in_bed(y) && self.top(y) > self.belt(y) + 0.06
    }

    /// Greenhouse half width at roof height.
    pub fn roof_half_w(&self) -> f32 {
        ((self.roof.3 + 0.05) * self.s.roof_w).min(self.half_w - 0.08)
    }

    /// Station y values: uniform steps plus every zone boundary (so panel splits fall on
    /// station edges), ascending.
    pub fn stations(&self, step: f32) -> Vec<f32> {
        let mut ys = Vec::new();
        let n = ((self.y_nose - self.y_tail) / step).ceil().max(2.0) as usize;
        for i in 0..=n {
            ys.push(self.y_tail + (self.y_nose - self.y_tail) * i as f32 / n as f32);
        }
        let mut keys = vec![
            self.y_fw,
            self.y_bf,
            self.roof.0,
            self.roof.1,
            self.y_deck,
            self.y_quarter.0,
            self.y_quarter.1,
            self.y_fd_hinge,
            self.y_fd_rear,
        ];
        if self.s.bed != BedKind::Closed {
            // Sharp cab-back / box-front walls: a key just either side of the step.
            let yb = self.y_deck - self.s.bed_gap;
            keys.extend([yb + 0.012, yb - 0.012, self.y_deck + 0.012]);
        }
        if let Some((h, e)) = self.rear_door {
            keys.push(h);
            keys.push(e);
        }
        for &(wy, _, r, _) in &self.arches {
            keys.push(wy - r);
            keys.push(wy + r);
            keys.push(wy);
        }
        for k in keys {
            if k > self.y_tail + 0.02 && k < self.y_nose - 0.02 {
                ys.retain(|y| (y - k).abs() > 0.01);
                ys.push(k);
            }
        }
        ys.sort_by(|a, b| a.total_cmp(b));
        ys.dedup_by(|a, b| (*a - *b).abs() < 0.005);
        ys
    }

    /// Lowest z of the side at `y`: the wheel arch where there is one.
    pub fn arch_floor(&self, y: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for &(wy, wz, r, _) in &self.arches {
            let q = ((y - wy) / r).abs();
            if q >= 1.0 {
                continue;
            }
            let k = match self.s.arch {
                ArchShape::Round => (1.0 - q * q).sqrt(),
                ArchShape::FlatTop => (1.0 - q * q * q * q).sqrt().sqrt(),
                ArchShape::Square => {
                    if q < 0.55 {
                        1.0
                    } else {
                        (1.0 - q) / 0.45
                    }
                }
            };
            let z = wz + r * k;
            best = Some(best.map_or(z, |b: f32| b.max(z)));
        }
        best
    }
}
