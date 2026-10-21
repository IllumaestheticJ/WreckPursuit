//! Police equipment geometry (`research_police.md` §1 "Shared equipment kit").
//!
//! Every item is a [`crate::style::DetailFn`]: it looks up the car's [`Kit`] by id and places
//! itself on the styled [`BodyProfile`], so one function serves all five versions. Sizes are
//! the research values (metres); positions along the car are `%L` (0 = front bumper, 1 = rear
//! bumper) or profile anchors (roof edges, glass lines, door stations).
//!
//! Lens parts of light bars / hidden LEDs are emitted per side (`_l` at x < 0, `_r` at x > 0)
//! with `LightbarRed` / `LightbarBlue` / `LightbarClear` so the renderer flashes the sides
//! independently (`PanelBody.lightbarMaterials.{left,right}`).

use wp_contracts::BodyRegion;

use crate::builder::{Builder, PartKey, Patch, PatchBand};
use crate::mesh::{MaterialSlot, PartKind};
use crate::profile::BodyProfile;
use crate::style::DetailCtx;

/// Roof light bar type (`research_police.md` §4.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bar {
    /// L-2010 low-profile LED bar: length × depth × lens height (m), on 0.03 m feet.
    Led {
        /// Length across the car.
        len: f32,
        /// Depth along the car.
        depth: f32,
        /// Lens height.
        h: f32,
    },
    /// L-80s twin-rotator bar: base 1.22 × 0.30 × 0.06, two domes per side, clear centre.
    Rotator,
    /// L-SLICK: no roof bar (hidden LEDs only).
    Slick,
}

/// Front push bar type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Push {
    /// Tube push bar: `uprights` (2 or 4) between a lower rail and a top rail (`rails` = 2)
    /// or a single lower rail with short uprights (`rails` = 1, PIT bumper).
    Bar {
        /// Overall width, m.
        width: f32,
        /// Lower rail height, m.
        z_lo: f32,
        /// Top rail / upright top height, m.
        z_hi: f32,
        /// 2 or 4 uprights.
        uprights: u8,
        /// 1 or 2 horizontal rails.
        rails: u8,
        /// Two forward LED cubes on the top rail.
        leds: bool,
    },
    /// 80s rubber-faced bumper guards (two posts).
    Guards,
}

/// A-pillar spotlight(s).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    /// Both A-pillars (else the driver side, left, only).
    pub both: bool,
    /// Housing radius, m.
    pub r: f32,
    /// Chrome housing (80s) or black (modern).
    pub chrome: bool,
}

/// Rear spoiler.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spoiler {
    /// None.
    None,
    /// Small decklid lip, body colour (livery).
    Lip,
    /// Pedestal wing with a red / blue LED strip on the trailing edge.
    Wing,
}

/// Where the side unit-number decal carrier sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitDecal {
    /// Centre along the car, %L.
    pub pct: f32,
    /// Bottom / top height, m.
    pub z: (f32, f32),
    /// Half length along the car, m.
    pub half_len: f32,
}

/// Equipment of one police version.
#[derive(Debug, Clone, PartialEq)]
pub struct Kit {
    /// Roof bar.
    pub bar: Bar,
    /// Bar centre, %L (clamped onto the roof).
    pub bar_pct: f32,
    /// Push bar.
    pub push: Option<Push>,
    /// Spotlight(s).
    pub spot: Spot,
    /// Antenna pucks: (distance forward of the roof's rear edge m, x m).
    pub pucks: &'static [(f32, f32)],
    /// Whip antennas: (%L, x m, length m).
    pub whips: &'static [(f32, f32, f32)],
    /// Visor LED bar inside the windshield top.
    pub visor: bool,
    /// LED bar inside the rear-glass top (red / amber / blue).
    pub rear_bar: bool,
    /// Amber traffic advisor on the rear parcel shelf.
    pub advisor: bool,
    /// Red / blue LED modules in the grille.
    pub grille_leds: bool,
    /// Rear spoiler.
    pub spoiler: Spoiler,
    /// Carbon front splitter.
    pub splitter: bool,
    /// Chrome rub strip along the sides at z 0.55 (80s).
    pub rub_strip: bool,
    /// Tow hooks under the front bumper.
    pub tow_hooks: bool,
    /// Region (left side) of the side unit-number decal carrier.
    pub unit_region: BodyRegion,
    /// Side unit-number decal carrier.
    pub unit: UnitDecal,
    /// Unit-number decal carrier on the trunk lid.
    pub trunk_number: bool,
}

/// The kit of a police car id (`sentinel_cruiser` for unknown ids).
pub fn kit(id: &str) -> Kit {
    let base = Kit {
        bar: Bar::Led {
            len: 1.22,
            depth: 0.30,
            h: 0.055,
        },
        bar_pct: 0.44,
        push: Some(Push::Bar {
            width: 1.10,
            z_lo: 0.40,
            z_hi: 0.78,
            uprights: 2,
            rails: 2,
            leds: false,
        }),
        spot: Spot {
            both: false,
            r: 0.065,
            chrome: false,
        },
        pucks: &[(0.07, -0.25), (0.07, 0.0), (0.07, 0.25)],
        whips: &[(0.90, 0.35, 0.75)],
        visor: false,
        rear_bar: false,
        advisor: true,
        grille_leds: true,
        spoiler: Spoiler::None,
        splitter: false,
        rub_strip: false,
        tow_hooks: false,
        unit_region: BodyRegion::RearLeftQuarter,
        unit: UnitDecal {
            pct: 0.80,
            z: (0.66, 0.84),
            half_len: 0.25,
        },
        trunk_number: true,
    };
    match id {
        "sentinel_warden" => Kit {
            bar: Bar::Rotator,
            bar_pct: 0.45,
            push: Some(Push::Guards),
            spot: Spot {
                both: true,
                r: 0.075,
                chrome: true,
            },
            pucks: &[],
            whips: &[(0.93, 0.0, 0.80), (0.86, 0.80, 0.60)],
            advisor: false,
            grille_leds: false,
            rub_strip: true,
            unit_region: BodyRegion::FrontLeftFender,
            unit: UnitDecal {
                pct: 0.315,
                z: (0.62, 0.78),
                half_len: 0.20,
            },
            ..base
        },
        "sentinel_wolfhound" => Kit {
            bar: Bar::Led {
                len: 1.35,
                depth: 0.30,
                h: 0.055,
            },
            bar_pct: 0.43,
            push: Some(Push::Bar {
                width: 1.15,
                z_lo: 0.40,
                z_hi: 0.80,
                uprights: 2,
                rails: 2,
                leds: true,
            }),
            spot: Spot {
                both: false,
                r: 0.06,
                chrome: false,
            },
            pucks: &[(0.06, -0.22), (0.06, 0.22), (0.18, -0.22), (0.18, 0.22)],
            whips: &[],
            visor: true,
            rear_bar: true,
            advisor: false,
            spoiler: Spoiler::Lip,
            unit: UnitDecal {
                pct: 0.815,
                z: (0.70, 0.88),
                half_len: 0.25,
            },
            ..base
        },
        "sentinel_bastion" => Kit {
            bar: Bar::Led {
                len: 1.37,
                depth: 0.30,
                h: 0.06,
            },
            bar_pct: 0.33,
            push: Some(Push::Bar {
                width: 1.30,
                z_lo: 0.48,
                z_hi: 1.00,
                uprights: 4,
                rails: 2,
                leds: true,
            }),
            spot: Spot {
                both: false,
                r: 0.06,
                chrome: false,
            },
            pucks: &[(0.10, -0.24), (0.10, 0.24), (0.30, -0.24), (0.30, 0.24)],
            whips: &[],
            rear_bar: true,
            advisor: false,
            grille_leds: true,
            tow_hooks: true,
            unit: UnitDecal {
                pct: 0.84,
                z: (0.78, 0.97),
                half_len: 0.26,
            },
            trunk_number: false,
            ..base
        },
        "sentinel_harrier" => Kit {
            bar: Bar::Slick,
            push: Some(Push::Bar {
                width: 0.95,
                z_lo: 0.38,
                z_hi: 0.62,
                uprights: 2,
                rails: 1,
                leds: false,
            }),
            spot: Spot {
                both: false,
                r: 0.05,
                chrome: false,
            },
            pucks: &[(0.06, -0.15), (0.06, 0.15)],
            whips: &[],
            visor: true,
            rear_bar: true,
            advisor: false,
            spoiler: Spoiler::Wing,
            splitter: true,
            unit: UnitDecal {
                pct: 0.895,
                z: (0.50, 0.66),
                half_len: 0.20,
            },
            trunk_number: false,
            ..base
        },
        _ => base,
    }
}

// ---------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------

/// Chassis y at `pct` of the body length from the front bumper.
pub fn y_at(p: &BodyProfile, pct: f32) -> f32 {
    p.y_nose - pct * (p.y_nose - p.y_tail)
}

fn roof_z(p: &BodyProfile, y: f32, x: f32) -> f32 {
    p.top_point(y, x).0[2]
}

fn key(name: &str, region: BodyRegion, kind: PartKind, mat: MaterialSlot) -> PartKey {
    PartKey::body(name, region, kind, mat)
}

/// Interval `a..b` mirrored to side `sx` (−1 left, +1 right), as (lo, hi).
fn xs(sx: f32, a: f32, b: f32) -> (f32, f32) {
    let (u, v) = (sx * a, sx * b);
    (u.min(v), u.max(v))
}

/// Roof bar centre y: the kit's %L clamped onto the roof.
pub fn bar_y(p: &BodyProfile, k: &Kit) -> f32 {
    let (rear, front) = (p.roof.0, p.roof.1);
    y_at(p, k.bar_pct).clamp(rear + 0.35, (front - 0.17).max(rear + 0.35))
}

/// Windshield surface y at height `z` (straight line from the cowl to the roof header).
fn windshield_y(p: &BodyProfile, z: f32) -> f32 {
    let (z0, z1) = (p.z_hood.0, p.roof.2);
    let t = ((z - z0) / (z1 - z0).max(1e-3)).clamp(0.0, 1.0);
    p.y_fw + (p.roof.1 - p.y_fw) * t
}

/// Rear-glass surface y at height `z` (straight line from the roof's rear edge to the deck).
fn rear_glass_y(p: &BodyProfile, z: f32) -> f32 {
    let t = ((p.roof.2 - z) / (p.roof.2 - p.z_deck).max(1e-3)).clamp(0.0, 1.0);
    p.roof.0 + (p.y_deck - p.roof.0) * t
}

/// Prism along x (`x0..x1`) of a convex polygon in the (y, z) plane.
fn prism_x(b: &mut Builder, k: &PartKey, x0: f32, x1: f32, yz: &[[f32; 2]]) {
    let n = yz.len();
    let (cy, cz) = (
        yz.iter().map(|q| q[0]).sum::<f32>() / n as f32,
        yz.iter().map(|q| q[1]).sum::<f32>() / n as f32,
    );
    let at = |x: f32, q: [f32; 2]| [x, q[0], q[1]];
    let left: Vec<[f32; 3]> = yz.iter().map(|&q| at(x0, q)).collect();
    let right: Vec<[f32; 3]> = yz.iter().map(|&q| at(x1, q)).collect();
    b.poly_facing(k, &left, [-1.0, 0.0, 0.0]);
    b.poly_facing(k, &right, [1.0, 0.0, 0.0]);
    for i in 0..n {
        let j = (i + 1) % n;
        let (my, mz) = (0.5 * (yz[i][0] + yz[j][0]), 0.5 * (yz[i][1] + yz[j][1]));
        b.poly_facing(
            k,
            &[left[i], left[j], right[j], right[i]],
            [0.0, my - cy, mz - cz],
        );
    }
}

// ---------------------------------------------------------------------------------------
// Light bars
// ---------------------------------------------------------------------------------------

/// Bar mounting height: above the roof under the whole bar footprint, + `feet` m.
fn bar_base_z(p: &BodyProfile, yb: f32, half: f32, depth: f32, feet: f32) -> f32 {
    let mut z = f32::MIN;
    for y in [yb - depth / 2.0, yb, yb + depth / 2.0] {
        for x in [0.0, 0.5 * half, 0.9 * half] {
            z = z.max(roof_z(p, y, x));
        }
    }
    z + feet
}

/// Feet / gutter clamps from the roof to the bar base at x = ±`x`.
fn bar_feet(p: &BodyProfile, b: &mut Builder, k: &PartKey, yb: f32, x: f32, zb: f32) {
    for sx in [-1.0f32, 1.0] {
        let xc = sx * x;
        let zr = roof_z(p, yb, xc) - 0.01;
        b.cuboid(k, [xc - 0.04, yb - 0.11, zr], [xc + 0.04, yb + 0.11, zb]);
    }
}

/// L-2010 low LED bar: black base on feet, per side 1 clear takedown module + 5 coloured
/// modules (red left, blue right) with black dividers.
pub fn led_bar(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let k = kit(&ctx.def.id);
    let Bar::Led { len, depth, h } = k.bar else {
        return;
    };
    let p = ctx.profile;
    let yb = bar_y(p, &k);
    let half = len / 2.0;
    let zb = bar_base_z(p, yb, half, depth, 0.03);
    let feet = key(
        "lightbar_feet",
        BodyRegion::Roof,
        PartKind::Lightbar,
        MaterialSlot::Trim,
    );
    bar_feet(p, b, &feet, yb, half - 0.12, zb + 0.002);
    let (y0, y1) = (yb - depth / 2.0, yb + depth / 2.0);
    b.beveled_box(k0, [-half, y0, zb], [half, y1, zb + 0.014], 0.006);
    let div = key(
        "lightbar_dividers",
        BodyRegion::Roof,
        PartKind::Lightbar,
        MaterialSlot::Trim,
    );
    let zt = zb + 0.014 + h;
    let td_w = 0.17;
    for (side, sx) in [("l", -1.0f32), ("r", 1.0)] {
        let colour = if sx < 0.0 {
            MaterialSlot::LightbarRed
        } else {
            MaterialSlot::LightbarBlue
        };
        let lens = key(
            &format!("lightbar_lens_{side}"),
            BodyRegion::Roof,
            PartKind::Lightbar,
            colour,
        );
        let td = key(
            &format!("lightbar_takedown_{side}"),
            BodyRegion::Roof,
            PartKind::Lightbar,
            MaterialSlot::LightbarClear,
        );
        let (a, c) = xs(sx, 0.004, td_w);
        b.beveled_box(&td, [a, y0 + 0.008, zb + 0.014], [c, y1 - 0.008, zt], 0.008);
        let (a, c) = xs(sx, td_w + 0.006, half - 0.004);
        b.beveled_box(
            &lens,
            [a, y0 + 0.006, zb + 0.014],
            [c, y1 - 0.006, zt],
            0.012,
        );
        // Dividers between the 5 coloured modules (+ between takedown and lens).
        let span = half - 0.004 - (td_w + 0.006);
        for i in 0..5 {
            let x = sx * (td_w + 0.003 + span * i as f32 / 5.0);
            b.cuboid(
                &div,
                [x - 0.004, y0 + 0.003, zb + 0.012],
                [x + 0.004, y1 - 0.003, zt + 0.002],
            );
        }
    }
    b.cuboid(
        &div,
        [-0.004, y0 + 0.003, zb + 0.012],
        [0.004, y1 - 0.003, zt + 0.002],
    );
}

/// L-80s twin-rotator bar: base, two domes per side (red left, blue right) with chrome
/// reflectors inside, clear centre box with takedowns, chrome gutter clamps.
pub fn rotator_bar(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let k = kit(&ctx.def.id);
    let p = ctx.profile;
    let yb = bar_y(p, &k);
    let (half, depth) = (0.61, 0.30);
    let zb = bar_base_z(p, yb, half, depth, 0.04);
    let feet = key(
        "lightbar_feet",
        BodyRegion::Roof,
        PartKind::Lightbar,
        MaterialSlot::Chrome,
    );
    bar_feet(p, b, &feet, yb, half - 0.08, zb + 0.002);
    let (y0, y1) = (yb - depth / 2.0, yb + depth / 2.0);
    b.beveled_box(k0, [-half, y0, zb], [half, y1, zb + 0.06], 0.012);
    let refl = key(
        "lightbar_reflectors",
        BodyRegion::Roof,
        PartKind::Lightbar,
        MaterialSlot::Chrome,
    );
    let profile = [
        [0.105, 0.0],
        [0.105, 0.05],
        [0.096, 0.09],
        [0.072, 0.118],
        [0.036, 0.134],
        [0.0, 0.138],
    ];
    for (side, sx) in [("l", -1.0f32), ("r", 1.0)] {
        let colour = if sx < 0.0 {
            MaterialSlot::LightbarRed
        } else {
            MaterialSlot::LightbarBlue
        };
        let dome = key(
            &format!("lightbar_dome_{side}"),
            BodyRegion::Roof,
            PartKind::Lightbar,
            colour,
        );
        for x in [0.30f32, 0.50] {
            let c = [sx * x, yb, zb + 0.06];
            b.lathe(&dome, c, [0.0, 0.0, 1.0], &profile, 12);
            b.cuboid(
                &refl,
                [c[0] - 0.05, yb - 0.008, c[2] + 0.01],
                [c[0] + 0.05, yb + 0.008, c[2] + 0.10],
            );
        }
        let td = key(
            &format!("lightbar_takedown_{side}"),
            BodyRegion::Roof,
            PartKind::Lightbar,
            MaterialSlot::LightbarClear,
        );
        let (a, c) = xs(sx, 0.004, 0.18);
        b.beveled_box(
            &td,
            [a, yb - 0.10, zb + 0.06],
            [c, yb + 0.10, zb + 0.15],
            0.012,
        );
    }
}

// ---------------------------------------------------------------------------------------
// Front end
// ---------------------------------------------------------------------------------------

/// Push bar (tube uprights + rails, rubber pads, mounts, optional LED cubes) or 80s bumper
/// guards. Region `Accessory` (the lattice's push-bar nodes; detaches as one group).
pub fn push_bar(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let k = kit(&ctx.def.id);
    let p = ctx.profile;
    let pads = key(
        "pushbar_pads",
        BodyRegion::Accessory,
        PartKind::Detail,
        MaterialSlot::Rubber,
    );
    let yn = p.y_nose;
    let Some(push) = k.push else {
        return;
    };
    let Push::Bar {
        width,
        z_lo,
        z_hi,
        uprights,
        rails,
        leds,
    } = push
    else {
        // Bumper guards: steel post + rubber face, x = ±0.35.
        for sx in [-1.0f32, 1.0] {
            let x = sx * 0.35;
            b.cuboid(k0, [x - 0.04, yn - 0.06, 0.40], [x + 0.04, yn + 0.06, 0.62]);
            b.beveled_box(
                &pads,
                [x - 0.06, yn + 0.06, 0.36],
                [x + 0.06, yn + 0.11, 0.66],
                0.015,
            );
        }
        return;
    };
    let r = 0.025;
    let y0 = yn + 0.08;
    let half = width / 2.0;
    let mut ux = vec![0.28f32];
    if uprights >= 4 {
        ux.push(half - 0.05);
    }
    for sx in [-1.0f32, 1.0] {
        for &u in &ux {
            let x = sx * u;
            b.cylinder(
                k0,
                [x, y0, z_lo - 0.06],
                [x, y0, z_hi + 0.03],
                r,
                r,
                8,
                true,
            );
            b.cuboid(
                &pads,
                [x - 0.03, y0 + 0.018, z_lo + 0.04],
                [x + 0.03, y0 + 0.042, z_hi - 0.02],
            );
        }
        // Mount struts back into the bumper.
        let x = sx * 0.28;
        for z in [z_lo + 0.06, (z_lo + z_hi) / 2.0] {
            b.cylinder(k0, [x, y0, z], [x, yn - 0.12, z], 0.03, 0.03, 8, false);
        }
    }
    let rail_z: Vec<f32> = if rails >= 2 {
        vec![z_lo, z_hi]
    } else {
        vec![z_lo]
    };
    for &z in &rail_z {
        b.cylinder(k0, [-half, y0, z], [half, y0, z], r, r, 8, true);
        // Rail ends bent back toward the bumper corners.
        for sx in [-1.0f32, 1.0] {
            b.cylinder(
                k0,
                [sx * half, y0, z],
                [sx * (half - 0.06), yn - 0.03, z],
                r,
                r,
                8,
                true,
            );
        }
    }
    if leds {
        for (side, sx) in [("l", -1.0f32), ("r", 1.0)] {
            let colour = if sx < 0.0 {
                MaterialSlot::LightbarRed
            } else {
                MaterialSlot::LightbarBlue
            };
            let led = key(
                &format!("pushbar_led_{side}"),
                BodyRegion::Accessory,
                PartKind::Lightbar,
                colour,
            );
            let x = sx * (half - 0.16);
            b.cuboid(
                &led,
                [x - 0.05, y0 - 0.01, z_hi + r],
                [x + 0.05, y0 + 0.035, z_hi + r + 0.05],
            );
            b.cuboid(
                &pads,
                [x - 0.055, y0 - 0.04, z_hi + r - 0.005],
                [x + 0.055, y0 - 0.01, z_hi + r + 0.055],
            );
        }
    }
}

/// Grille insert with red (left) / blue (right) LED modules just below the nose top.
pub fn grille_leds(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let yn = p.y_nose;
    let zt = p.z_nose - 0.05;
    b.cuboid(k0, [-0.30, yn - 0.05, zt - 0.11], [0.30, yn + 0.004, zt]);
    for (side, sx) in [("l", -1.0f32), ("r", 1.0)] {
        let colour = if sx < 0.0 {
            MaterialSlot::LightbarRed
        } else {
            MaterialSlot::LightbarBlue
        };
        let led = key(
            &format!("grille_led_{side}"),
            BodyRegion::FrontBumper,
            PartKind::Lightbar,
            colour,
        );
        for (a, c) in [(0.08f32, 0.13f32), (0.17, 0.25)] {
            let (x0, x1) = xs(sx, a, c);
            b.cuboid(
                &led,
                [x0, yn - 0.02, zt - 0.075],
                [x1, yn + 0.012, zt - 0.04],
            );
        }
    }
}

/// Carbon splitter plate under the nose.
pub fn splitter(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let w = p.half_width(p.y_nose - 0.1) - 0.10;
    let z = p.sill(p.y_nose - 0.1);
    b.beveled_box(
        k0,
        [-w, p.y_nose - 0.25, z - 0.02],
        [w, p.y_nose + 0.06, z + 0.004],
        0.008,
    );
}

/// Tow hooks (loops) under the front bumper, x = ±0.55.
pub fn tow_hook(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let y = p.y_nose - 0.06;
    let z = p.sill(p.y_nose - 0.15) + 0.02;
    b.torus(k0, [-0.55, y, z], [1.0, 0.0, 0.0], 0.04, 0.012, 8, 4);
    b.mirror_part(&k0.name, &k0.mirrored());
}

// ---------------------------------------------------------------------------------------
// Greenhouse / roof
// ---------------------------------------------------------------------------------------

/// A-pillar spotlight (driver side; both sides when the kit says so): housing aimed forward,
/// headlight-material lens, shaft through the pillar.
pub fn spotlight(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let k = kit(&ctx.def.id);
    let p = ctx.profile;
    let y = p.y_fw + (p.roof.1 - p.y_fw) * 0.30;
    let z = p.z_belt + (p.roof.2 - p.z_belt) * 0.28;
    let (pt, n) = p.side_point(y, z);
    let r = k.spot.r;
    let c = [pt[0] + n[0] * (r + 0.035), y, z + n[2] * (r + 0.035)];
    b.cylinder(
        k0,
        [c[0], c[1] - 0.08, c[2]],
        [c[0], c[1] + 0.07, c[2]],
        r * 0.85,
        r,
        12,
        true,
    );
    let lens = key(
        "spotlight_lens_l",
        BodyRegion::Cabin,
        PartKind::Detail,
        MaterialSlot::Headlight,
    );
    b.cylinder(
        &lens,
        [c[0], c[1] + 0.07, c[2]],
        [c[0], c[1] + 0.078, c[2]],
        r * 0.86,
        r * 0.86,
        12,
        true,
    );
    b.cylinder(
        k0,
        c,
        [pt[0] - n[0] * 0.03, y, z - n[2] * 0.03],
        0.013,
        0.013,
        6,
        true,
    );
    if k.spot.both {
        b.mirror_part(&k0.name, &k0.mirrored());
        b.mirror_part(&lens.name, &lens.mirrored());
    }
}

/// Low antenna pucks on the roof's rear part.
pub fn pucks(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let k = kit(&ctx.def.id);
    let p = ctx.profile;
    for &(dy, x) in k.pucks {
        let y = p.roof.0 + dy;
        let z = roof_z(p, y, x);
        b.cylinder(
            k0,
            [x, y, z - 0.006],
            [x, y, z + 0.022],
            0.05,
            0.03,
            10,
            true,
        );
    }
}

/// Whip antennas (trunk lid; roof when ahead of the deck).
pub fn whips(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let k = kit(&ctx.def.id);
    let p = ctx.profile;
    let roof = key(
        "antenna_whip_roof",
        BodyRegion::Roof,
        PartKind::Detail,
        MaterialSlot::Trim,
    );
    for &(pct, x, len) in k.whips {
        let y = y_at(p, pct);
        let kk = if y < p.y_deck { k0 } else { &roof };
        let z = roof_z(p, y, x);
        b.cylinder(
            kk,
            [x, y, z - 0.005],
            [x, y, z + 0.025],
            0.018,
            0.012,
            8,
            true,
        );
        b.cylinder(
            kk,
            [x, y, z + 0.02],
            [x, y - 0.09, z + len],
            0.004,
            0.0025,
            5,
            true,
        );
    }
}

/// Visor LED bar inside the windshield top: black back plate, red left / blue right.
pub fn visor_bar(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let z1 = p.roof.2 - 0.05;
    let z0 = z1 - 0.045;
    let yf = windshield_y(p, z1) - 0.035;
    let yr = yf - 0.05;
    let xw = 0.40f32.min(p.roof_half - 0.25);
    b.cuboid(
        k0,
        [-xw - 0.01, yr - 0.012, z0 - 0.006],
        [xw + 0.01, yr, z1 + 0.006],
    );
    lens_pair(b, "visor_bar_lens", [yr, yf], [z0, z1], xw, None);
}

/// LED bar inside the rear-glass top: red left / amber centre / blue right, facing back.
pub fn rear_window_bar(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let z1 = p.roof.2 - 0.05;
    let z0 = z1 - 0.04;
    let yr = rear_glass_y(p, z1) + 0.035;
    let yf = yr + 0.045;
    let xw = 0.45f32.min(p.roof_half - 0.25);
    b.cuboid(
        k0,
        [-xw - 0.01, yf, z0 - 0.006],
        [xw + 0.01, yf + 0.012, z1 + 0.006],
    );
    lens_pair(
        b,
        "rear_window_bar_lens",
        [yr, yf],
        [z0, z1],
        xw,
        Some(0.12),
    );
}

/// Red (x < 0) / blue (x > 0) lens boxes `name_l` / `name_r`, optional amber centre ±`amber`.
fn lens_pair(b: &mut Builder, name: &str, y: [f32; 2], z: [f32; 2], xw: f32, amber: Option<f32>) {
    let inner = amber.unwrap_or(0.01);
    for (side, sx) in [("l", -1.0f32), ("r", 1.0)] {
        let colour = if sx < 0.0 {
            MaterialSlot::LightbarRed
        } else {
            MaterialSlot::LightbarBlue
        };
        let k = key(
            &format!("{name}_{side}"),
            BodyRegion::Cabin,
            PartKind::Lightbar,
            colour,
        );
        let (x0, x1) = xs(sx, inner, xw);
        b.cuboid(&k, [x0, y[0], z[0]], [x1, y[1], z[1]]);
    }
    if let Some(a) = amber {
        let k = key(
            &format!("{name}_amber"),
            BodyRegion::Cabin,
            PartKind::Lightbar,
            MaterialSlot::EmissiveAmber,
        );
        b.cuboid(&k, [-a + 0.005, y[0], z[0]], [a - 0.005, y[1], z[1]]);
    }
}

/// Amber traffic advisor (8 cells) on the rear parcel shelf, facing back.
pub fn traffic_advisor(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let z0 = p.z_deck + 0.035;
    let z1 = z0 + 0.05;
    let yr = rear_glass_y(p, z1) + 0.03;
    let yf = yr + 0.05;
    let xw = 0.45f32.min(p.roof_half - 0.15);
    let cells = key(
        "traffic_advisor_cells",
        BodyRegion::Cabin,
        PartKind::Lightbar,
        MaterialSlot::EmissiveAmber,
    );
    b.cuboid(&cells, [-xw, yr, z0], [xw, yf, z1]);
    b.cuboid(
        k0,
        [-xw - 0.01, yf, z0 - 0.008],
        [xw + 0.01, yf + 0.012, z1 + 0.008],
    );
    for i in 1..8 {
        let x = -xw + 2.0 * xw * i as f32 / 8.0;
        b.cuboid(
            k0,
            [x - 0.004, yr - 0.002, z0 - 0.002],
            [x + 0.004, yf, z1 + 0.002],
        );
    }
}

/// Partition behind the front seats: solid lower panel, polycarbonate upper pane, bars.
pub fn partition(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let y = match p.rear_door {
        Some((hinge, _)) => hinge - 0.08,
        None => p.y_fd_rear + 0.15,
    };
    let zb = p.z_belt;
    let zt = p.roof.2 - 0.10;
    let xw = 0.62f32.min(p.roof_half - 0.14);
    let panel = key(
        "partition_panel",
        BodyRegion::Cabin,
        PartKind::Interior,
        MaterialSlot::Interior,
    );
    b.cuboid(
        &panel,
        [-xw, y - 0.01, zb - 0.40],
        [xw, y + 0.005, zb + 0.02],
    );
    let glass = key(
        "partition_glass",
        BodyRegion::Cabin,
        PartKind::Detail,
        MaterialSlot::Glass,
    );
    b.cuboid(&glass, [-xw, y - 0.004, zb + 0.02], [xw, y + 0.002, zt]);
    for z in [zb + 0.02, zt] {
        b.cylinder(
            k0,
            [-xw, y + 0.01, z],
            [xw, y + 0.01, z],
            0.009,
            0.009,
            6,
            true,
        );
    }
    for x in [-xw, -0.5 * xw, 0.0, 0.5 * xw, xw] {
        b.cylinder(
            k0,
            [x, y + 0.01, zb - 0.40],
            [x, y + 0.01, zt],
            0.008,
            0.008,
            6,
            true,
        );
    }
}

// ---------------------------------------------------------------------------------------
// Rear
// ---------------------------------------------------------------------------------------

/// Small decklid lip spoiler (livery paint).
pub fn spoiler_lip(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let ya = p.y_tail + 0.06;
    let yb = ya + 0.12;
    let za = roof_z(p, ya, 0.0);
    let zb = roof_z(p, yb, 0.0);
    let w = p.half_width(yb) - 0.14;
    prism_x(
        b,
        k0,
        -w,
        w,
        &[
            [yb, zb - 0.006],
            [ya, za - 0.006],
            [ya, za + 0.045],
            [yb, zb + 0.004],
        ],
    );
}

/// Pedestal wing: carbon blade, end plates, two pedestals, red / blue LED strip on the
/// trailing edge.
pub fn spoiler_wing(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let yc = p.y_tail + 0.20;
    let zd = roof_z(p, yc, 0.0);
    let w = (p.half_width(yc) - 0.08).min(0.70);
    b.beveled_box(
        k0,
        [-w, yc - 0.10, zd + 0.12],
        [w, yc + 0.10, zd + 0.14],
        0.006,
    );
    for sx in [-1.0f32, 1.0] {
        let x = sx * 0.45;
        let zs = roof_z(p, yc, x);
        b.cuboid(
            k0,
            [x - 0.015, yc - 0.06, zs - 0.01],
            [x + 0.015, yc + 0.04, zd + 0.125],
        );
        let (x0, x1) = xs(sx, w, w + 0.012);
        b.cuboid(k0, [x0, yc - 0.11, zd + 0.09], [x1, yc + 0.10, zd + 0.165]);
    }
    for (side, sx) in [("l", -1.0f32), ("r", 1.0)] {
        let colour = if sx < 0.0 {
            MaterialSlot::LightbarRed
        } else {
            MaterialSlot::LightbarBlue
        };
        let led = key(
            &format!("spoiler_led_{side}"),
            BodyRegion::Trunk,
            PartKind::Lightbar,
            colour,
        );
        let (x0, x1) = xs(sx, 0.005, 0.40);
        b.cuboid(
            &led,
            [x0, yc - 0.106, zd + 0.122],
            [x1, yc - 0.096, zd + 0.138],
        );
    }
}

// ---------------------------------------------------------------------------------------
// Sides
// ---------------------------------------------------------------------------------------

/// Chrome rub strip at z ≈ 0.55 along both sides, one piece per panel (it dents / opens with
/// its panel), interrupted at the wheel arches.
pub fn rub_strip(ctx: &DetailCtx, _k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let (z0, z1) = (0.535f32, 0.565f32);
    let gap = 0.008;
    let mut pieces: Vec<(f32, f32, BodyRegion, &str)> = Vec::new();
    match p.rear_door {
        Some((hinge, rear)) => {
            pieces.push((hinge + gap, p.y_fd_hinge - gap, BodyRegion::LeftDoorF, "fl"));
            pieces.push((rear + gap, hinge - gap, BodyRegion::LeftDoorR, "rl"));
            pieces.push((
                p.y_quarter.1 + gap,
                rear - gap,
                BodyRegion::RearLeftQuarter,
                "q",
            ));
        }
        None => {
            pieces.push((
                p.y_fd_rear + gap,
                p.y_fd_hinge - gap,
                BodyRegion::LeftDoorF,
                "fl",
            ));
            pieces.push((
                p.y_quarter.1 + gap,
                p.y_fd_rear - gap,
                BodyRegion::RearLeftQuarter,
                "q",
            ));
        }
    }
    pieces.push((
        p.y_fd_hinge + gap,
        p.y_bf - gap,
        BodyRegion::FrontLeftFender,
        "f",
    ));
    for (ya, yb, region, tag) in pieces {
        // Cut out the wheel arches.
        let mut spans = vec![(ya, yb)];
        for &(ay, az, ar) in &p.arches {
            if az + ar + p.style.arch_gap < z0 {
                continue;
            }
            let (c0, c1) = (ay - ar - 0.06, ay + ar + 0.06);
            spans = spans
                .into_iter()
                .flat_map(|(s0, s1)| {
                    let mut out = Vec::new();
                    if c0 > s0 {
                        out.push((s0, c0.min(s1)));
                    }
                    if c1 < s1 {
                        out.push((c1.max(s0), s1));
                    }
                    out
                })
                .collect();
        }
        let k = key(
            &format!("rub_strip_{tag}_l"),
            region,
            PartKind::Detail,
            MaterialSlot::Chrome,
        );
        for (s0, s1) in spans {
            if s1 - s0 < 0.05 {
                continue;
            }
            b.surface_patch(
                &k,
                p,
                &Patch {
                    y0: s0,
                    y1: s1,
                    band: PatchBand::Side { z0, z1 },
                    offset_m: 0.0,
                    thickness_m: 0.012,
                    steps: (0, 1),
                },
            );
        }
        if b.has(&k.name) {
            b.mirror_part(&k.name, &k.mirrored());
        }
    }
}

// ---------------------------------------------------------------------------------------
// Unit-number decal carriers (re-mapped by the renderer onto a per-unit number texture)
// ---------------------------------------------------------------------------------------

/// Side unit number (left; the detail is mirrored).
pub fn decal_side(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let k = kit(&ctx.def.id);
    let p = ctx.profile;
    let y = y_at(p, k.unit.pct);
    b.surface_patch(
        k0,
        p,
        &Patch {
            y0: y - k.unit.half_len,
            y1: y + k.unit.half_len,
            band: PatchBand::Side {
                z0: k.unit.z.0,
                z1: k.unit.z.1,
            },
            offset_m: 0.002,
            thickness_m: 0.0,
            steps: (4, 2),
        },
    );
}

/// Roof (aerial) unit number, behind the light bar.
pub fn decal_roof(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let k = kit(&ctx.def.id);
    let p = ctx.profile;
    let y1 = match k.bar {
        Bar::Led { depth, .. } => bar_y(p, &k) - depth / 2.0 - 0.06,
        Bar::Rotator => bar_y(p, &k) - 0.21,
        Bar::Slick => p.roof.1 - 0.10,
    }
    .min(p.roof.1 - 0.06);
    let y0 = (p.roof.0 + 0.16).max(y1 - 0.56);
    let xw = 0.42f32.min(p.roof_half - 0.12);
    b.surface_patch(
        k0,
        p,
        &Patch {
            y0,
            y1,
            band: PatchBand::Top { x0: -xw, x1: xw },
            offset_m: 0.002,
            thickness_m: 0.0,
            steps: (4, 4),
        },
    );
}

/// Trunk-lid unit number.
pub fn decal_trunk(ctx: &DetailCtx, k0: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let y0 = p.y_tail + 0.30;
    let y1 = (y0 + 0.16).min(p.y_deck - 0.08);
    b.surface_patch(
        k0,
        p,
        &Patch {
            y0,
            y1,
            band: PatchBand::Top { x0: 0.12, x1: 0.50 },
            offset_m: 0.002,
            thickness_m: 0.0,
            steps: (2, 3),
        },
    );
}
