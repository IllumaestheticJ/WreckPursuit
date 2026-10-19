//! `gen-lattice`: procedural damage lattices from overall dimensions and a body profile.
//!
//! The body is sliced into *stations* along Y (rear bumper face → front bumper face, ≤ 0.36 m
//! apart, with extra stations at the axles, firewall, pillars and glass lines). Each station
//! carries up to four layers of nodes — floor, mid (bumper / door-beam height), belt (hood /
//! window line) and top (roof or glass) — in five columns (outer left, left rail, centre,
//! right rail, outer right). Beams join neighbouring slots within a station and to the next
//! station (axial + both face diagonals), so every face is triangulated.
//!
//! Load paths are made of different materials and sections: front/rear rails get a soft
//! sacrificial segment behind the bumper beam and harder segments behind it (progressive
//! crumpling), the cabin floor is pinned to the chassis, sills/pillars/roof rails are high
//! strength steel, panels mild steel or aluminium, bumper covers polypropylene, wheel hubs
//! cast aluminium (brittle) and glass brittle. The engine is a rigid 8-node block on mounts.
//! Bumpers, hood, trunk and doors are detachable groups; components are mounted where they
//! sit in a real car.

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{
    BodyRegion, Component, ComponentMount, Detachable, LatticeBeam, LatticeDef, LatticeNode,
};

/// Body style. Changes the greenhouse, door count, bed and structural strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyProfile {
    /// Four-door three-box saloon.
    Sedan,
    /// Two-door three-box coupe (long doors, short rear side).
    Coupe,
    /// Two-box hatchback (roof to the rear, tailgate).
    Hatch,
    /// Pickup: two-door cab and an open bed with a tailgate; stronger rails.
    Truck,
    /// Two-box SUV: tall, roof to the rear, stronger rails.
    Suv,
    /// Police sedan: sedan body, reinforced rails and a push bar.
    Cruiser,
}

impl BodyProfile {
    /// All profiles.
    pub const ALL: [BodyProfile; 6] = [
        BodyProfile::Sedan,
        BodyProfile::Coupe,
        BodyProfile::Hatch,
        BodyProfile::Truck,
        BodyProfile::Suv,
        BodyProfile::Cruiser,
    ];

    /// Parse a lowercase profile name (`sedan`, `coupe`, `hatch`, `truck`, `suv`, `cruiser`).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "sedan" => Self::Sedan,
            "coupe" => Self::Coupe,
            "hatch" | "hatchback" => Self::Hatch,
            "truck" | "pickup" => Self::Truck,
            "suv" => Self::Suv,
            "cruiser" | "police" => Self::Cruiser,
            _ => return None,
        })
    }
}

/// Inputs for [`generate_lattice`].
#[derive(Debug, Clone, PartialEq)]
pub struct LatticeParams {
    /// Lattice id (also the output file stem).
    pub id: String,
    /// Body style.
    pub profile: BodyProfile,
    /// Body width (X), m.
    pub width_m: f32,
    /// Body length (Y), m.
    pub length_m: f32,
    /// Body height (Z), m.
    pub height_m: f32,
    /// Wheelbase, m. Axles sit at y = ±wheelbase/2 (chassis origin = wheelbase centre).
    pub wheelbase_m: f32,
    /// Height of the floor node layer above the ground, m.
    pub ground_clearance_m: f32,
    /// Total lattice mass, kg (≈ 25 % of the car).
    pub lattice_mass_kg: f32,
    /// Car mass, kg. Beam sections scale with `car_mass / 1400` so every car crushes about
    /// the same distance at the same speed.
    pub car_mass_kg: f32,
    /// Front overhang (front axle → bumper face), m. `None` = centred body.
    pub front_overhang_m: Option<f32>,
    /// Extra multiplier on every structural section (durability tuning per car), 1 = stock.
    pub strength_scale: f32,
}

impl LatticeParams {
    /// Params with defaults for clearance (profile dependent), car mass (4 × lattice),
    /// centred overhangs and stock strength.
    pub fn new(
        id: &str,
        profile: BodyProfile,
        size_wlh_m: [f32; 3],
        wheelbase_m: f32,
        lattice_mass_kg: f32,
    ) -> Self {
        Self {
            id: id.to_string(),
            profile,
            width_m: size_wlh_m[0],
            length_m: size_wlh_m[1],
            height_m: size_wlh_m[2],
            wheelbase_m,
            ground_clearance_m: match profile {
                BodyProfile::Truck | BodyProfile::Suv => 0.25,
                _ => 0.15,
            },
            lattice_mass_kg,
            car_mass_kg: lattice_mass_kg * 4.0,
            front_overhang_m: None,
            strength_scale: 1.0,
        }
    }
}

/// Greenhouse and structure description of a profile. `*_u` are fractions of body length
/// measured from the rear bumper face (0) to the front bumper face (1).
#[derive(Debug, Clone, Copy)]
struct Shape {
    belt_frac: f32,
    roof_w_frac: f32,
    ws_base_u: f32,
    roof_front_u: f32,
    roof_rear_u: f32,
    deck_u: f32,
    four_doors: bool,
    open_bed: bool,
    rail_scale: f32,
    pushbar: bool,
}

fn shape(p: BodyProfile) -> Shape {
    let sedan = Shape {
        belt_frac: 0.60,
        roof_w_frac: 0.78,
        ws_base_u: 0.665,
        roof_front_u: 0.555,
        roof_rear_u: 0.335,
        deck_u: 0.235,
        four_doors: true,
        open_bed: false,
        rail_scale: 1.0,
        pushbar: false,
    };
    match p {
        BodyProfile::Sedan => sedan,
        BodyProfile::Coupe => Shape {
            belt_frac: 0.58,
            roof_w_frac: 0.74,
            ws_base_u: 0.655,
            roof_front_u: 0.53,
            roof_rear_u: 0.37,
            deck_u: 0.215,
            four_doors: false,
            ..sedan
        },
        BodyProfile::Hatch => Shape {
            belt_frac: 0.58,
            roof_w_frac: 0.80,
            ws_base_u: 0.68,
            roof_front_u: 0.565,
            roof_rear_u: 0.11,
            deck_u: 0.04,
            rail_scale: 0.9,
            ..sedan
        },
        BodyProfile::Truck => Shape {
            belt_frac: 0.62,
            roof_w_frac: 0.82,
            ws_base_u: 0.71,
            roof_front_u: 0.63,
            roof_rear_u: 0.45,
            deck_u: 0.445,
            four_doors: false,
            open_bed: true,
            rail_scale: 1.35,
            ..sedan
        },
        BodyProfile::Suv => Shape {
            belt_frac: 0.60,
            roof_w_frac: 0.84,
            ws_base_u: 0.70,
            roof_front_u: 0.60,
            roof_rear_u: 0.06,
            deck_u: 0.03,
            rail_scale: 1.2,
            ..sedan
        },
        BodyProfile::Cruiser => Shape {
            rail_scale: 1.3,
            pushbar: true,
            ..sedan
        },
    }
}

/// Beam sections, m², for a 1400 kg car (scaled by car mass and `strength_scale`).
/// These are the main crash-calibration knobs; see the README.
#[derive(Debug, Clone, Copy)]
pub struct Sections {
    /// Bumper cover (polypropylene) within the bumper.
    pub cover: f32,
    /// Bumper cover attachment to the body.
    pub cover_attach: f32,
    /// Glass.
    pub glass: f32,
    /// Body panel (mild steel / aluminium hood).
    pub panel: f32,
    /// Panel ↔ structure attachment.
    pub panel_attach: f32,
    /// Left-right attachments of the door skins and rear quarter panels to the body (cabin
    /// cage, frame): what holds the side skin against a nose in a T-bone. Fore-aft door
    /// attachments (they carry frontal load into the body side) and all front-fender
    /// attachments (they brace the front rails) stay `panel_attach`.
    pub side_attach: f32,
    /// Generic frame member outside the cabin (subframe, diagonals, crossmembers).
    pub structural: f32,
    /// Passenger-cell member running fore-aft or vertically (survival cell, stronger than
    /// the rails so crush stays in the crumple zones).
    pub cabin: f32,
    /// Passenger-cell cross-car member (floor/seat crossmembers; sets side-impact crush).
    pub cross_member: f32,
    /// Left-right frame member ahead of / behind the cabin (subframe crossmembers).
    pub frame_cross: f32,
    /// Left-right member between the two inner (rail) columns ahead of / behind the cabin.
    pub rail_brace: f32,
    /// Front/rear rail sacrificial segment (crash can).
    pub rail_soft: f32,
    /// Front/rear rail main segments.
    pub rail_hard: f32,
    /// Bumper reinforcement beam.
    pub bumper_beam: f32,
    /// Sills.
    pub sill: f32,
    /// A/B/C pillars.
    pub pillar: f32,
    /// Roof rails.
    pub roof_rail: f32,
    /// Door intrusion beam.
    pub door_beam: f32,
    /// Engine block (rigid, unbreakable).
    pub engine_block: f32,
    /// Engine mounts to the rails.
    pub engine_mount: f32,
    /// Engine ↔ firewall load path.
    pub engine_firewall: f32,
    /// Wheel knuckle / strut (cast, brittle).
    pub knuckle: f32,
}

/// Calibrated default sections (see `data/modes/sim.toml` calibration notes).
pub const DEFAULT_SECTIONS: Sections = Sections {
    cover: 1.2e-4,
    cover_attach: 0.8e-4,
    glass: 0.6e-4,
    panel: 0.25e-4,
    panel_attach: 0.25e-4,
    side_attach: 0.05e-4,
    structural: 0.25e-4,
    cabin: 3.0e-4,
    cross_member: 0.06e-4,
    frame_cross: 0.25e-4,
    rail_brace: 1.0e-4,
    rail_soft: 0.5e-4,
    rail_hard: 0.9e-4,
    bumper_beam: 2.0e-4,
    sill: 3.0e-4,
    pillar: 2.0e-4,
    roof_rail: 1.5e-4,
    door_beam: 1.5e-4,
    engine_block: 20.0e-4,
    engine_mount: 0.6e-4,
    engine_firewall: 1.2e-4,
    knuckle: 1.5e-4,
};

/// Bumper cover depth (face station → bumper beam station), m.
pub const BUMPER_DEPTH_M: f32 = 0.18;
/// Length of the soft rail segment behind the bumper beam, m.
pub const SOFT_RAIL_LEN_M: f32 = 0.30;
/// Maximum distance between stations, m.
pub const MAX_STATION_SPACING_M: f32 = 0.36;
/// Key stations closer than this to an earlier key are dropped, m.
pub const MIN_STATION_GAP_M: f32 = 0.10;
/// Rear rails and rear subframe members are this much stronger than the front ones: the
/// front has the engine block as an extra load path, the rear does not.
pub const REAR_RAIL_FACTOR: f32 = 1.6;
/// Rail inboard position as a fraction of body width.
pub const RAIL_X_FRAC: f32 = 0.30;

/// Group ids of the generated detachables.
pub const GROUP_FRONT_BUMPER: u16 = 0;
/// Rear bumper group id.
pub const GROUP_REAR_BUMPER: u16 = 1;
/// Hood group id.
pub const GROUP_HOOD: u16 = 2;
/// Trunk / tailgate group id.
pub const GROUP_TRUNK: u16 = 3;
/// Left front door group id.
pub const GROUP_LEFT_DOOR_F: u16 = 4;
/// Right front door group id.
pub const GROUP_RIGHT_DOOR_F: u16 = 5;
/// Left rear door group id (four-door bodies only).
pub const GROUP_LEFT_DOOR_R: u16 = 6;
/// Right rear door group id (four-door bodies only).
pub const GROUP_RIGHT_DOOR_R: u16 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Zone {
    RearBumper,
    RearCrumple,
    Rear,
    Cabin,
    EngineBay,
    FrontCrumple,
    FrontBumper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Top {
    Windshield,
    Roof,
    RearGlass,
}

#[derive(Debug, Clone)]
struct Station {
    y: f32,
    zone: Zone,
    top: Option<(f32, Top)>,
    belt_z: f32,
    trunk: bool,
    first_cabin: bool,
    last_cabin: bool,
}

#[derive(Debug, Clone, Copy)]
struct Info {
    station: usize,
    layer: u8,
    col: u8,
    region: BodyRegion,
    engine: bool,
    hub: bool,
}

const FLOOR: u8 = 0;
const MID: u8 = 1;
const BELT: u8 = 2;
const TOP: u8 = 3;

fn is_structural(r: BodyRegion) -> bool {
    matches!(
        r,
        BodyRegion::FrameFront
            | BodyRegion::FrameRear
            | BodyRegion::FrameLeft
            | BodyRegion::FrameRight
            | BodyRegion::Cabin
    )
}

fn is_door(r: BodyRegion) -> bool {
    matches!(
        r,
        BodyRegion::LeftDoorF
            | BodyRegion::LeftDoorR
            | BodyRegion::RightDoorF
            | BodyRegion::RightDoorR
    )
}

/// Rear quarter panels. (Front fenders are excluded on purpose: their left-right attachments
/// brace the upper front rails against sideways folding in a frontal crush.)
fn is_side_skin(r: BodyRegion) -> bool {
    matches!(
        r,
        BodyRegion::FrontLeftFender
            | BodyRegion::FrontRightFender
            | BodyRegion::RearLeftQuarter
            | BodyRegion::RearRightQuarter
    )
}

fn group_of(r: BodyRegion, four_doors: bool) -> Option<u16> {
    match r {
        BodyRegion::FrontBumper => Some(GROUP_FRONT_BUMPER),
        BodyRegion::RearBumper => Some(GROUP_REAR_BUMPER),
        BodyRegion::Hood => Some(GROUP_HOOD),
        BodyRegion::Trunk => Some(GROUP_TRUNK),
        BodyRegion::LeftDoorF => Some(GROUP_LEFT_DOOR_F),
        BodyRegion::RightDoorF => Some(GROUP_RIGHT_DOOR_F),
        BodyRegion::LeftDoorR if four_doors => Some(GROUP_LEFT_DOOR_R),
        BodyRegion::RightDoorR if four_doors => Some(GROUP_RIGHT_DOOR_R),
        _ => None,
    }
}

/// Generate a lattice. Deterministic: same params → identical output.
pub fn generate_lattice(p: &LatticeParams) -> LatticeDef {
    let sh = shape(p.profile);
    let (w, l, hgt) = (p.width_m, p.length_m, p.height_m);
    let clearance = p.ground_clearance_m;
    let fo = p.front_overhang_m.unwrap_or((l - p.wheelbase_m) * 0.5);
    let y_front = p.wheelbase_m * 0.5 + fo;
    let y_rear = y_front - l;
    let y_at = |u: f32| y_rear + u * l;
    let y_fa = p.wheelbase_m * 0.5;
    let y_ra = -y_fa;
    let y_fw = y_at(sh.ws_base_u);
    let cabin_rear = (y_ra + 0.3).max(y_at(sh.roof_rear_u) - 0.2).min(y_fw - 0.8);
    let y_b = if sh.four_doors {
        0.5 * (y_fw + cabin_rear)
    } else {
        cabin_rear + 0.3 * (y_fw - cabin_rear)
    };

    // ---- stations -------------------------------------------------------------------
    let keys = [
        y_front,
        y_rear,
        y_front - BUMPER_DEPTH_M,
        y_rear + BUMPER_DEPTH_M,
        y_fa,
        y_ra,
        y_fw,
        cabin_rear,
        y_b,
        y_at(sh.roof_front_u),
        y_at(sh.roof_rear_u),
        y_at(sh.deck_u),
        y_at(0.5 * (sh.ws_base_u + sh.roof_front_u)),
        y_at(0.5 * (sh.roof_rear_u + sh.deck_u)),
    ];
    let mut ys: Vec<f32> = Vec::new();
    for k in keys {
        if k >= y_rear - 1e-4
            && k <= y_front + 1e-4
            && ys.iter().all(|y| (y - k).abs() > MIN_STATION_GAP_M)
        {
            ys.push(k);
        }
    }
    ys.sort_by(f32::total_cmp);
    let mut filled = Vec::new();
    for win in ys.windows(2) {
        let (a, b) = (win[0], win[1]);
        filled.push(a);
        let n = ((b - a) / MAX_STATION_SPACING_M).ceil() as usize;
        for k in 1..n {
            filled.push(a + (b - a) * k as f32 / n as f32);
        }
    }
    filled.push(*ys.last().expect("stations"));
    let ys = filled;
    let ns = ys.len();

    let belt = sh.belt_frac * hgt;
    let mid_z = clearance + 0.45 * (belt - clearance);
    let mut stations: Vec<Station> = Vec::with_capacity(ns);
    for (s, &y) in ys.iter().enumerate() {
        let u = (y - y_rear) / l;
        let zone = if s == ns - 1 {
            Zone::FrontBumper
        } else if s == 0 {
            Zone::RearBumper
        } else if y > y_fw + 0.01 {
            if y >= y_fa + 0.05 {
                Zone::FrontCrumple
            } else {
                Zone::EngineBay
            }
        } else if y >= cabin_rear - 0.01 {
            Zone::Cabin
        } else if y > y_ra - 0.05 {
            Zone::Rear
        } else {
            Zone::RearCrumple
        };
        let bumper = matches!(zone, Zone::FrontBumper | Zone::RearBumper);
        let top = if !bumper && u > sh.deck_u + 1e-3 && u < sh.ws_base_u - 1e-3 {
            if u > sh.roof_front_u {
                let t = (sh.ws_base_u - u) / (sh.ws_base_u - sh.roof_front_u);
                Some((belt + (hgt - belt) * t, Top::Windshield))
            } else if u >= sh.roof_rear_u {
                Some((hgt, Top::Roof))
            } else if sh.open_bed {
                None
            } else {
                let t = (u - sh.deck_u) / (sh.roof_rear_u - sh.deck_u);
                Some((belt + (hgt - belt) * t, Top::RearGlass))
            }
        } else {
            None
        };
        let belt_z = belt
            * match zone {
                Zone::FrontBumper => 0.75,
                Zone::FrontCrumple => 0.92,
                Zone::RearBumper => 0.8,
                Zone::RearCrumple => 0.95,
                _ => 1.0,
            };
        let rear_zone = matches!(zone, Zone::Rear | Zone::RearCrumple);
        stations.push(Station {
            y,
            zone,
            top,
            belt_z,
            trunk: rear_zone && (top.is_none() || s == 1),
            first_cabin: false,
            last_cabin: false,
        });
    }
    let cabin_idx: Vec<usize> = (0..ns)
        .filter(|&s| stations[s].zone == Zone::Cabin)
        .collect();
    // "First" cabin station = the firewall (highest y), "last" = rear seat bulkhead.
    if let (Some(&firewall), Some(&bulkhead)) = (cabin_idx.last(), cabin_idx.first()) {
        stations[firewall].first_cabin = true;
        stations[bulkhead].last_cabin = true;
    }

    // ---- nodes ------------------------------------------------------------------------
    let present = |st: &Station, layer: u8, s: usize| -> Vec<u8> {
        let bumper = matches!(st.zone, Zone::FrontBumper | Zone::RearBumper);
        match layer {
            FLOOR => vec![0, 1, 3, 4],
            MID => {
                // Bumper covers and the bumper reinforcement beams behind them span the
                // full width, so they get a centre node.
                if bumper || s == 1 || s + 2 == ns {
                    vec![0, 1, 2, 3, 4]
                } else {
                    vec![0, 1, 3, 4]
                }
            }
            BELT => match st.zone {
                Zone::Cabin => vec![0, 1, 3, 4],
                Zone::Rear | Zone::RearCrumple if sh.open_bed && s != 1 => vec![0, 4],
                _ => vec![0, 1, 2, 3, 4],
            },
            _ => {
                if st.top.is_some() {
                    vec![0, 2, 4]
                } else {
                    vec![]
                }
            }
        }
    };
    let front_axle_s = nearest(&ys, y_fa);
    let rear_axle_s = nearest(&ys, y_ra);
    let region_for = |st: &Station, layer: u8, col: u8| -> BodyRegion {
        use BodyRegion::*;
        let left = col == 0;
        let outer = col == 0 || col == 4;
        let top_region = |c: u8| match st.top.map(|t| t.1) {
            Some(Top::Roof) => Roof,
            Some(Top::Windshield) if c == 2 => Windshield,
            Some(Top::RearGlass) if c == 2 => RearGlass,
            _ => Cabin,
        };
        match st.zone {
            Zone::FrontBumper => FrontBumper,
            Zone::RearBumper => RearBumper,
            Zone::FrontCrumple | Zone::EngineBay => match layer {
                FLOOR => FrameFront,
                MID | BELT if outer => {
                    if left {
                        FrontLeftFender
                    } else {
                        FrontRightFender
                    }
                }
                MID => FrameFront,
                BELT => Hood,
                _ => top_region(col),
            },
            Zone::Cabin => match layer {
                FLOOR if outer => {
                    if left {
                        FrameLeft
                    } else {
                        FrameRight
                    }
                }
                FLOOR => Cabin,
                MID | BELT if outer => {
                    if st.y > y_b + 0.01 && st.y < y_fw - 0.01 {
                        if left {
                            LeftDoorF
                        } else {
                            RightDoorF
                        }
                    } else if st.y < y_b - 0.01 && st.y > cabin_rear + 0.01 {
                        if left {
                            LeftDoorR
                        } else {
                            RightDoorR
                        }
                    } else {
                        Cabin
                    }
                }
                MID | BELT => Cabin,
                _ => top_region(col),
            },
            Zone::Rear | Zone::RearCrumple => match layer {
                FLOOR => FrameRear,
                MID | BELT if outer => {
                    if left {
                        RearLeftQuarter
                    } else {
                        RearRightQuarter
                    }
                }
                MID => FrameRear,
                BELT if st.trunk => Trunk,
                BELT => Cabin,
                _ => top_region(col),
            },
        }
    };

    let mut infos: Vec<Info> = Vec::new();
    let mut nodes: Vec<LatticeNode> = Vec::new();
    let mut slot: BTreeMap<(usize, u8, u8), u16> = BTreeMap::new();
    let mut weights: Vec<f32> = Vec::new();
    for (s, st) in stations.iter().enumerate() {
        let bumper = matches!(st.zone, Zone::FrontBumper | Zone::RearBumper);
        let outer_x = 0.5 * w * if bumper { 0.92 } else { 1.0 };
        let rail_x = RAIL_X_FRAC * w;
        let roof_x = 0.5 * sh.roof_w_frac * w;
        for layer in [FLOOR, MID, BELT, TOP] {
            for col in present(st, layer, s) {
                let x = if layer == TOP {
                    [-roof_x, 0.0, 0.0, 0.0, roof_x][col as usize]
                } else {
                    [-outer_x, -rail_x, 0.0, rail_x, outer_x][col as usize]
                };
                let z = match layer {
                    FLOOR => clearance,
                    MID => mid_z,
                    BELT => st.belt_z,
                    _ => st.top.map(|t| t.0).unwrap_or(hgt),
                };
                let region = region_for(st, layer, col);
                let pinned = st.zone == Zone::Cabin
                    && layer == FLOOR
                    && (col == 1 || col == 3)
                    && !st.first_cabin
                    && !st.last_cabin;
                let hub = layer == FLOOR
                    && (col == 0 || col == 4)
                    && (s == front_axle_s || s == rear_axle_s);
                let id = nodes.len() as u16;
                slot.insert((s, layer, col), id);
                nodes.push(LatticeNode {
                    id,
                    pos_m: [x, st.y, z],
                    mass_kg: 1.0,
                    pinned_to_chassis: pinned,
                    region,
                });
                infos.push(Info {
                    station: s,
                    layer,
                    col,
                    region,
                    engine: false,
                    hub,
                });
                weights.push(1.0);
            }
        }
    }

    // Engine block: 8 nodes between the front rails.
    let y_ef = y_front - (0.45 * fo).max(0.35);
    let y_er = (y_ef - 0.65).max(y_fw + 0.12);
    let mut engine_ids = Vec::new();
    for &y in &[y_er, y_ef] {
        for &z in &[clearance + 0.12, mid_z + 0.15] {
            for &x in &[-0.22 * w, 0.22 * w] {
                let id = nodes.len() as u16;
                nodes.push(LatticeNode {
                    id,
                    pos_m: [x, y, z],
                    mass_kg: 1.0,
                    pinned_to_chassis: false,
                    region: BodyRegion::FrameFront,
                });
                infos.push(Info {
                    station: nearest(&ys, y),
                    layer: if z < mid_z { FLOOR } else { MID },
                    col: if x < 0.0 { 1 } else { 3 },
                    region: BodyRegion::FrameFront,
                    engine: true,
                    hub: false,
                });
                weights.push(4.0);
                engine_ids.push(id);
            }
        }
    }
    let wsum: f32 = weights.iter().sum();
    for (n, wt) in nodes.iter_mut().zip(&weights) {
        n.mass_kg = p.lattice_mass_kg * wt / wsum;
    }

    // ---- beams ------------------------------------------------------------------------
    let mut pairs: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut add = |a: u16, b: u16| {
        if a != b {
            pairs.insert((a.min(b), a.max(b)));
        }
    };
    let has = |s: usize, layer: u8, col: u8| slot.contains_key(&(s, layer, col));
    for s in 0..ns {
        let st = &stations[s];
        for layer in [FLOOR, MID, BELT, TOP] {
            let cols = present(st, layer, s);
            for (k, &c) in cols.iter().enumerate() {
                let a = slot[&(s, layer, c)];
                // Same station, same layer: next present column.
                if let Some(&c2) = cols.get(k + 1) {
                    add(a, slot[&(s, layer, c2)]);
                }
                // Same station, layer above: verticals and in-plane diagonals.
                if layer < TOP {
                    for c2 in present(st, layer + 1, s) {
                        let dc = (c as i32 - c2 as i32).abs();
                        let mid = (c + c2) / 2;
                        if dc <= 1 || (dc == 2 && (!has(s, layer, mid) || !has(s, layer + 1, mid)))
                        {
                            add(a, slot[&(s, layer + 1, c2)]);
                        }
                    }
                }
                // Next station.
                if s + 1 < ns {
                    let nx = &stations[s + 1];
                    for c2 in present(nx, layer, s + 1) {
                        let dc = (c as i32 - c2 as i32).abs();
                        let mid = (c + c2) / 2;
                        if dc <= 1 || (dc == 2 && (!has(s, layer, mid) || !has(s + 1, layer, mid)))
                        {
                            add(a, slot[&(s + 1, layer, c2)]);
                        }
                    }
                    for l2 in [layer.wrapping_sub(1), layer + 1] {
                        if l2 > TOP {
                            continue;
                        }
                        for c2 in present(nx, l2, s + 1) {
                            let dc = (c as i32 - c2 as i32).abs();
                            if dc == 0 || (dc == 1 && (layer == TOP || l2 == TOP)) {
                                add(a, slot[&(s + 1, l2, c2)]);
                            }
                        }
                    }
                }
            }
        }
    }
    // Engine block, mounts and firewall load path.
    let firewall_s = (0..ns).find(|&s| stations[s].first_cabin);
    for (i, &a) in engine_ids.iter().enumerate() {
        for &b in &engine_ids[i + 1..] {
            add(a, b);
        }
        let ai = infos[a as usize];
        let side = ai.col;
        let rail_layer = ai.layer;
        let ya = nodes[a as usize].pos_m[1];
        // Two nearest stations by y.
        let mut st_by_dist: Vec<usize> = (1..ns - 1).collect();
        st_by_dist.sort_by(|&x, &y| {
            (stations[x].y - ya)
                .abs()
                .total_cmp(&(stations[y].y - ya).abs())
        });
        for &s in st_by_dist.iter().take(2) {
            if let Some(&b) = slot.get(&(s, rail_layer, side)) {
                add(a, b);
            }
        }
        if (ya - y_er).abs() < 1e-4 {
            if let Some(fs) = firewall_s {
                for l2 in [FLOOR, MID] {
                    if let Some(&b) = slot.get(&(fs, l2, side)) {
                        add(a, b);
                    }
                }
            }
        }
    }

    let mass_scale = (p.car_mass_kg / 1400.0).max(0.2) * p.strength_scale;
    let sec = DEFAULT_SECTIONS;
    let bumper_beam_s = [1usize, ns - 2];
    let soft_front_min_y = y_front - BUMPER_DEPTH_M - SOFT_RAIL_LEN_M - 1e-3;
    let soft_rear_max_y = y_rear + BUMPER_DEPTH_M + SOFT_RAIL_LEN_M + 1e-3;
    let mut beams = Vec::with_capacity(pairs.len());
    for (a, b) in pairs {
        let (ia, ib) = (infos[a as usize], infos[b as usize]);
        let (ra, rb) = (ia.region, ib.region);
        let (ya, yb) = (nodes[a as usize].pos_m[1], nodes[b as usize].pos_m[1]);
        let either = |f: &dyn Fn(BodyRegion) -> bool| f(ra) || f(rb);
        let both = |f: &dyn Fn(BodyRegion) -> bool| f(ra) && f(rb);
        // A beam that runs mostly left-right (carries side load).
        let lateral = |a: u16, b: u16| {
            let (pa, pb) = (nodes[a as usize].pos_m, nodes[b as usize].pos_m);
            (pa[0] - pb[0]).abs() > (pa[1] - pb[1]).abs().max((pa[2] - pb[2]).abs())
        };
        let same_station = ia.station == ib.station;
        let (material, area, breakable): (&str, f32, bool) = if ia.engine && ib.engine {
            ("hss", sec.engine_block * mass_scale, false)
        } else if ia.engine || ib.engine {
            let other = if ia.engine { ib } else { ia };
            if stations[other.station].zone == Zone::Cabin {
                ("hss", sec.engine_firewall * mass_scale, true)
            } else {
                ("hss", sec.engine_mount * mass_scale, true)
            }
        } else if either(&|r| matches!(r, BodyRegion::Windshield | BodyRegion::RearGlass)) {
            ("glass", sec.glass, true)
        } else if both(&|r| matches!(r, BodyRegion::FrontBumper | BodyRegion::RearBumper)) {
            ("pp_bumper", sec.cover, true)
        } else if either(&|r| matches!(r, BodyRegion::FrontBumper | BodyRegion::RearBumper)) {
            ("pp_bumper", sec.cover_attach, true)
        } else if (ia.hub || ib.hub) && same_station {
            ("cast_aluminum", sec.knuckle * mass_scale, true)
        } else if !same_station
            && ia.col == ib.col
            && ia.layer == ib.layer
            && (ia.col == 1 || ia.col == 3)
            && ia.layer <= MID
            && (both(&|r| r == BodyRegion::FrameFront) || both(&|r| r == BodyRegion::FrameRear))
        {
            let soft = ya.min(yb) >= soft_front_min_y || ya.max(yb) <= soft_rear_max_y;
            let s = if soft { sec.rail_soft } else { sec.rail_hard };
            let rear = if ra == BodyRegion::FrameRear {
                REAR_RAIL_FACTOR
            } else {
                1.0
            };
            ("hss", s * rear * sh.rail_scale * mass_scale, true)
        } else if same_station
            && bumper_beam_s.contains(&ia.station)
            && ia.layer == MID
            && ib.layer == MID
            && ia.col != 0
            && ia.col != 4
            && ib.col != 0
            && ib.col != 4
        {
            let push = if sh.pushbar && ia.station == ns - 2 {
                2.0
            } else {
                1.0
            };
            ("hss", sec.bumper_beam * push * mass_scale, true)
        } else if both(&|r| r == BodyRegion::FrameLeft) || both(&|r| r == BodyRegion::FrameRight) {
            ("hss", sec.sill * mass_scale, true)
        } else if !same_station
            && ia.layer == TOP
            && ib.layer == TOP
            && ia.col == ib.col
            && ia.col != 2
        {
            ("hss", sec.roof_rail * mass_scale, true)
        } else if same_station
            && ia.col == ib.col
            && (ia.col == 0 || ia.col == 4)
            && ia.layer.abs_diff(ib.layer) == 1
            && ia.layer.max(ib.layer) >= BELT
            && both(&|r| is_structural(r) || r == BodyRegion::Roof)
        {
            ("hss", sec.pillar * mass_scale, true)
        } else if !same_station
            && ia.layer == MID
            && ib.layer == MID
            && ia.col == ib.col
            && either(&is_door)
        {
            ("hss", sec.door_beam * mass_scale, true)
        } else if both(&is_structural) {
            let (pa, pb) = (nodes[a as usize].pos_m, nodes[b as usize].pos_m);
            let (dx, _dy, dz) = (
                (pa[0] - pb[0]).abs(),
                (pa[1] - pb[1]).abs(),
                (pa[2] - pb[2]).abs(),
            );
            let cabin_zone = stations[ia.station].zone == Zone::Cabin
                && stations[ib.station].zone == Zone::Cabin;
            // Any cabin member that spans columns carries side load (cross-car); only members
            // within one column form the fore-aft / vertical survival cell.
            if cabin_zone && dx > 0.05 {
                ("hss", sec.cross_member * mass_scale, true)
            } else if cabin_zone {
                ("hss", sec.cabin * mass_scale, true)
            } else if lateral(a, b) && ia.col != 0 && ia.col != 4 && ib.col != 0 && ib.col != 4 {
                // Rail-to-rail bracing between the inner columns: keeps the front/rear rails
                // from folding sideways in a frontal, out of reach of a side impact.
                ("hss", sec.rail_brace * mass_scale, true)
            } else if same_station && dx > dz {
                // Left-right members of the engine bay / rear floor (subframe crossmembers):
                // they carry side load and barely any fore-aft crush load.
                ("hss", sec.frame_cross * mass_scale, true)
            } else if both(&|r| r == BodyRegion::FrameRear) {
                ("hss", sec.structural * REAR_RAIL_FACTOR * mass_scale, true)
            } else {
                ("hss", sec.structural * mass_scale, true)
            }
        } else if either(&|r| r == BodyRegion::Hood) {
            let s = if both(&|r| r == BodyRegion::Hood) {
                sec.panel
            } else {
                sec.panel_attach
            };
            ("aluminum", s * mass_scale.sqrt(), true)
        } else if !is_structural(ra) && !is_structural(rb) {
            ("mild_steel", sec.panel * mass_scale.sqrt(), true)
        } else if (either(&is_door) || either(&is_side_skin)) && lateral(a, b) {
            ("mild_steel", sec.side_attach * mass_scale.sqrt(), true)
        } else {
            ("mild_steel", sec.panel_attach * mass_scale.sqrt(), true)
        };
        let ga = group_of(ra, sh.four_doors);
        let gb = group_of(rb, sh.four_doors);
        let detach_group = match (ga, gb) {
            (Some(x), Some(y)) if x != y => Some(x.min(y)),
            (Some(x), None) | (None, Some(x)) => Some(x),
            _ => None,
        };
        beams.push(LatticeBeam {
            a,
            b,
            material_id: material.to_string(),
            area_m2: area,
            breakable,
            detach_group,
        });
    }

    // ---- detachables ------------------------------------------------------------------
    let regions_present: BTreeSet<BodyRegion> = nodes.iter().map(|n| n.region).collect();
    let mut detachables = Vec::new();
    let find = |s: usize, layer: u8, col: u8| slot.get(&(s, layer, col)).copied();
    let stations_of = |r: BodyRegion| -> Vec<usize> {
        let mut v: Vec<usize> = infos
            .iter()
            .filter(|i| i.region == r && !i.engine)
            .map(|i| i.station)
            .collect();
        v.sort();
        v.dedup();
        v
    };
    let ms = mass_scale.sqrt();
    let mut push_det =
        |group: u16, region: BodyRegion, hinge: Option<[u16; 2]>, impulse: f32, mass: f32| {
            if let Some(hinge_nodes) = hinge {
                if regions_present.contains(&region) {
                    detachables.push(Detachable {
                        group,
                        region,
                        hinge_nodes,
                        break_impulse_ns: impulse * mass_scale,
                        mass_kg: mass * ms,
                    });
                }
            }
        };
    let pair = |a: Option<u16>, b: Option<u16>| a.zip(b).map(|(a, b)| [a, b]);
    push_det(
        GROUP_FRONT_BUMPER,
        BodyRegion::FrontBumper,
        pair(find(ns - 1, MID, 0), find(ns - 1, MID, 4)),
        3000.0,
        5.0,
    );
    push_det(
        GROUP_REAR_BUMPER,
        BodyRegion::RearBumper,
        pair(find(0, MID, 0), find(0, MID, 4)),
        3000.0,
        5.0,
    );
    if let Some(&hs) = stations_of(BodyRegion::Hood).first() {
        push_det(
            GROUP_HOOD,
            BodyRegion::Hood,
            pair(find(hs, BELT, 1), find(hs, BELT, 3)),
            5000.0,
            16.0,
        );
    }
    if let Some(&ts) = stations_of(BodyRegion::Trunk).last() {
        push_det(
            GROUP_TRUNK,
            BodyRegion::Trunk,
            pair(find(ts, BELT, 1), find(ts, BELT, 3)),
            4500.0,
            14.0,
        );
    }
    for (group, region, col) in [
        (GROUP_LEFT_DOOR_F, BodyRegion::LeftDoorF, 0u8),
        (GROUP_RIGHT_DOOR_F, BodyRegion::RightDoorF, 4),
        (GROUP_LEFT_DOOR_R, BodyRegion::LeftDoorR, 0),
        (GROUP_RIGHT_DOOR_R, BodyRegion::RightDoorR, 4),
    ] {
        if group_of(region, sh.four_doors).is_none() {
            continue;
        }
        if let Some(&ds) = stations_of(region).last() {
            push_det(
                group,
                region,
                pair(find(ds, MID, col), find(ds, BELT, col)),
                8000.0,
                22.0,
            );
        }
    }

    // ---- components -------------------------------------------------------------------
    let mut components = Vec::new();
    let mut mount = |kind: Component, ids: Vec<Option<u16>>, fail: f32, total: f32| {
        let node_ids: Vec<u16> = ids.into_iter().flatten().collect();
        if !node_ids.is_empty() {
            components.push(ComponentMount {
                kind,
                node_ids,
                intrusion_fail_m: fail,
                intrusion_total_m: total,
            });
        }
    };
    let front_inner = |s: usize, layer: u8| vec![find(s, layer, 1), find(s, layer, 3)];
    let radiator_s = nearest_in(&ys, y_front - 0.35, 2, ns - 3);
    let battery_s = nearest_in(&ys, 0.5 * (y_fa + y_front - 0.30), 1, ns - 2);
    let drive_s = nearest_in(&ys, 0.5 * (y_fa + y_fw), 1, ns - 2);
    let tank_s = nearest_in(&ys, y_ra + 0.35, 1, ns - 2);
    let exhaust_s = nearest_in(&ys, y_ra - 0.40, 1, ns - 2);
    let fw = firewall_s.unwrap_or(drive_s);
    mount(
        Component::Engine,
        engine_ids.iter().map(|&i| Some(i)).collect(),
        0.25,
        0.45,
    );
    mount(
        Component::Radiator,
        front_inner(radiator_s, MID),
        0.08,
        0.18,
    );
    mount(
        Component::Battery,
        vec![find(battery_s, MID, 1)],
        0.12,
        0.25,
    );
    mount(Component::FuelTank, front_inner(tank_s, FLOOR), 0.12, 0.25);
    mount(
        Component::SteeringRack,
        front_inner(front_axle_s, FLOOR),
        0.06,
        0.15,
    );
    mount(Component::Gearbox, front_inner(fw, FLOOR), 0.12, 0.25);
    mount(
        Component::Driveshaft,
        front_inner(drive_s, FLOOR),
        0.10,
        0.22,
    );
    mount(
        Component::Exhaust,
        vec![find(exhaust_s, FLOOR, 3)],
        0.10,
        0.25,
    );
    mount(
        Component::Lights,
        vec![
            find(ns - 1, MID, 0),
            find(ns - 1, MID, 4),
            find(0, MID, 0),
            find(0, MID, 4),
        ],
        0.04,
        0.10,
    );
    for (s, col, susp, wheel, brake) in [
        (
            front_axle_s,
            0u8,
            Component::SuspensionFL,
            Component::WheelFL,
            Component::BrakeFL,
        ),
        (
            front_axle_s,
            4,
            Component::SuspensionFR,
            Component::WheelFR,
            Component::BrakeFR,
        ),
        (
            rear_axle_s,
            0,
            Component::SuspensionRL,
            Component::WheelRL,
            Component::BrakeRL,
        ),
        (
            rear_axle_s,
            4,
            Component::SuspensionRR,
            Component::WheelRR,
            Component::BrakeRR,
        ),
    ] {
        mount(
            susp,
            vec![find(s, FLOOR, col), find(s, MID, col)],
            0.09,
            0.18,
        );
        mount(wheel, vec![find(s, FLOOR, col)], 0.15, 0.25);
        mount(brake, vec![find(s, FLOOR, col)], 0.10, 0.20);
    }

    LatticeDef {
        id: p.id.clone(),
        nodes,
        beams,
        detachables,
        components,
        skin_binding: Vec::new(),
    }
}

/// Detach group of a bolt-on accessory (push bar, kit). Groups 0-7 are the generator's
/// bumpers, hood, trunk and doors.
pub const GROUP_ACCESSORY: u16 = 8;

/// Push-bar tube section, m² (high-strength steel; not scaled by car mass).
pub const PUSHBAR_SECTION_M2: f32 = 8.0e-4;
/// Push-bar mount section, m² (bolted to the front bumper-beam rail nodes).
pub const PUSHBAR_MOUNT_SECTION_M2: f32 = 6.0e-4;

/// Police push bar added by [`add_push_bar`] (`gen-lattice --accessory pushbar`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PushBarOptions {
    /// Push-bar mass, kg (added on top of the lattice mass, spread over its 4 nodes).
    pub mass_kg: f32,
    /// Hinge break impulse of the bar, N·s.
    pub break_impulse_ns: f32,
    /// Multiplier on the tube and mount sections.
    pub section_scale: f32,
    /// Region of the bar's nodes. `Accessory` (default) bills it as an accessory and keeps it
    /// out of the car's health; `FrontBumper` reproduces the pre-v0.3.0 roster data.
    pub region: BodyRegion,
    /// Heights of the lower and upper tube, m above the ground. `None` = 0.35 / 0.80 m,
    /// or 0.45 / 0.95 m on the truck and SUV profiles.
    pub heights_m: Option<[f32; 2]>,
}

impl Default for PushBarOptions {
    fn default() -> Self {
        Self {
            mass_kg: 25.0,
            break_impulse_ns: 15_000.0,
            section_scale: 1.0,
            region: BodyRegion::Accessory,
            heights_m: None,
        }
    }
}

/// Add a push bar: 4 nodes 0.10 m ahead of the frontmost node (x = ±0.32 × width), joined by
/// 6 tubes (never detaching on their own) and bolted to the frontmost front-rail nodes of the
/// same side by mounts in detach group [`GROUP_ACCESSORY`]. Beams are re-sorted by (a, b).
/// Same geometry as the `police_package` step of `tools/specsheet/build_roster.py`.
/// Returns the new node ids (lower left, lower right, upper left, upper right).
pub fn add_push_bar(l: &mut LatticeDef, p: &LatticeParams, o: &PushBarOptions) -> [u16; 4] {
    let rail_x = RAIL_X_FRAC * p.width_m;
    let engine: BTreeSet<u16> = l
        .components
        .iter()
        .filter(|c| c.kind == Component::Engine)
        .flat_map(|c| c.node_ids.iter().copied())
        .collect();
    let ff: Vec<&LatticeNode> = l
        .nodes
        .iter()
        .filter(|n| {
            n.region == BodyRegion::FrameFront
                && (n.pos_m[0].abs() - rail_x).abs() < 1e-3
                && !engine.contains(&n.id)
        })
        .collect();
    let y_bb = ff.iter().map(|n| n.pos_m[1]).fold(f32::MIN, f32::max);
    let mounts: Vec<(u16, f32)> = ff
        .iter()
        .filter(|n| (n.pos_m[1] - y_bb).abs() < 1e-3)
        .map(|n| (n.id, n.pos_m[0]))
        .collect();
    let y_bar = l.nodes.iter().map(|n| n.pos_m[1]).fold(f32::MIN, f32::max) + 0.10;
    let zs = o.heights_m.unwrap_or(match p.profile {
        BodyProfile::Truck | BodyProfile::Suv => [0.45, 0.95],
        _ => [0.35, 0.80],
    });
    let round4 = |v: f32| (v * 1e4).round() / 1e4;
    let mut new = [0u16; 4];
    let mut k = 0;
    for z in zs {
        for x in [-0.32 * p.width_m, 0.32 * p.width_m] {
            let id = l.nodes.len() as u16;
            l.nodes.push(LatticeNode {
                id,
                pos_m: [round4(x), round4(y_bar), z],
                mass_kg: o.mass_kg / 4.0,
                pinned_to_chassis: false,
                region: o.region,
            });
            new[k] = id;
            k += 1;
        }
    }
    let [lo_l, lo_r, hi_l, hi_r] = new;
    let tube = PUSHBAR_SECTION_M2 * o.section_scale;
    let mount = PUSHBAR_MOUNT_SECTION_M2 * o.section_scale;
    for (a, b) in [
        (lo_l, lo_r),
        (hi_l, hi_r),
        (lo_l, hi_l),
        (lo_r, hi_r),
        (lo_l, hi_r),
        (lo_r, hi_l),
    ] {
        l.beams.push(LatticeBeam {
            a: a.min(b),
            b: a.max(b),
            material_id: "hss".into(),
            area_m2: tube,
            breakable: true,
            detach_group: None,
        });
    }
    for id in new {
        let left = l.nodes[id as usize].pos_m[0] < 0.0;
        for &(m, mx) in &mounts {
            if (mx < 0.0) == left {
                l.beams.push(LatticeBeam {
                    a: m.min(id),
                    b: m.max(id),
                    material_id: "hss".into(),
                    area_m2: mount,
                    breakable: true,
                    detach_group: Some(GROUP_ACCESSORY),
                });
            }
        }
    }
    l.beams.sort_by_key(|b| (b.a, b.b));
    l.detachables.push(Detachable {
        group: GROUP_ACCESSORY,
        region: o.region,
        hinge_nodes: [lo_l, lo_r],
        break_impulse_ns: o.break_impulse_ns,
        mass_kg: o.mass_kg,
    });
    new
}

/// Heavy-duty components (`gen-lattice --hd Radiator=1.5,…`): each listed component's
/// failure and total intrusion × its factor (rounded to 0.1 mm, as the roster tool does).
pub fn apply_heavy_duty(l: &mut LatticeDef, factors: &BTreeMap<Component, f32>) {
    let r4 = |v: f32| (v * 1e4).round() / 1e4;
    for c in &mut l.components {
        if let Some(&k) = factors.get(&c.kind) {
            c.intrusion_fail_m = r4(c.intrusion_fail_m * k);
            c.intrusion_total_m = r4(c.intrusion_total_m * k);
        }
    }
}

/// Scale the detach impulse of every group of `region` except [`GROUP_ACCESSORY`]
/// (`gen-lattice --detach-scale FrontBumper=2`; rounded to 0.1 N·s).
pub fn scale_detach_impulse(l: &mut LatticeDef, region: BodyRegion, scale: f32) {
    for d in &mut l.detachables {
        if d.region == region && d.group != GROUP_ACCESSORY {
            d.break_impulse_ns = (d.break_impulse_ns * scale * 10.0).round() / 10.0;
        }
    }
}

fn nearest(ys: &[f32], y: f32) -> usize {
    nearest_in(ys, y, 0, ys.len() - 1)
}

fn nearest_in(ys: &[f32], y: f32, lo: usize, hi: usize) -> usize {
    let mut best = lo;
    for s in lo..=hi {
        if (ys[s] - y).abs() < (ys[best] - y).abs() {
            best = s;
        }
    }
    best
}

/// Serialize a lattice as JSON with one node/beam/etc. per line (diff-friendly, compact).
/// Parses back to an identical [`LatticeDef`].
pub fn lattice_to_json_lines(l: &LatticeDef) -> String {
    fn list<T: serde::Serialize>(out: &mut String, key: &str, items: &[T], last: bool) {
        out.push_str(&format!("  \"{key}\": ["));
        for (i, item) in items.iter().enumerate() {
            out.push_str(if i == 0 { "\n    " } else { ",\n    " });
            out.push_str(&serde_json::to_string(item).expect("serialize lattice item"));
        }
        out.push_str(if items.is_empty() { "]" } else { "\n  ]" });
        out.push_str(if last { "\n" } else { ",\n" });
    }
    let mut out = String::from("{\n");
    out.push_str(&format!(
        "  \"id\": {},\n",
        serde_json::to_string(&l.id).expect("serialize id")
    ));
    list(&mut out, "nodes", &l.nodes, false);
    list(&mut out, "beams", &l.beams, false);
    list(&mut out, "detachables", &l.detachables, false);
    list(&mut out, "components", &l.components, false);
    list(&mut out, "skin_binding", &l.skin_binding, true);
    out.push_str("}\n");
    out
}

/// Parameters of the checked-in debug lattices (`just lattices` regenerates them).
pub fn debug_lattice_params() -> Vec<LatticeParams> {
    let mut sedan = LatticeParams::new(
        "debug_sedan",
        BodyProfile::Sedan,
        [1.80, 4.70, 1.45],
        2.70,
        350.0,
    );
    sedan.car_mass_kg = 1400.0;
    let mut coupe = LatticeParams::new(
        "debug_coupe",
        BodyProfile::Coupe,
        [1.85, 4.50, 1.32],
        2.65,
        375.0,
    );
    coupe.car_mass_kg = 1500.0;
    vec![sedan, coupe]
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::MaterialDef;

    fn materials() -> BTreeMap<String, MaterialDef> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/materials");
        let mut m = BTreeMap::new();
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let def = wp_contracts::load_material_def(&p).unwrap();
            m.insert(def.id.clone(), def);
        }
        m
    }

    fn params(profile: BodyProfile) -> LatticeParams {
        let mut p = LatticeParams::new("test", profile, [1.8, 4.7, 1.45], 2.7, 350.0);
        p.car_mass_kg = 1400.0;
        p
    }

    #[test]
    fn every_profile_is_valid_and_in_size_range() {
        let mats = materials();
        for profile in BodyProfile::ALL {
            let l = generate_lattice(&params(profile));
            wp_contracts::validate_lattice_def(&l).unwrap_or_else(|e| panic!("{profile:?}: {e}"));
            assert!(
                (200..=350).contains(&l.nodes.len()),
                "{profile:?}: {} nodes",
                l.nodes.len()
            );
            for b in &l.beams {
                assert!(mats.contains_key(&b.material_id), "{}", b.material_id);
            }
            let regions: BTreeSet<BodyRegion> = l.nodes.iter().map(|n| n.region).collect();
            for r in [
                BodyRegion::FrontBumper,
                BodyRegion::RearBumper,
                BodyRegion::Hood,
                BodyRegion::Trunk,
                BodyRegion::Windshield,
                BodyRegion::FrameFront,
                BodyRegion::FrameRear,
                BodyRegion::FrameLeft,
                BodyRegion::FrameRight,
                BodyRegion::Cabin,
                BodyRegion::LeftDoorF,
                BodyRegion::RightDoorF,
            ] {
                assert!(regions.contains(&r), "{profile:?} lacks {r:?}");
            }
            assert!(l.nodes.iter().any(|n| n.pinned_to_chassis));
            let groups: BTreeSet<u16> = l.detachables.iter().map(|d| d.group).collect();
            for g in [
                GROUP_FRONT_BUMPER,
                GROUP_REAR_BUMPER,
                GROUP_HOOD,
                GROUP_TRUNK,
                GROUP_LEFT_DOOR_F,
            ] {
                assert!(groups.contains(&g), "{profile:?} lacks group {g}");
            }
            for d in &l.detachables {
                assert!(
                    l.beams.iter().any(|b| b.detach_group == Some(d.group)),
                    "{profile:?}: group {} has no attachment beams",
                    d.group
                );
            }
            let kinds: BTreeSet<Component> = l.components.iter().map(|c| c.kind).collect();
            assert_eq!(kinds.len(), 21, "{profile:?}: every component mounted");
            for m in [
                "glass",
                "pp_bumper",
                "hss",
                "mild_steel",
                "aluminum",
                "cast_aluminum",
            ] {
                assert!(
                    l.beams.iter().any(|b| b.material_id == m),
                    "{profile:?} lacks {m}"
                );
            }
        }
    }

    #[test]
    fn rails_have_soft_then_hard_segments() {
        let l = generate_lattice(&params(BodyProfile::Sedan));
        let areas: BTreeSet<u32> = l
            .beams
            .iter()
            .filter(|b| {
                let (na, nb) = (&l.nodes[b.a as usize], &l.nodes[b.b as usize]);
                na.region == BodyRegion::FrameFront
                    && nb.region == BodyRegion::FrameFront
                    && b.material_id == "hss"
                    && (na.pos_m[0] - nb.pos_m[0]).abs() < 1e-4
                    && (na.pos_m[2] - nb.pos_m[2]).abs() < 1e-4
                    && na.pos_m[0].abs() > 0.3
                    && na.pos_m[0].abs() < 0.6
            })
            .map(|b| (b.area_m2 * 1e7) as u32)
            .collect();
        assert!(areas.contains(&((DEFAULT_SECTIONS.rail_soft * 1e7) as u32)));
        assert!(areas.contains(&((DEFAULT_SECTIONS.rail_hard * 1e7) as u32)));
    }

    #[test]
    fn json_lines_round_trip_and_deterministic() {
        let l = generate_lattice(&params(BodyProfile::Hatch));
        assert_eq!(l, generate_lattice(&params(BodyProfile::Hatch)));
        let text = lattice_to_json_lines(&l);
        let back = wp_contracts::parse_lattice_def("test", &text).expect("parses");
        assert_eq!(back, l);
    }

    #[test]
    fn checked_in_debug_lattices_are_current() {
        for p in debug_lattice_params() {
            let path = format!(
                "{}/../../data/lattices/{}.json",
                env!("CARGO_MANIFEST_DIR"),
                p.id
            );
            let on_disk = wp_contracts::load_lattice_def(&path).expect("lattice loads");
            assert_eq!(
                on_disk,
                generate_lattice(&p),
                "{} is stale; run `just lattices`",
                p.id
            );
        }
    }

    #[test]
    fn push_bar_heavy_duty_and_detach_scale_options() {
        let mats = materials();
        let p = params(BodyProfile::Cruiser);
        let stock = generate_lattice(&p);
        let mut l = stock.clone();
        let o = PushBarOptions {
            mass_kg: 40.0,
            break_impulse_ns: 30_000.0,
            section_scale: 1.5,
            ..PushBarOptions::default()
        };
        let new = add_push_bar(&mut l, &p, &o);
        wp_contracts::validate_lattice_def(&l).unwrap();
        assert_eq!(l.nodes.len(), stock.nodes.len() + 4);
        let front = stock
            .nodes
            .iter()
            .map(|n| n.pos_m[1])
            .fold(f32::MIN, f32::max);
        for id in new {
            let n = &l.nodes[id as usize];
            assert_eq!(n.region, BodyRegion::Accessory);
            assert!((n.pos_m[1] - front - 0.10).abs() < 1e-3);
            assert_eq!(n.mass_kg, 10.0);
        }
        let bar = l
            .detachables
            .iter()
            .find(|d| d.group == GROUP_ACCESSORY)
            .unwrap();
        assert_eq!(bar.break_impulse_ns, 30_000.0);
        let mounts = l
            .beams
            .iter()
            .filter(|b| b.detach_group == Some(GROUP_ACCESSORY))
            .count();
        assert!(mounts >= 4, "{mounts} mounts");
        assert!(l
            .beams
            .windows(2)
            .all(|w| (w[0].a, w[0].b) <= (w[1].a, w[1].b)));
        // The model accepts it (the bar is a detachable group).
        let m = crate::DamageModel::new(&l, &mats);
        assert_eq!(m.health(), 1.0);

        let hd: BTreeMap<Component, f32> = [(Component::Radiator, 1.5)].into_iter().collect();
        apply_heavy_duty(&mut l, &hd);
        let rad = |l: &LatticeDef| {
            l.components
                .iter()
                .find(|c| c.kind == Component::Radiator)
                .unwrap()
                .intrusion_fail_m
        };
        assert!((rad(&l) - 1.5 * rad(&stock)).abs() < 1e-4);
        scale_detach_impulse(&mut l, BodyRegion::FrontBumper, 2.0);
        scale_detach_impulse(&mut l, BodyRegion::Accessory, 3.0);
        let imp = |l: &LatticeDef, g: u16| {
            l.detachables
                .iter()
                .find(|d| d.group == g)
                .unwrap()
                .break_impulse_ns
        };
        let f0 = imp(&stock, GROUP_FRONT_BUMPER);
        assert!((imp(&l, GROUP_FRONT_BUMPER) - 2.0 * f0).abs() < 0.1);
        assert_eq!(
            imp(&l, GROUP_ACCESSORY),
            30_000.0,
            "push bar keeps its own impulse"
        );
    }
}
