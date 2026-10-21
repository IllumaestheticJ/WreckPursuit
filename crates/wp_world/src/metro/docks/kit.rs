//! Port structures built from boxes, cylinders and prisms: ship-to-shore gantry cranes,
//! rubber-tyred yard cranes, straddle carriers, reach stackers, floodlight masts, container
//! ships, a bulk carrier, a drive-in ferry and small boats.
//!
//! Every builder takes a [`Fr`] (local frame) and returns the parts; the caller checks and
//! places them. Solid parts are only what a car can reach (legs, hulls, bodies); everything
//! high up (booms, girders, cabins, deck cargo) is non-solid.

use wp_math::Rng;

use super::{pick, strut, Fr};
use crate::metro::ctx::{cylinder, prism};
use crate::network::{Feature, FeatureKind as K, Material as M};

/// Container colours with weights.
pub const CONTAINER_COLOURS: [(M, u32); 6] = [
    (M::Red, 3),
    (M::Blue, 3),
    (M::Green, 2),
    (M::SafetyOrange, 1),
    (M::White, 2),
    (M::Rust, 2),
];

/// Container height (one tier), m.
pub const TIER_M: f32 = 2.59;
/// Container width, m.
pub const BOX_W: f32 = 2.44;
/// 40 ft container length, m.
pub const FEU_M: f32 = 12.2;
/// 20 ft container length, m.
pub const TEU_M: f32 = 6.1;

/// Ship-to-shore crane gauge (seaward to landward rail), m.
pub const STS_GAUGE: f32 = 30.0;
/// Ship-to-shore crane outreach beyond the seaward rail, m.
pub const STS_OUTREACH: f32 = 52.0;

/// Ship-to-shore gantry crane. `fr.o` = seaward rail point (ground), `fr.u` along the quay,
/// `fr.v` toward the water. Legs at ±9 m along the quay on both rails.
pub fn gantry_crane(fr: &Fr, z0: f32) -> Vec<Feature> {
    let mut out = Vec::new();
    let y = M::SafetyYellow;
    let g = STS_GAUGE;
    let portal = 44.0;
    for b in [0.0, -g] {
        for a in [-9.0, 9.0] {
            out.push(fr.bx(K::GantryCrane, y, a, b, z0, 1.6, 1.6, portal, true));
        }
        // Upper portal beam along the quay.
        out.push(fr.bx(
            K::GantryCrane,
            y,
            0.0,
            b,
            z0 + portal - 2.0,
            19.6,
            1.8,
            2.0,
            false,
        ));
    }
    for a in [-9.0, 9.0] {
        // Sill beam across the gauge.
        out.push(fr.bx(
            K::GantryCrane,
            y,
            a,
            -g * 0.5,
            z0 + 12.0,
            1.2,
            g + 1.6,
            1.6,
            false,
        ));
        // A-frame post.
        out.push(fr.bx(
            K::GantryCrane,
            y,
            a * 0.75,
            -g,
            z0 + portal,
            1.2,
            1.2,
            20.0,
            false,
        ));
    }
    // Boom (back-reach to outreach).
    let back = -g - 22.0;
    out.push(fr.bx(
        K::GantryCrane,
        y,
        0.0,
        (back + STS_OUTREACH) * 0.5,
        z0 + portal,
        3.5,
        STS_OUTREACH - back,
        3.0,
        false,
    ));
    out.push(fr.bx(
        K::GantryCrane,
        y,
        0.0,
        -g,
        z0 + portal + 19.0,
        15.0,
        1.2,
        1.2,
        false,
    ));
    // Forestay from the apex to the boom tip.
    out.push(strut(
        K::GantryCrane,
        y,
        fr.p3(0.0, -g, z0 + portal + 19.6),
        fr.p3(0.0, STS_OUTREACH - 2.0, z0 + portal + 3.0),
        0.6,
        false,
    ));
    // Machinery house, trolley, spreader.
    out.push(fr.bx(
        K::GantryCrane,
        M::White,
        0.0,
        back + 7.0,
        z0 + portal + 3.0,
        10.0,
        12.0,
        6.0,
        false,
    ));
    out.push(fr.bx(
        K::GantryCrane,
        M::Steel,
        0.0,
        18.0,
        z0 + portal - 3.4,
        6.0,
        5.0,
        3.4,
        false,
    ));
    out.push(fr.bx(
        K::GantryCrane,
        M::Steel,
        0.0,
        18.0,
        z0 + 24.0,
        FEU_M,
        2.6,
        0.8,
        false,
    ));
    // Red aircraft-warning lights.
    out.push(fr.bx(
        K::GantryCrane,
        M::Light,
        0.0,
        -g,
        z0 + portal + 20.2,
        0.6,
        0.6,
        0.6,
        false,
    ));
    out.push(fr.bx(
        K::GantryCrane,
        M::Light,
        0.0,
        STS_OUTREACH - 0.5,
        z0 + portal + 3.0,
        0.6,
        0.6,
        0.6,
        false,
    ));
    out
}

/// Rubber-tyred yard gantry crane straddling a block: `fr.o` = centre, `fr.u` along the
/// block, `span` = leg-to-leg distance across the block.
pub fn rtg(fr: &Fr, z0: f32, span: f32) -> Vec<Feature> {
    let mut out = Vec::new();
    let y = M::SafetyYellow;
    let h = 19.0;
    for a in [-6.0, 6.0] {
        for b in [-span * 0.5, span * 0.5] {
            out.push(fr.bx(K::PortMachine, y, a, b, z0, 1.6, 0.9, h, true));
        }
        out.push(fr.bx(
            K::PortMachine,
            y,
            a,
            0.0,
            z0 + h,
            1.2,
            span + 0.9,
            1.6,
            false,
        ));
    }
    for b in [-span * 0.5, span * 0.5] {
        out.push(fr.bx(
            K::PortMachine,
            y,
            0.0,
            b,
            z0 + h - 1.0,
            13.6,
            0.9,
            1.2,
            false,
        ));
    }
    out.push(fr.bx(
        K::PortMachine,
        M::White,
        0.0,
        span * 0.15,
        z0 + h - 3.0,
        4.0,
        3.0,
        3.0,
        false,
    ));
    out.push(fr.bx(
        K::PortMachine,
        M::Steel,
        0.0,
        span * 0.5 - 2.0,
        z0 + h + 1.6,
        5.0,
        3.0,
        2.0,
        false,
    ));
    out
}

/// Straddle carrier parked at `fr.o` facing `fr.u` (footprint 5.6 × 9.6 m, 15 m high).
pub fn straddle_carrier(fr: &Fr, z0: f32) -> Vec<Feature> {
    let mut out = Vec::new();
    let c = M::SafetyOrange;
    for a in [-4.2, 4.2] {
        for b in [-2.5, 2.5] {
            out.push(fr.bx(K::PortMachine, c, a, b, z0, 0.9, 0.7, 13.0, true));
        }
    }
    for b in [-2.5, 2.5] {
        out.push(fr.bx(K::PortMachine, c, 0.0, b, z0 + 12.6, 9.6, 0.8, 1.6, false));
    }
    out.push(fr.bx(
        K::PortMachine,
        M::White,
        4.5,
        0.0,
        z0 + 12.0,
        1.8,
        3.0,
        2.4,
        false,
    ));
    out.push(fr.bx(
        K::PortMachine,
        M::Steel,
        0.0,
        0.0,
        z0 + 9.0,
        FEU_M - 2.0,
        2.4,
        0.6,
        false,
    ));
    out
}

/// Reach stacker parked at `fr.o` facing `fr.u` (4.2 × 11 m body).
pub fn reach_stacker(fr: &Fr, z0: f32) -> Vec<Feature> {
    vec![
        fr.bx(
            K::PortMachine,
            M::SafetyOrange,
            0.0,
            0.0,
            z0,
            8.0,
            4.2,
            2.6,
            true,
        ),
        fr.bx(
            K::PortMachine,
            M::Glass,
            -1.0,
            0.0,
            z0 + 2.6,
            2.4,
            2.2,
            2.2,
            false,
        ),
        strut(
            K::PortMachine,
            M::SafetyOrange,
            fr.p3(-3.0, 0.0, z0 + 3.0),
            fr.p3(5.5, 0.0, z0 + 11.0),
            1.0,
            false,
        ),
        fr.bx(
            K::PortMachine,
            M::Steel,
            6.0,
            0.0,
            z0 + 10.0,
            2.0,
            FEU_M - 1.0,
            0.6,
            false,
        ),
    ]
}

/// Floodlight mast at `base` (ground at `z0`): a tall solid column and a light head.
pub fn floodlight(fr: &Fr, z0: f32) -> Vec<Feature> {
    let p = fr.at(0.0, 0.0);
    vec![
        cylinder(K::Lamp, M::Steel, [p[0], p[1], z0], 0.45, 32.0, true),
        fr.bx(K::Lamp, M::Light, 0.0, 0.0, z0 + 32.0, 1.4, 4.0, 1.0, false),
    ]
}

/// Hull colours with weights.
const HULL_COLOURS: [(M, u32); 4] = [(M::Red, 2), (M::Blue, 2), (M::Black, 3), (M::Green, 1)];

/// Hull draught below the water / deck height of the big ships, m.
const HULL_BOTTOM_Z: f32 = -6.0;

/// Footprint of a ship hull: `fr.o` = hull centre, `fr.u` = toward the bow.
pub fn hull_footprint(fr: &Fr, len: f32, beam: f32) -> Vec<crate::geom::V2> {
    let (h, b) = (len * 0.5, beam * 0.5);
    vec![
        fr.at(-h, -b),
        fr.at(h - 28.0, -b),
        fr.at(h - 10.0, -b * 0.7),
        fr.at(h, -1.5),
        fr.at(h, 1.5),
        fr.at(h - 10.0, b * 0.7),
        fr.at(h - 28.0, b),
        fr.at(-h, b),
    ]
}

fn hull(fr: &Fr, len: f32, beam: f32, deck_z: f32, mat: M) -> Vec<Feature> {
    let h = len * 0.5;
    let main = fr.bx(
        K::Ship,
        mat,
        -14.0,
        0.0,
        HULL_BOTTOM_Z,
        len - 28.0,
        beam,
        deck_z - HULL_BOTTOM_Z,
        true,
    );
    let b = beam * 0.5;
    let bow = prism(
        K::Ship,
        mat,
        vec![
            fr.at(h - 28.5, -b),
            fr.at(h - 10.0, -b * 0.7),
            fr.at(h, -1.5),
            fr.at(h, 1.5),
            fr.at(h - 10.0, b * 0.7),
            fr.at(h - 28.5, b),
        ],
        HULL_BOTTOM_Z,
        deck_z,
        true,
    );
    vec![main, bow]
}

fn accommodation(fr: &Fr, len: f32, beam: f32, deck_z: f32, a: f32, height: f32) -> Vec<Feature> {
    let _ = len;
    vec![
        fr.bx(
            K::Ship,
            M::White,
            a,
            0.0,
            deck_z,
            14.0,
            beam - 6.0,
            height,
            true,
        ),
        fr.bx(
            K::Ship,
            M::White,
            a + 3.0,
            0.0,
            deck_z + height - 2.0,
            5.0,
            beam + 2.0,
            2.0,
            false,
        ),
        fr.bx(
            K::Ship,
            M::Black,
            a - 12.0,
            0.0,
            deck_z,
            7.0,
            6.0,
            height + 6.0,
            false,
        ),
        fr.bx(
            K::Ship,
            M::Light,
            a,
            0.0,
            deck_z + height,
            0.6,
            0.6,
            0.6,
            false,
        ),
    ]
}

/// Container ship: `fr.o` = hull centre on the water, `fr.u` toward the bow.
pub fn container_ship(rng: &mut Rng, fr: &Fr, len: f32, beam: f32) -> Vec<Feature> {
    let deck_z = 12.0;
    let mat = pick(rng, &HULL_COLOURS);
    let mut out = hull(fr, len, beam, deck_z, mat);
    let h = len * 0.5;
    let stern_house = -h + 22.0;
    out.extend(accommodation(fr, len, beam, deck_z, stern_house, 24.0));
    // Deck cargo: bays of 40 ft boxes from behind the house to the bow, rows across the
    // beam split into 2–4 groups of one colour and height each.
    let rows = ((beam - 2.0) / (BOX_W + 0.1)).floor() as usize;
    let mut a = stern_house + 9.0 + FEU_M * 0.5;
    while a + FEU_M * 0.5 < h - 30.0 {
        let groups = 2 + rng.below(3) as usize;
        let mut r = 0;
        for gi in 0..groups {
            let left = rows - r;
            let take = if gi + 1 == groups {
                left
            } else {
                (1 + rng.below((left - (groups - gi - 1)).max(1) as u32) as usize).min(left)
            };
            if take == 0 {
                continue;
            }
            let tiers = 1 + rng.below(6);
            let w = take as f32 * (BOX_W + 0.1);
            let b = -(rows as f32) * (BOX_W + 0.1) * 0.5 + r as f32 * (BOX_W + 0.1) + w * 0.5;
            out.push(fr.bx(
                K::Container,
                pick(rng, &CONTAINER_COLOURS),
                a,
                b,
                deck_z,
                FEU_M,
                w - 0.1,
                tiers as f32 * TIER_M,
                false,
            ));
            r += take;
        }
        a += FEU_M + 0.6;
    }
    out
}

/// Bulk carrier: hatch covers and deck cranes instead of containers.
pub fn bulk_carrier(rng: &mut Rng, fr: &Fr, len: f32, beam: f32) -> Vec<Feature> {
    let deck_z = 10.0;
    let mat = pick(rng, &HULL_COLOURS);
    let mut out = hull(fr, len, beam, deck_z, mat);
    let h = len * 0.5;
    let stern_house = -h + 20.0;
    out.extend(accommodation(fr, len, beam, deck_z, stern_house, 20.0));
    let hatch_mat = if rng.chance(0.5) { M::Rust } else { M::Green };
    let first = stern_house + 16.0;
    let usable = h - 30.0 - first;
    let n = 7usize;
    let pitch = usable / n as f32;
    for k in 0..n {
        let a = first + pitch * (k as f32 + 0.5);
        out.push(fr.bx(
            K::Ship,
            hatch_mat,
            a,
            0.0,
            deck_z,
            pitch - 4.0,
            beam - 8.0,
            1.6,
            false,
        ));
        if k % 2 == 1 {
            let c = fr.at(a - pitch * 0.5, 0.0);
            out.push(cylinder(
                K::Ship,
                M::White,
                [c[0], c[1], deck_z],
                1.2,
                14.0,
                false,
            ));
            out.push(strut(
                K::Ship,
                M::White,
                fr.p3(a - pitch * 0.5, 0.0, deck_z + 12.0),
                fr.p3(a - pitch * 0.5 + 18.0, beam * 0.3, deck_z + 18.0),
                0.8,
                false,
            ));
        }
    }
    out
}

/// Car deck height of the ferry, m.
pub const FERRY_DECK_Z: f32 = 3.2;

/// Drive-in ferry: `fr.o` = bow door (on the water), `fr.u` toward the stern, `len` long.
/// The car deck (hull top) is solid and drivable, enclosed by solid casings and a closed
/// stern; the passenger decks above are non-solid.
pub fn ferry(fr: &Fr, len: f32, beam: f32) -> Vec<Feature> {
    let b = beam * 0.5;
    let bottom = -3.5;
    let casing_h = 6.0;
    vec![
        fr.bx(
            K::Ship,
            M::Blue,
            len * 0.5,
            0.0,
            bottom,
            len,
            beam,
            FERRY_DECK_Z - bottom,
            true,
        ),
        fr.bx(
            K::Ship,
            M::White,
            len * 0.5,
            -b + 1.2,
            FERRY_DECK_Z,
            len,
            2.4,
            casing_h,
            true,
        ),
        fr.bx(
            K::Ship,
            M::White,
            len * 0.5,
            b - 1.2,
            FERRY_DECK_Z,
            len,
            2.4,
            casing_h,
            true,
        ),
        fr.bx(
            K::Ship,
            M::White,
            len - 1.0,
            0.0,
            FERRY_DECK_Z,
            2.0,
            beam,
            casing_h,
            true,
        ),
        fr.bx(
            K::Ship,
            M::White,
            len * 0.55,
            0.0,
            FERRY_DECK_Z + casing_h,
            len * 0.7,
            beam,
            5.5,
            false,
        ),
        fr.bx(
            K::Ship,
            M::Glass,
            len * 0.3,
            0.0,
            FERRY_DECK_Z + casing_h + 5.5,
            8.0,
            beam + 2.0,
            2.6,
            false,
        ),
        fr.bx(
            K::Ship,
            M::Blue,
            len * 0.7,
            0.0,
            FERRY_DECK_Z + casing_h + 5.5,
            6.0,
            4.0,
            7.0,
            false,
        ),
    ]
}

/// Small boat at `fr.o` (bow toward `fr.u`): yacht (sail mast), motor yacht or fishing boat.
pub fn boat(rng: &mut Rng, fr: &Fr, len: f32) -> Vec<Feature> {
    let beam = (len * 0.33).clamp(2.6, 5.0);
    let kind = rng.below(3);
    let hull_mat = match kind {
        0 | 1 => M::White,
        _ => pick(rng, &[(M::Red, 1), (M::Blue, 1), (M::Green, 1)]),
    };
    let mut out = vec![fr.bx(K::Boat, hull_mat, 0.0, 0.0, -0.6, len, beam, 1.6, true)];
    match kind {
        0 => {
            let p = fr.at(len * 0.1, 0.0);
            out.push(fr.bx(
                K::Boat,
                M::White,
                -len * 0.1,
                0.0,
                1.0,
                len * 0.35,
                beam * 0.7,
                0.9,
                false,
            ));
            out.push(cylinder(
                K::Boat,
                M::Steel,
                [p[0], p[1], 1.0],
                0.1,
                len * 1.25,
                false,
            ));
        }
        1 => {
            out.push(fr.bx(
                K::Boat,
                M::Glass,
                -len * 0.05,
                0.0,
                1.0,
                len * 0.5,
                beam * 0.8,
                1.6,
                false,
            ));
        }
        _ => {
            let p = fr.at(len * 0.2, 0.0);
            out.push(fr.bx(
                K::Boat,
                M::White,
                -len * 0.25,
                0.0,
                1.0,
                len * 0.25,
                beam * 0.7,
                2.2,
                false,
            ));
            out.push(cylinder(
                K::Boat,
                M::Steel,
                [p[0], p[1], 1.0],
                0.12,
                6.0,
                false,
            ));
        }
    }
    out
}
