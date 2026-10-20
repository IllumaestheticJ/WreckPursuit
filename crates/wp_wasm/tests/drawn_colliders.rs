//! Every collider the player can hit hard is drawn (lead helper, playtest row 50 "hitting
//! invisible objects").
//!
//! Drives a city chase and a free drive in Port Vesper v2 headless (a seeded wander at full
//! throttle that hits kerbs, walls, buildings and traffic, plus a parked car knocked every
//! few seconds), collects every hard static-world contact (`SimEvent::Contact` with the world,
//! > 2,000 N·s in one tick) and checks each lies within 1 m of something the browser draws:
//! the city greybox the testbed loads (`data/city/vesper_v2.greybox.glb`, parsed here: roads,
//! sidewalks, kerbs, buildings, walls, bridges, terrain) or a prop from `props_static()`
//! (parked cars, cones; drawn by `apps/testbed-web/src/render/props.ts`).
//!
//! `cargo test -p wp_wasm --test drawn_colliders -- --nocapture` prints the per-kind tally.

use std::collections::BTreeMap;

use simcli::scenario::CitySpec;
use wp_contracts::{DamageMode, EntityId, SimEvent, VehicleInput, WORLD_ENTITY};
use wp_math::Rng;
use wp_wasm::props::{self, PROP_STRIDE};
use wp_wasm::scenario;
use wp_wasm::session::Session;

/// A static contact this hard in one tick must hit something drawn, N·s.
const HARD_NS: f32 = 2000.0;
/// How close to drawn geometry the contact point must be, m.
const DRAWN_WITHIN_M: f32 = 1.0;

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn add_s(a: V3, b: V3, s: f32) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}

/// Closest point on triangle `abc` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_tri(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add_s(a, ab, d1 / (d1 - d3));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add_s(a, ac, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add_s(b, sub(c, b), w);
    }
    let denom = 1.0 / (va + vb + vc);
    add_s(add_s(a, ab, vb * denom), ac, vc * denom)
}

/// The drawn city: greybox triangles in sim space, binned on a grid by their bounding box.
struct Drawn {
    tris: Vec<[V3; 3]>,
    material: Vec<u16>,
    names: Vec<String>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
    /// Triangles too big to bin (sea, large terrain quads): always checked.
    big: Vec<u32>,
}

const CELL_M: f32 = 8.0;

fn cell(v: f32) -> i32 {
    (v / CELL_M).floor() as i32
}

impl Drawn {
    /// Parse the greybox `.glb` the testbed loads (glTF +Y up → sim +Z up).
    fn load(path: &std::path::Path) -> Drawn {
        let bytes = std::fs::read(path).expect("greybox glb (run `just city-v2`)");
        assert_eq!(&bytes[0..4], b"glTF");
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
        let jl = u32_at(12);
        let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + jl]).unwrap();
        let bin0 = 20 + jl + 8;
        let views = doc["bufferViews"].as_array().unwrap();
        let accs = doc["accessors"].as_array().unwrap();
        let view_off = |acc: usize| -> usize {
            let a = &accs[acc];
            let v = &views[a["bufferView"].as_u64().unwrap() as usize];
            bin0 + v["byteOffset"].as_u64().unwrap_or(0) as usize
                + a["byteOffset"].as_u64().unwrap_or(0) as usize
        };
        let names: Vec<String> = doc["materials"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["name"].as_str().unwrap_or("?").to_string())
            .collect();
        let f32_at = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let mut d = Drawn {
            tris: Vec::new(),
            material: Vec::new(),
            names,
            grid: BTreeMap::new(),
            big: Vec::new(),
        };
        for mesh in doc["meshes"].as_array().unwrap() {
            for prim in mesh["primitives"].as_array().unwrap() {
                let pa = prim["attributes"]["POSITION"].as_u64().unwrap() as usize;
                let ia = prim["indices"].as_u64().unwrap() as usize;
                let mat = prim["material"].as_u64().unwrap_or(0) as u16;
                let n = accs[pa]["count"].as_u64().unwrap() as usize;
                let po = view_off(pa);
                let pos: Vec<V3> = (0..n)
                    .map(|i| {
                        let o = po + i * 12;
                        // glTF (x, y, z) = sim (x, z, −y).
                        [f32_at(o), -f32_at(o + 8), f32_at(o + 4)]
                    })
                    .collect();
                assert_eq!(
                    accs[ia]["componentType"].as_u64(),
                    Some(5123),
                    "u16 indices"
                );
                let ni = accs[ia]["count"].as_u64().unwrap() as usize;
                let io = view_off(ia);
                let idx = |k: usize| {
                    u16::from_le_bytes(bytes[io + k * 2..io + k * 2 + 2].try_into().unwrap())
                        as usize
                };
                for t in 0..ni / 3 {
                    d.push(
                        [pos[idx(t * 3)], pos[idx(t * 3 + 1)], pos[idx(t * 3 + 2)]],
                        mat,
                    );
                }
            }
        }
        d
    }

    fn push(&mut self, t: [V3; 3], mat: u16) {
        let id = self.tris.len() as u32;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for v in &t {
            x0 = x0.min(v[0]);
            y0 = y0.min(v[1]);
            x1 = x1.max(v[0]);
            y1 = y1.max(v[1]);
        }
        self.tris.push(t);
        self.material.push(mat);
        let (cx0, cy0, cx1, cy1) = (cell(x0), cell(y0), cell(x1), cell(y1));
        if (cx1 - cx0 + 1) * (cy1 - cy0 + 1) > 256 {
            self.big.push(id);
            return;
        }
        for cx in cx0..=cx1 {
            for cy in cy0..=cy1 {
                self.grid.entry((cx, cy)).or_default().push(id);
            }
        }
    }

    /// Distance from `p` to the nearest drawn triangle (searching ±`reach` m), and its material.
    fn nearest(&self, p: V3, reach: f32) -> (f32, &str) {
        let mut best = (f32::INFINITY, u16::MAX);
        let mut test = |id: u32| {
            let t = &self.tris[id as usize];
            let d = len(sub(p, closest_on_tri(p, t[0], t[1], t[2])));
            if d < best.0 {
                best = (d, self.material[id as usize]);
            }
        };
        for cx in cell(p[0] - reach)..=cell(p[0] + reach) {
            for cy in cell(p[1] - reach)..=cell(p[1] + reach) {
                for id in self.grid.get(&(cx, cy)).into_iter().flatten() {
                    test(*id);
                }
            }
        }
        for id in &self.big {
            test(*id);
        }
        let name = self
            .names
            .get(best.1 as usize)
            .map_or("none", |s| s.as_str());
        (best.0, name)
    }
}

/// Distance from `p` to the oriented box of prop record `r` (`props_static()` layout).
fn prop_distance(p: V3, pos: V3, q: [f32; 4], half: V3) -> f32 {
    // Into the prop frame: rotate by the inverse quaternion.
    let d = sub(p, pos);
    let (x, y, z, w) = (-q[0], -q[1], -q[2], q[3]);
    let t = [
        2.0 * (y * d[2] - z * d[1]),
        2.0 * (z * d[0] - x * d[2]),
        2.0 * (x * d[1] - y * d[0]),
    ];
    let l = [
        d[0] + w * t[0] + (y * t[2] - z * t[1]),
        d[1] + w * t[1] + (z * t[0] - x * t[2]),
        d[2] + w * t[2] + (x * t[1] - y * t[0]),
    ];
    let o = [
        (l[0].abs() - half[0]).max(0.0),
        (l[1].abs() - half[1]).max(0.0),
        (l[2].abs() - half[2]).max(0.0),
    ];
    len(o)
}

/// Nearest drawn prop to `p` (current sim poses, as `props_moved()` keeps them drawn).
fn nearest_prop(s: &Session, half: &[V3], p: V3) -> f32 {
    let mut best = f32::INFINITY;
    for (i, h) in half.iter().enumerate() {
        let (pos, q) = s.sim().prop_pose(i).unwrap();
        if (pos[0] - p[0]).abs() > 8.0 || (pos[1] - p[1]).abs() > 8.0 {
            continue;
        }
        best = best.min(prop_distance(p, pos, q, *h));
    }
    best
}

#[derive(Default)]
struct Tally {
    hard: usize,
    /// Hard contacts per what they were nearest to (greybox material or "prop").
    by_kind: BTreeMap<String, usize>,
    /// Hard contacts only a prop explains (invisible before props were drawn).
    prop_only: usize,
    invisible: Vec<(V3, f32, String, f32, f32)>,
    max_ns: f32,
}

/// Drive `sc` for `seconds` with a seeded wander; knock a parked car every `knock_s`.
fn sweep(name: &str, sc: simcli::Scenario, seconds: f32, knock_s: f32, drawn: &Drawn) -> Tally {
    let mut s = Session::new(sc, false).unwrap();
    let list = props::static_list(s.sim());
    let half: Vec<V3> = list
        .chunks(PROP_STRIDE)
        .map(|r| [r[8], r[9], r[10]])
        .collect();
    assert!(half.len() > 1000, "{name}: city has its parked cars");
    let player: EntityId = s.entities()[0];
    let mut rng = Rng::new(0x5EED_0050);
    let mut tally = Tally::default();
    let frames = (seconds * 60.0) as u32;
    let mut steer = 0.0f32;
    let mut stuck_s = 0.0f32;
    let mut reverse_s = 0.0f32;
    for f in 0..frames {
        let t = f as f32 / 60.0;
        if f % 90 == 0 {
            steer = rng.range_f32(-0.8, 0.8);
        }
        if f > 0 && f % (knock_s * 60.0) as u32 == 0 {
            // A hard hit on a parked car (≈ 90 km/h into its flank).
            let _ = s.debug_knock_prop(player, None, 25.0);
            stuck_s = 0.0;
        }
        let speed = s
            .sim()
            .vehicle_states()
            .into_iter()
            .find(|(id, _)| *id == player)
            .map_or(0.0, |(_, st)| st.speed_mps);
        stuck_s = if speed.abs() < 2.0 {
            stuck_s + 1.0 / 60.0
        } else {
            0.0
        };
        if stuck_s > 2.0 && reverse_s <= 0.0 {
            reverse_s = 1.2;
        }
        if stuck_s > 7.0 {
            let _ = s.reset_vehicle(player);
            stuck_s = 0.0;
        }
        let input = if reverse_s > 0.0 {
            reverse_s -= 1.0 / 60.0;
            VehicleInput {
                steer: -steer,
                brake: 1.0,
                ..Default::default()
            }
        } else {
            VehicleInput {
                steer,
                throttle: 1.0,
                ..Default::default()
            }
        };
        let _ = s.set_input(player, input);
        s.run_ticks(4);
        for ev in s.drain_events() {
            let SimEvent::Contact {
                a,
                b,
                point,
                impulse_ns,
                ..
            } = ev
            else {
                continue;
            };
            if b != WORLD_ENTITY && a != WORLD_ENTITY {
                continue;
            }
            if impulse_ns < HARD_NS {
                continue;
            }
            tally.hard += 1;
            tally.max_ns = tally.max_ns.max(impulse_ns);
            let (dg, mat) = drawn.nearest(point, DRAWN_WITHIN_M + 1.0);
            let dp = nearest_prop(&s, &half, point);
            let kind = if dp <= dg {
                "prop".to_string()
            } else {
                mat.to_string()
            };
            *tally.by_kind.entry(kind).or_default() += 1;
            if dp <= DRAWN_WITHIN_M && dg > DRAWN_WITHIN_M {
                tally.prop_only += 1;
            }
            if dg.min(dp) > DRAWN_WITHIN_M {
                tally
                    .invisible
                    .push((point, impulse_ns, mat.to_string(), dg, dp));
                eprintln!(
                    "{name}: t={t:.1}s invisible hard contact {impulse_ns:.0} N·s at ({:.1}, {:.1}, {:.1}): greybox {dg:.2} m ({mat}), prop {dp:.2} m",
                    point[0], point[1], point[2]
                );
            }
        }
    }
    eprintln!(
        "{name}: {} hard static contacts (max {:.0} N·s); nearest drawn: {:?}; {} explained only by a prop; {} invisible",
        tally.hard,
        tally.max_ns,
        tally.by_kind,
        tally.prop_only,
        tally.invisible.len()
    );
    tally
}

#[test]
fn every_hard_static_contact_is_near_something_drawn() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/city");
    let drawn = Drawn::load(&dir.join("vesper_v2.greybox.glb"));
    let chase = sweep(
        "city chase",
        scenario::city_chase("lindwurm_4t", DamageMode::Arcade),
        120.0,
        8.0,
        &drawn,
    );
    let free = sweep(
        "free drive",
        scenario::free_drive_in("debug_sedan", DamageMode::Arcade, CitySpec::vesper()),
        60.0,
        8.0,
        &drawn,
    );
    for (name, t) in [("city chase", &chase), ("free drive", &free)] {
        assert!(
            t.hard >= 10,
            "{name}: only {} hard contacts sampled",
            t.hard
        );
        assert!(
            t.by_kind.get("prop").copied().unwrap_or(0) > 0,
            "{name}: the sweep never hit a prop"
        );
        assert!(
            t.invisible.is_empty(),
            "{name}: {} hard contacts with nothing drawn within {DRAWN_WITHIN_M} m: {:?}",
            t.invisible.len(),
            &t.invisible[..t.invisible.len().min(5)]
        );
    }
}

#[test]
fn the_reported_crash_site_has_a_parked_car() {
    // Playtest row 50: 66,800 N·s static contact at ~150 km/h around (-367.8, 172.1) in the
    // city chase. A parked car (now drawn) is within reach of that point.
    let s = Session::new(
        scenario::city_chase("lindwurm_4t", DamageMode::Arcade),
        false,
    )
    .unwrap();
    let list = props::static_list(s.sim());
    let near = list
        .chunks(PROP_STRIDE)
        .map(|r| ((r[1] + 367.8).powi(2) + (r[2] - 172.1).powi(2)).sqrt())
        .fold(f32::INFINITY, f32::min);
    assert!(
        near < 4.0,
        "nearest prop to the crash site is {near:.1} m away"
    );
}
