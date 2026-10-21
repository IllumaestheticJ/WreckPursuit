//! Engines (`research_mechanical.md` §1): nine types built from blocks, rolled banks, heads,
//! cam / valve covers, intakes, air cleaners, turbos, headers, placed per layout (FF
//! transverse, front longitudinal, mid), plus the radiator and a front-mount intercooler.
//!
//! An engine is built in its own frame — origin at the crank centre, middle of the block;
//! `x` across the crank (+x = right for a longitudinal engine, toward the firewall for a
//! transverse one), `y` along the crank (+y = front / timing end), `z` up — then placed: the
//! frame is rotated for the mount, and lowered and squashed in `z` until every vertex clears
//! the hood (or deck / rear glass) by [`HOOD_CLEARANCE`], keeping the sump above the ground.
//! So the engine shows once the hood is open or gone, or through scoops / engine-cover glass,
//! and never pokes through the body.
//!
//! [`spec::CarSpec::engine_detail`] (hero cars) adds runners, coil packs, tubular headers,
//! turbo housings, fans and piping; otherwise a cheap block of ~200–350 tris that still reads
//! right with the hood gone.
//!
//! Parts (rigid, `Engine` kind, `FrameFront` or `FrameRear`): `engine_block` (dark metal),
//! `engine_cover` (cam covers: tint / chrome / black / silver), `engine_metal` (intake,
//! compressor, aluminium), `engine_trim` (plastic covers, airboxes, fan), `engine_chrome`,
//! `engine_carbon`, `engine_headers` (exhaust metal), `engine_radiator`, `engine_intercooler`.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot, PartKind};
use crate::profile::BodyProfile;

use super::prim::{tube, Frame};
use super::spec::{CarSpec, Cover, Engine, Mount};

/// Gap kept between the engine and the hood / deck surface above it, m.
pub const HOOD_CLEARANCE: f32 = 0.03;
/// Lowest point of the sump above the ground, m.
const SUMP_CLEARANCE: f32 = 0.10;

/// Where the engine put things the other systems connect to (chassis space).
#[derive(Debug, Clone, Default)]
pub struct EngineOut {
    /// Exhaust exits (one per bank / manifold / turbo).
    pub exits: Vec<[f32; 3]>,
    /// Gearbox / transaxle output (driveshaft start), if any.
    pub gearbox_end: Option<[f32; 3]>,
    /// Engine sits behind the cabin.
    pub mid: bool,
    /// Crank height after placement.
    pub crank_z: f32,
}

/// Material slots used by the engine, in local emission.
#[derive(Clone, Copy)]
enum Mat {
    Block,
    Cover,
    Metal,
    Trim,
    Chrome,
    Carbon,
    Headers,
}

const MATS: [(Mat, &str, MaterialSlot); 7] = [
    (Mat::Block, "engine_block", MaterialSlot::Engine),
    (Mat::Cover, "engine_cover", MaterialSlot::Caliper),
    (Mat::Metal, "engine_metal", MaterialSlot::Metal),
    (Mat::Trim, "engine_trim", MaterialSlot::Trim),
    (Mat::Chrome, "engine_chrome", MaterialSlot::Chrome),
    (Mat::Carbon, "engine_carbon", MaterialSlot::Carbon),
    (Mat::Headers, "engine_headers", MaterialSlot::Exhaust),
];

/// Local emission context.
struct E {
    b: Builder,
    f: Frame,
    detail: bool,
    /// Exhaust side (local x sign).
    ex: f32,
    exits: Vec<[f32; 3]>,
    gearbox_end: Option<[f32; 3]>,
    /// Compressor outlet / throttle body (local) for intercooler piping.
    charge: Option<([f32; 3], [f32; 3])>,
}

fn key(m: Mat) -> PartKey {
    let (_, name, slot) = MATS
        .iter()
        .find(|q| q.0 as u8 == m as u8)
        .copied()
        .expect("mat");
    // Region / rigid are assigned when the parts are copied into the car (place()).
    PartKey::body(name, BodyRegion::FrameFront, PartKind::Engine, slot)
}

impl E {
    fn bx(&mut self, m: Mat, c: [f32; 3], h: [f32; 3]) {
        self.f.bx(&mut self.b, &key(m), c, h, 0.0);
    }
    /// Box whose centre and axes are rolled about the crank axis by `roll` degrees.
    fn bxr(&mut self, m: Mat, c: [f32; 3], h: [f32; 3], roll: f32) {
        let (sn, cs) = super::prim::sc_deg(roll);
        let c2 = [c[0] * cs + c[2] * sn, c[1], -c[0] * sn + c[2] * cs];
        self.f.bx(&mut self.b, &key(m), c2, h, roll);
    }
    fn cyl(&mut self, m: Mat, a: [f32; 3], e: [f32; 3], r: f32, sides: usize) {
        self.f.cyl(&mut self.b, &key(m), a, e, r, r, sides, true);
    }
    fn tube(&mut self, m: Mat, path: &[[f32; 3]], r: f32, sides: usize) {
        self.f.tube(&mut self.b, &key(m), path, r, sides);
    }
    /// Round flat can (air cleaner) about local z: body + lid.
    fn can(&mut self, body: Mat, lid: Mat, c: [f32; 3], r: f32, h: f32, n: usize) {
        let k = key(body);
        self.f.lathe(
            &mut self.b,
            &k,
            c,
            [0.0, 0.0, 1.0],
            &[[0.6 * r, 0.0], [r, 0.0], [r, h]],
            n,
        );
        let kl = key(lid);
        self.f.lathe(
            &mut self.b,
            &kl,
            c,
            [0.0, 0.0, 1.0],
            &[
                [r + 0.008, h - 0.004],
                [r + 0.008, h + 0.006],
                [0.75 * r, h + 0.018],
                [0.0, h + 0.022],
            ],
            n,
        );
        if self.detail {
            // Wing nut.
            self.cyl(
                lid,
                [c[0], c[1], c[2] + h + 0.02],
                [c[0], c[1], c[2] + h + 0.045],
                0.012,
                6,
            );
        }
    }
}

/// V engine banks: crankcase, sump, two rolled banks with heads and covers. `half_v` = half
/// the V angle (45 for a 90° V), `l` block length, `cover_w` cam-cover width.
fn v_banks(e: &mut E, l: f32, half_v: f32, bank_w: f32, cover_w: f32, cover_len: f32) {
    e.bx(Mat::Block, [0.0, 0.0, 0.0], [0.17, 0.5 * l, 0.12]);
    e.bx(
        Mat::Block,
        [0.0, -0.02, -0.17],
        [0.15, 0.5 * l - 0.03, 0.05],
    );
    for side in [-1.0f32, 1.0] {
        let roll = side * half_v;
        e.bxr(
            Mat::Block,
            [0.0, 0.0, 0.21],
            [0.5 * bank_w, 0.5 * l, 0.13],
            roll,
        );
        e.bxr(
            Mat::Block,
            [0.0, 0.0, 0.37],
            [0.5 * bank_w + 0.015, 0.5 * l - 0.01, 0.04],
            roll,
        );
        e.bxr(
            Mat::Cover,
            [0.0, 0.0, 0.44],
            [0.5 * cover_w, 0.5 * cover_len, 0.035],
            roll,
        );
    }
    // Timing cover + water pump nose.
    e.bx(Mat::Block, [0.0, 0.5 * l + 0.02, 0.12], [0.14, 0.02, 0.2]);
    e.cyl(
        Mat::Block,
        [0.0, 0.5 * l + 0.04, 0.12],
        [0.0, 0.5 * l + 0.10, 0.12],
        0.05,
        8,
    );
}

/// Point on top of a rolled bank cover (local), `along` the crank.
fn cover_top(half_v: f32, side: f32, along: f32) -> [f32; 3] {
    let (sn, cs) = super::prim::sc_deg(side * half_v);
    let z = 0.475;
    [z * sn, along, z * cs]
}

/// Tubular headers: `n` primaries per bank on side `side` (local x sign), merging into a
/// collector under the block; returns the collector end.
fn headers(e: &mut E, side: f32, n: usize, l: f32, x_port: f32, z_port: f32) -> [f32; 3] {
    let end = [side * (x_port - 0.04), -0.5 * l - 0.06, -0.20];
    if e.detail {
        for k in 0..n {
            let y = -0.5 * l + (k as f32 + 0.5) * l / n as f32;
            let path = [
                [side * x_port, y, z_port],
                [side * (x_port + 0.07), y, z_port - 0.08],
                [side * (x_port + 0.04), y - 0.06, -0.06],
                [end[0], end[1] + 0.12, end[2] + 0.04],
            ];
            e.tube(Mat::Headers, &path, 0.019, 6);
        }
        e.tube(
            Mat::Headers,
            &[[end[0], end[1] + 0.14, end[2] + 0.04], end],
            0.032,
            8,
        );
    } else {
        // Cast log manifold turning down at the rear.
        e.cyl(
            Mat::Headers,
            [side * (x_port + 0.03), 0.5 * l - 0.06, z_port - 0.04],
            [side * (x_port + 0.03), -0.5 * l + 0.04, z_port - 0.04],
            0.03,
            6,
        );
        e.tube(
            Mat::Headers,
            &[
                [side * (x_port + 0.03), -0.5 * l + 0.04, z_port - 0.04],
                [side * (x_port + 0.01), -0.5 * l - 0.03, z_port - 0.14],
                end,
            ],
            0.028,
            6,
        );
    }
    end
}

/// Longitudinal gearbox / bellhousing tapering back from the block's rear face.
fn gearbox(e: &mut E, l: f32, len: f32, w0: f32, w1: f32) {
    let y0 = -0.5 * l;
    e.f.taper(
        &mut e.b,
        &key(Mat::Block),
        0.0,
        -0.03,
        y0 - len,
        y0,
        [w1, 0.11],
        [w0, 0.16],
    );
    e.gearbox_end = Some([0.0, y0 - len, -0.04]);
}

/// Big-block pushrod V8 (A): chrome / painted valve covers, carb + round air cleaner.
fn big_block(e: &mut E) {
    let l = 0.66;
    v_banks(e, l, 45.0, 0.22, 0.13, 0.58);
    e.bx(Mat::Block, [0.0, 0.0, 0.31], [0.15, 0.24, 0.06]);
    e.bx(Mat::Metal, [0.0, 0.02, 0.405], [0.075, 0.075, 0.04]);
    e.can(
        Mat::Block,
        Mat::Chrome,
        [0.0, 0.02, 0.44],
        0.18,
        0.07,
        if e.detail { 16 } else { 10 },
    );
    if e.detail {
        plug_leads(e, l, 45.0, 4);
        fan(e, l);
    }
    for side in [-1.0f32, 1.0] {
        let x = headers(e, side, 4, l, 0.33, 0.13);
        e.exits.push(x);
    }
    gearbox(e, l, 0.7, 0.15, 0.09);
}

/// Plug leads: a row of thin rods per bank down the outside of each head.
fn plug_leads(e: &mut E, l: f32, half_v: f32, per_bank: usize) {
    for side in [-1.0f32, 1.0] {
        for k in 0..per_bank {
            let y = -0.5 * l + (k as f32 + 0.5) * l / per_bank as f32;
            let top = cover_top(half_v, side, y);
            let low = [top[0] + side * 0.06, y, top[2] - 0.12];
            e.cyl(Mat::Trim, top, low, 0.007, 4);
        }
    }
}

/// Engine fan behind the radiator.
fn fan(e: &mut E, l: f32) {
    e.cyl(
        Mat::Trim,
        [0.0, 0.5 * l + 0.12, 0.12],
        [0.0, 0.5 * l + 0.145, 0.12],
        0.19,
        10,
    );
}

/// Small-block pushrod V8 (B): modern plenum + airbox + coil packs, or old TBI cleaner.
fn small_block(e: &mut E, old: bool) {
    let l = 0.62;
    v_banks(e, l, 45.0, 0.2, 0.12, 0.54);
    if old {
        e.bx(Mat::Block, [0.0, 0.0, 0.31], [0.14, 0.22, 0.06]);
        e.bx(Mat::Metal, [0.0, 0.02, 0.39], [0.06, 0.06, 0.03]);
        e.can(
            Mat::Trim,
            Mat::Metal,
            [0.0, 0.02, 0.42],
            0.165,
            0.07,
            if e.detail { 14 } else { 10 },
        );
        if e.detail {
            plug_leads(e, l, 45.0, 4);
        }
    } else {
        e.bx(Mat::Metal, [0.0, 0.0, 0.33], [0.27, 0.2, 0.06]);
        e.cyl(Mat::Metal, [0.0, 0.2, 0.33], [0.0, 0.28, 0.33], 0.045, 8);
        e.tube(
            Mat::Trim,
            &[
                [0.0, 0.28, 0.33],
                [-0.18, 0.5 * l + 0.12, 0.3],
                [-0.3, 0.5 * l + 0.24, 0.24],
            ],
            0.05,
            6,
        );
        e.bx(Mat::Trim, [-0.33, 0.5 * l + 0.3, 0.2], [0.16, 0.12, 0.09]);
        if e.detail {
            coil_packs(e, l, 45.0, 4);
            e.bx(Mat::Trim, [0.0, 0.0, 0.405], [0.24, 0.2, 0.02]);
        }
    }
    for side in [-1.0f32, 1.0] {
        let x = headers(e, side, 4, l, 0.31, 0.12);
        e.exits.push(x);
    }
    gearbox(e, l, 0.75, 0.15, 0.09);
}

/// Coil packs: small boxes in a row on top of each bank cover.
fn coil_packs(e: &mut E, l: f32, half_v: f32, per_bank: usize) {
    for side in [-1.0f32, 1.0] {
        for k in 0..per_bank {
            let y = -0.45 * l + (k as f32 + 0.5) * 0.9 * l / per_bank as f32;
            e.bxr(
                Mat::Trim,
                [0.0, y, 0.49],
                [0.025, 0.025, 0.03],
                side * half_v,
            );
        }
    }
}

/// Modern DOHC V8 (C): wide cam covers with coil packs, intake plenum, snorkel to an airbox.
fn dohc_v8(e: &mut E) {
    let l = 0.70;
    v_banks(e, l, 45.0, 0.24, 0.2, 0.6);
    e.bx(Mat::Metal, [0.0, 0.0, 0.36], [0.24, 0.16, 0.065]);
    e.cyl(Mat::Metal, [0.0, 0.16, 0.36], [0.0, 0.24, 0.36], 0.043, 8);
    e.tube(
        Mat::Trim,
        &[
            [0.0, 0.24, 0.36],
            [-0.16, 0.5 * l + 0.15, 0.33],
            [-0.32, 0.5 * l + 0.3, 0.26],
        ],
        0.05,
        6,
    );
    e.bx(Mat::Trim, [-0.36, 0.5 * l + 0.36, 0.22], [0.16, 0.11, 0.09]);
    if e.detail {
        coil_packs(e, l, 45.0, 4);
        // Runner bumps along the plenum sides.
        for side in [-1.0f32, 1.0] {
            for k in 0..4 {
                let y = -0.12 + 0.08 * k as f32;
                e.bx(Mat::Metal, [side * 0.25, y, 0.33], [0.02, 0.025, 0.04]);
            }
        }
    }
    for side in [-1.0f32, 1.0] {
        let x = headers(e, side, 4, l, 0.35, 0.16);
        e.exits.push(x);
    }
    gearbox(e, l, 0.75, 0.16, 0.1);
}

/// Inline 4 / 6 (D, E, F): block, head, cam cover tilted `tilt`° (top toward +x), intake on
/// −ex, exhaust / turbos on +ex.
#[allow(clippy::too_many_arguments)]
fn inline(
    e: &mut E,
    cyl: usize,
    tilt: f32,
    turbos: u8,
    itb: bool,
    plastic_cover: bool,
    longitudinal: bool,
) {
    let l = if cyl == 6 { 0.78 } else { 0.5 };
    let ex = e.ex;
    e.bxr(Mat::Block, [0.0, 0.0, 0.1], [0.15, 0.5 * l, 0.2], tilt);
    e.bx(
        Mat::Block,
        [0.0, -0.02, -0.16],
        [0.13, 0.5 * l - 0.03, 0.06],
    );
    e.bxr(
        Mat::Block,
        [0.0, 0.0, 0.355],
        [0.16, 0.5 * l - 0.01, 0.055],
        tilt,
    );
    e.bxr(
        Mat::Cover,
        [0.0, 0.0, 0.44],
        [0.11, 0.5 * l - 0.04, 0.03],
        tilt,
    );
    e.bxr(
        Mat::Block,
        [0.0, 0.5 * l + 0.02, 0.12],
        [0.12, 0.02, 0.22],
        tilt,
    );
    let (ts, tc) = super::prim::sc_deg(tilt);
    let rot = |p: [f32; 3]| [p[0] * tc + p[2] * ts, p[1], -p[0] * ts + p[2] * tc];
    if e.detail {
        // Coil bumps along the cam cover spine.
        for k in 0..cyl {
            let y = -0.5 * l + (k as f32 + 0.5) * l / cyl as f32;
            e.bxr(Mat::Trim, [0.0, y, 0.475], [0.022, 0.022, 0.012], tilt);
        }
    }
    // Intake side.
    let throttle;
    if itb {
        for k in 0..cyl {
            let y = -0.5 * l + (k as f32 + 0.5) * l / cyl as f32;
            let a = rot([-ex * 0.17, y, 0.33]);
            let b2 = rot([-ex * 0.27, y, 0.33]);
            e.cyl(Mat::Metal, a, b2, 0.026, 6);
        }
        e.bxr(
            Mat::Trim,
            [-ex * 0.33, 0.0, 0.33],
            [0.06, 0.5 * l - 0.06, 0.07],
            tilt,
        );
        throttle = rot([-ex * 0.33, 0.5 * l, 0.33]);
    } else {
        e.bxr(
            Mat::Metal,
            [-ex * 0.26, 0.0, 0.27],
            [0.06, 0.5 * l - 0.05, 0.065],
            tilt,
        );
        throttle = rot([-ex * 0.26, 0.5 * l - 0.02, 0.27]);
        if e.detail {
            for k in 0..cyl {
                let y = -0.5 * l + (k as f32 + 0.5) * l / cyl as f32;
                let path = [
                    rot([-ex * 0.16, y, 0.36]),
                    rot([-ex * 0.24, y, 0.39]),
                    rot([-ex * 0.26, y, 0.31]),
                ];
                e.tube(Mat::Metal, &path, 0.02, 6);
            }
        }
        e.bx(
            Mat::Trim,
            [-ex * 0.24, 0.5 * l + 0.24, 0.24],
            [0.14, 0.1, 0.075],
        );
    }
    if plastic_cover {
        e.bxr(
            Mat::Trim,
            [0.0, 0.0, 0.49],
            [0.22, 0.5 * l - 0.02, 0.025],
            tilt,
        );
    }
    // Exhaust side: manifold / headers, heat shield, turbos.
    let mani = rot([ex * 0.2, 0.0, 0.22]);
    if e.detail {
        for k in 0..cyl {
            let y = -0.5 * l + (k as f32 + 0.5) * l / cyl as f32;
            let path = [
                rot([ex * 0.16, y, 0.30]),
                rot([ex * 0.25, y, 0.24]),
                [mani[0] + ex * 0.04, 0.0, mani[2] - 0.08],
            ];
            e.tube(Mat::Headers, &path, 0.018, 6);
        }
        if turbos == 0 {
            e.bxr(
                Mat::Metal,
                [ex * 0.27, 0.0, 0.24],
                [0.02, 0.5 * l - 0.04, 0.07],
                tilt,
            );
        }
    } else {
        e.cyl(
            Mat::Headers,
            [mani[0] + ex * 0.04, 0.5 * l - 0.05, mani[2]],
            [mani[0] + ex * 0.04, -0.5 * l + 0.05, mani[2]],
            0.03,
            6,
        );
    }
    let mut charge_out = None;
    for t in 0..turbos {
        let y = if turbos == 2 {
            0.2 * l - 0.36 * l * t as f32
        } else {
            0.08
        };
        let c = [ex * 0.32, y, 0.13];
        e.cyl(
            Mat::Headers,
            [c[0], c[1] - 0.05, c[2]],
            [c[0], c[1] + 0.02, c[2]],
            0.06,
            if e.detail { 10 } else { 6 },
        );
        e.cyl(
            Mat::Block,
            [c[0], c[1] + 0.02, c[2]],
            [c[0], c[1] + 0.06, c[2]],
            0.03,
            6,
        );
        e.cyl(
            Mat::Metal,
            [c[0], c[1] + 0.06, c[2]],
            [c[0], c[1] + 0.13, c[2]],
            0.068,
            if e.detail { 10 } else { 6 },
        );
        if e.detail {
            e.tube(
                Mat::Metal,
                &[[c[0], c[1] + 0.13, c[2]], [c[0], c[1] + 0.18, c[2] + 0.02]],
                0.035,
                6,
            );
            e.tube(
                Mat::Headers,
                &[mani, [c[0] - ex * 0.03, c[1] - 0.03, c[2] + 0.04]],
                0.028,
                6,
            );
        }
        charge_out.get_or_insert([c[0], c[1] + 0.12, c[2] + 0.07]);
        e.exits.push([c[0], c[1] - 0.04, c[2] - 0.2]);
    }
    if turbos == 0 {
        e.exits.push([mani[0] + ex * 0.02, -0.12, -0.18]);
    }
    if let Some(co) = charge_out {
        e.charge = Some((co, throttle));
    }
    if longitudinal {
        gearbox(e, l, if cyl == 6 { 0.7 } else { 0.62 }, 0.14, 0.09);
    } else {
        // Transaxle on the −y end (left side of an FF car).
        e.bx(
            Mat::Block,
            [0.0, -0.5 * l - 0.17, -0.02],
            [0.15, 0.17, 0.15],
        );
    }
    if e.detail && longitudinal {
        fan(e, l);
    }
}

/// Twin-turbo 60° V6 (G), transverse, under a plastic cover.
fn tt_v6(e: &mut E) {
    let l = 0.46;
    v_banks(e, l, 30.0, 0.2, 0.16, 0.42);
    e.bx(Mat::Trim, [0.0, 0.0, 0.47], [0.25, 0.21, 0.03]);
    if e.detail {
        e.tube(
            Mat::Metal,
            &[[0.0, 0.21, 0.44], [-0.15, 0.4, 0.35], [-0.3, 0.52, 0.2]],
            0.032,
            6,
        );
    }
    for side in [-1.0f32, 1.0] {
        e.cyl(
            Mat::Headers,
            [side * 0.3, -0.5 * l, 0.0],
            [side * 0.3, -0.5 * l + 0.06, 0.0],
            0.055,
            6,
        );
        e.exits.push([side * 0.3, -0.5 * l, -0.15]);
    }
    e.bx(
        Mat::Block,
        [0.0, -0.5 * l - 0.17, -0.02],
        [0.15, 0.17, 0.15],
    );
}

/// Flat-plane V8 (H), mid: red covers, twin silver plenums, transverse gearbox behind,
/// intake tubes to the side scoops.
fn flat_plane_v8(e: &mut E, half_w: f32) {
    let l = 0.60;
    v_banks(e, l, 45.0, 0.22, 0.18, 0.5);
    for side in [-1.0f32, 1.0] {
        e.bx(Mat::Metal, [side * 0.075, 0.0, 0.33], [0.06, 0.225, 0.05]);
        if e.detail {
            for k in 0..4 {
                let y = -0.18 + 0.12 * k as f32;
                e.bx(Mat::Metal, [side * 0.14, y, 0.32], [0.02, 0.03, 0.035]);
            }
            e.tube(
                Mat::Trim,
                &[
                    [side * 0.1, 0.15, 0.38],
                    [side * 0.4, 0.2, 0.32],
                    [side * (half_w - 0.14), 0.25, 0.25],
                ],
                0.06,
                8,
            );
        }
        let x = headers(e, side, 4, l, 0.33, 0.15);
        e.exits.push(x);
    }
    e.bx(
        Mat::Block,
        [0.0, -0.5 * l - 0.55, -0.02],
        [0.275, 0.15, 0.16],
    );
    e.gearbox_end = None;
}

/// V10 / V12 (I), mid: tinted covers with carbon strips, carbon airbox (or stacks),
/// longitudinal dual-clutch box behind.
fn v10(e: &mut E, cyl: usize, stacks: bool) {
    let l = if cyl == 12 { 0.82 } else { 0.69 };
    v_banks(e, l, 45.0, 0.23, 0.17, 0.62);
    for side in [-1.0f32, 1.0] {
        e.bxr(
            Mat::Carbon,
            [0.0, 0.0, 0.478],
            [0.022, 0.29, 0.004],
            side * 45.0,
        );
    }
    if stacks {
        let per = cyl / 2;
        for side in [-1.0f32, 1.0] {
            for k in 0..per {
                let y = -0.5 * l + 0.06 + (k as f32 + 0.5) * (l - 0.12) / per as f32;
                let c = [side * 0.06, y, 0.36];
                e.f.lathe(
                    &mut e.b,
                    &key(Mat::Metal),
                    c,
                    [0.0, 0.0, 1.0],
                    &[[0.03, 0.0], [0.03, 0.05], [0.04, 0.075], [0.044, 0.08]],
                    8,
                );
            }
        }
    } else {
        e.bx(Mat::Carbon, [0.0, 0.0, 0.37], [0.2, 0.275, 0.06]);
        for side in [-1.0f32, 1.0] {
            e.cyl(
                Mat::Trim,
                [side * 0.09, 0.03, 0.43],
                [side * 0.09, 0.03, 0.434],
                0.045,
                8,
            );
        }
    }
    for side in [-1.0f32, 1.0] {
        let x = headers(e, side, cyl / 2, l, 0.34, 0.15);
        e.exits.push(x);
    }
    e.bx(
        Mat::Block,
        [0.0, -0.5 * l - 0.27, -0.02],
        [0.175, 0.25, 0.15],
    );
}

/// Boxer (flat 4/6): two horizontal banks.
fn flat(e: &mut E) {
    let l = 0.5;
    e.bx(Mat::Block, [0.0, 0.0, 0.0], [0.15, 0.5 * l, 0.14]);
    for side in [-1.0f32, 1.0] {
        e.bx(
            Mat::Block,
            [side * 0.27, 0.0, 0.0],
            [0.12, 0.5 * l - 0.02, 0.1],
        );
        e.bx(
            Mat::Cover,
            [side * 0.4, 0.0, 0.0],
            [0.03, 0.5 * l - 0.06, 0.08],
        );
        e.exits.push([side * 0.3, 0.0, -0.15]);
    }
    e.bx(Mat::Metal, [0.0, 0.0, 0.2], [0.18, 0.18, 0.06]);
}

/// Rotary: compact block + intake.
fn rotary(e: &mut E) {
    e.bx(Mat::Block, [0.0, 0.0, 0.05], [0.18, 0.2, 0.2]);
    e.bx(Mat::Cover, [0.0, 0.0, 0.27], [0.14, 0.16, 0.03]);
    e.bx(Mat::Metal, [-e.ex * 0.22, 0.0, 0.2], [0.06, 0.15, 0.08]);
    e.exits.push([e.ex * 0.2, 0.0, -0.1]);
}

/// Electric: motor can + inverter box (no exhaust).
fn electric(e: &mut E) {
    e.cyl(Mat::Metal, [-0.2, 0.0, 0.0], [0.2, 0.0, 0.0], 0.13, 10);
    e.bx(Mat::Trim, [0.0, 0.0, 0.22], [0.25, 0.18, 0.08]);
}

/// Emit the engine (+ radiator, intercooler) and report its connection points.
pub fn engine(def: &VehicleDef, p: &BodyProfile, s: &CarSpec, b: &mut Builder) -> EngineOut {
    if s.engine == Engine::None {
        return EngineOut::default();
    }
    let y_fa = def.wheels[0].position_m[1];
    let y_ra = def.wheels[2].position_m[1];
    let mount = s.mount;
    let mid = matches!(mount, Mount::Mid | Mount::Rear);
    let transverse = mount == Mount::Transverse;
    let mut e = E {
        b: Builder::new(),
        f: Frame::new(
            [0.0; 3],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        ),
        detail: s.engine_detail && s.hero,
        ex: if transverse { -1.0 } else { 1.0 },
        exits: Vec::new(),
        gearbox_end: None,
        charge: None,
    };
    let stacks = false;
    // (crank height, block front face y, half length) per type and mount.
    let (crank, half_l) = match s.engine {
        Engine::BigBlockV8 => {
            big_block(&mut e);
            (0.40, 0.33)
        }
        Engine::SmallBlockV8 { old } => {
            small_block(&mut e, old);
            (if def.body_size_m[2] > 1.7 { 0.55 } else { 0.40 }, 0.31)
        }
        Engine::DohcV8 => {
            dohc_v8(&mut e);
            (0.36, 0.35)
        }
        Engine::Inline4 { turbo, fmic } => {
            let tilt = if transverse { 12.0 } else { 0.0 };
            inline(
                &mut e,
                4,
                tilt,
                u8::from(turbo),
                false,
                turbo && !fmic,
                !transverse,
            );
            (if transverse { 0.30 } else { 0.32 }, 0.25)
        }
        Engine::Inline6 { turbos, itb, fmic } => {
            let tilt = if itb { -30.0 } else { -10.0 };
            inline(&mut e, 6, tilt, turbos, itb, turbos > 0 && !fmic, true);
            (0.33, 0.39)
        }
        Engine::TwinTurboV6 => {
            tt_v6(&mut e);
            (0.38, 0.23)
        }
        Engine::FlatPlaneV8 => {
            flat_plane_v8(&mut e, p.half_w);
            (0.28, 0.30)
        }
        Engine::V10 => {
            v10(&mut e, 10, stacks);
            (0.27, 0.345)
        }
        Engine::V12 => {
            v10(&mut e, 12, stacks);
            (0.27, 0.41)
        }
        Engine::Flat => {
            flat(&mut e);
            (0.3, 0.25)
        }
        Engine::Rotary => {
            rotary(&mut e);
            (0.3, 0.2)
        }
        Engine::Electric => {
            electric(&mut e);
            (0.3, 0.2)
        }
        Engine::None => unreachable!(),
    };
    // Placement: axes and origin (crank centre, mid block) in chassis space.
    let (ax, oy, ox) = match mount {
        Mount::Transverse => (
            [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            y_fa + 0.05,
            0.05,
        ),
        Mount::Longitudinal => {
            let front_off = match s.engine {
                Engine::BigBlockV8 => 0.12,
                Engine::SmallBlockV8 { .. } if def.body_size_m[2] > 1.7 => 0.05,
                Engine::SmallBlockV8 { .. } => 0.10,
                Engine::DohcV8 => 0.08,
                Engine::Inline6 { .. } => 0.18,
                _ => 0.02,
            };
            (
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                y_fa + front_off - half_l,
                0.0,
            )
        }
        Mount::Mid => {
            let front = match s.engine {
                Engine::V10 | Engine::V12 => 1.05,
                _ => 0.95,
            };
            (
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                y_ra + front - half_l,
                0.0,
            )
        }
        Mount::Rear => (
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            y_ra - 0.35,
            0.0,
        ),
    };
    // Keep the whole engine (with its gearbox / airbox) inside the body ends.
    let to_xy = |l: [f32; 3]| -> [f32; 2] {
        [
            ox + ax[0][0] * l[0] + ax[1][0] * l[1],
            oy + ax[0][1] * l[0] + ax[1][1] * l[1],
        ]
    };
    let mut verts: Vec<[f32; 3]> = Vec::new();
    for k in e.b.keys() {
        if let Some((pos, _, _)) = e.b.geometry(&k.name) {
            verts.extend_from_slice(pos);
        }
    }
    let min_z = verts.iter().map(|v| v[2]).fold(0.0, f32::min);
    let mut dy = 0.0;
    {
        let ys: Vec<f32> = verts.iter().map(|v| to_xy(*v)[1]).collect();
        let (lo, hi) = (
            ys.iter().copied().fold(f32::MAX, f32::min),
            ys.iter().copied().fold(f32::MIN, f32::max),
        );
        if hi > p.y_nose - 0.10 {
            dy = p.y_nose - 0.10 - hi;
        } else if lo < p.y_tail + 0.15 {
            dy = p.y_tail + 0.15 - lo;
        }
    }
    let surface = |x: f32, y: f32| -> f32 {
        let y = y.clamp(p.y_tail + 0.02, p.y_nose - 0.02);
        let x = x.clamp(-0.95 * p.half_w, 0.95 * p.half_w);
        p.top_point(y, x).0[2] - HOOD_CLEARANCE
    };
    // Lower the crank (down to the sump clearance), then squash z until everything clears.
    let mut z0 = crank;
    let hood: Vec<f32> = verts
        .iter()
        .map(|v| {
            let q = to_xy(*v);
            surface(q[0], q[1] + dy)
        })
        .collect();
    let excess = verts
        .iter()
        .zip(&hood)
        .map(|(v, h)| z0 + v[2] - h)
        .fold(f32::MIN, f32::max);
    let z_min = SUMP_CLEARANCE - min_z;
    if excess > 0.0 {
        z0 = (z0 - excess).max(z_min.min(z0));
    }
    let mut kz: f32 = 1.0;
    for (v, h) in verts.iter().zip(&hood) {
        if v[2] > 0.02 && z0 + v[2] > *h {
            kz = kz.min((h - z0) / v[2]);
        }
    }
    let kz = kz.clamp(0.55, 1.0);
    let place = |l: [f32; 3]| -> [f32; 3] {
        let q = to_xy(l);
        [q[0], q[1] + dy, z0 + kz * l[2]]
    };
    let region = if mid {
        BodyRegion::FrameRear
    } else {
        BodyRegion::FrameFront
    };
    let cover_slot = match s.cover {
        Cover::Tint => MaterialSlot::Caliper,
        Cover::Chrome => MaterialSlot::Chrome,
        Cover::Black => MaterialSlot::Trim,
        Cover::Silver => MaterialSlot::Metal,
        Cover::Dull => MaterialSlot::Engine,
    };
    for k in e.b.keys() {
        let Some((pos, _, idx)) = e.b.geometry(&k.name) else {
            continue;
        };
        let material = if k.name == "engine_cover" {
            cover_slot
        } else {
            k.material
        };
        let out = PartKey::body(&k.name, region, PartKind::Engine, material).rigid();
        for t in idx.chunks(3) {
            b.poly(
                &out,
                &[
                    place(pos[t[0] as usize]),
                    place(pos[t[1] as usize]),
                    place(pos[t[2] as usize]),
                ],
            );
        }
    }
    if mid && e.detail && matches!(s.engine, Engine::V10 | Engine::V12) {
        engine_tub(b, region, &place, half_l, p.half_w, s.engine);
    }
    if !mid {
        radiator(b, p, def, s, transverse, z0);
        if let (
            Engine::Inline4 { fmic: true, .. } | Engine::Inline6 { fmic: true, .. },
            Some((co, th)),
        ) = (s.engine, e.charge)
        {
            intercooler(b, p, s.hero, place(co), place(th));
        }
    }
    EngineOut {
        exits: e.exits.iter().map(|&q| place(q)).collect(),
        gearbox_end: e.gearbox_end.map(place),
        mid,
        crank_z: z0,
    }
}

/// Dark tub around a mid engine (seen through the engine-cover glass / louvres).
fn engine_tub(
    b: &mut Builder,
    region: BodyRegion,
    place: &dyn Fn([f32; 3]) -> [f32; 3],
    half_l: f32,
    half_w: f32,
    engine: Engine,
) {
    let key = PartKey::body("engine_tub", region, PartKind::Engine, MaterialSlot::Trim).rigid();
    let w = (half_w - 0.12).min(0.6);
    let rear = if matches!(engine, Engine::V10 | Engine::V12) {
        -half_l - 0.55
    } else {
        -half_l - 0.72
    };
    let (y0, y1, z0, z1) = (rear, half_l + 0.08, -0.2, 0.32);
    let c = |x: f32, y: f32, z: f32| place([x, y, z]);
    // Walls facing inward (seen from above / inside).
    let quads = [
        (
            [c(-w, y0, z0), c(w, y0, z0), c(w, y1, z0), c(-w, y1, z0)],
            [0.0, 0.0, 1.0],
        ),
        (
            [c(-w, y0, z0), c(-w, y1, z0), c(-w, y1, z1), c(-w, y0, z1)],
            [1.0, 0.0, 0.0],
        ),
        (
            [c(w, y0, z0), c(w, y1, z0), c(w, y1, z1), c(w, y0, z1)],
            [-1.0, 0.0, 0.0],
        ),
        (
            [c(-w, y1, z0), c(w, y1, z0), c(w, y1, z1), c(-w, y1, z1)],
            [0.0, -1.0, 0.0],
        ),
    ];
    for (q, n) in quads {
        b.poly_facing(&key, &q, n);
    }
}

/// Radiator core ahead of a front engine.
fn radiator(
    b: &mut Builder,
    p: &BodyProfile,
    def: &VehicleDef,
    s: &CarSpec,
    transverse: bool,
    crank: f32,
) {
    let y_fa = def.wheels[0].position_m[1];
    let key = PartKey::body(
        "engine_radiator",
        BodyRegion::FrameFront,
        PartKind::Engine,
        MaterialSlot::Grille,
    )
    .rigid();
    let y = (y_fa + if transverse { 0.45 } else { 0.55 }).min(p.y_nose - 0.14);
    let hw = (p.half_width(y) - 0.25).clamp(0.2, 0.36);
    let top = (p.top_point(y, 0.0).0[2] - 0.06).min(crank + 0.42);
    let bottom = (p.z_floor + 0.08).max(0.16);
    if top > bottom + 0.1 {
        b.cuboid(&key, [-hw, y - 0.02, bottom], [hw, y + 0.02, top]);
        if s.hero {
            let shroud = PartKey::body(
                "engine_trim_shroud",
                BodyRegion::FrameFront,
                PartKind::Engine,
                MaterialSlot::Trim,
            )
            .rigid();
            b.cuboid(
                &shroud,
                [-hw + 0.03, y - 0.06, bottom + 0.03],
                [hw - 0.03, y - 0.02, top - 0.03],
            );
        }
    }
}

/// Front-mount intercooler behind the lower grille, with charge pipes to the compressor
/// outlet `co` and the throttle body `th` (chassis).
fn intercooler(b: &mut Builder, p: &BodyProfile, hero: bool, co: [f32; 3], th: [f32; 3]) {
    let core = PartKey::body(
        "engine_intercooler",
        BodyRegion::FrameFront,
        PartKind::Engine,
        MaterialSlot::Grille,
    )
    .rigid();
    let metal = PartKey::body(
        "engine_charge_pipes",
        BodyRegion::FrameFront,
        PartKind::Engine,
        MaterialSlot::Metal,
    )
    .rigid();
    let y = p.y_nose - 0.11;
    let z0 = (p.z_floor + 0.1).max(0.2);
    let z1 = (z0 + 0.18).min(p.z_bumper_top.0 - 0.03).max(z0 + 0.1);
    let hw = (p.half_width(y) - 0.3).clamp(0.2, 0.28);
    b.cuboid(&core, [-hw, y - 0.033, z0], [hw, y + 0.033, z1]);
    // End tanks.
    b.cuboid(
        &metal,
        [hw, y - 0.04, z0 - 0.01],
        [hw + 0.05, y + 0.04, z1 + 0.01],
    );
    b.cuboid(
        &metal,
        [-hw - 0.05, y - 0.04, z0 - 0.01],
        [-hw, y + 0.04, z1 + 0.01],
    );
    let zm = 0.5 * (z0 + z1);
    let sides = if hero { 8 } else { 6 };
    let r = 0.03;
    let a = [
        co,
        [co[0], co[1] + 0.05, co[2] - 0.15],
        [co[0].signum() * (hw + 0.02), y - 0.1, zm],
        [co[0].signum() * (hw + 0.02), y - 0.04, zm],
    ];
    tube(b, &metal, &a, r, sides, false);
    let side_t = -co[0].signum();
    let c = [
        [side_t * (hw + 0.02), y - 0.04, zm],
        [side_t * (hw + 0.02), y - 0.12, zm + 0.05],
        [th[0], th[1] + 0.12, th[2] + 0.02],
        th,
    ];
    tube(b, &metal, &c, r, sides, false);
}
