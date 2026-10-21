//! Per-car style sheets (`data/panels/styles/<car>.toml`): what makes each car look like itself.
//!
//! The lattice fixes *where* the hood, doors, roof, glass and trunk are (so panels deform and
//! hinge correctly); the style sheet decides *how the body between those anchors is shaped*
//! (nose and tail, flares, creases, hood bulge, roof bow, glass curvature, open bed or cargo
//! box) and *what is bolted on* (lamps, grille, bumpers, spoiler, exhausts, police gear, truck
//! hardware, wheels) plus the car's palette and personality text for the render list.
//!
//! Every field has a default, so a sheet only lists what differs from a plain modern car.
//! Units: metres; chassis space (+X right, +Y forward, +Z up). Lamp and part positions are
//! absolute chassis coordinates (run `car-profile <car>` for the anchors to author against).
//! Lateral `x` values are given for the right side and mirrored unless `mirror = false`.

use serde::{Deserialize, Serialize};

/// A whole style sheet.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CarStyle {
    /// Who the car is: personality text and the visual brief (render list).
    pub identity: Identity,
    /// Paint colours.
    pub palette: Palette,
    /// Body shape between the lattice anchors.
    pub body: BodyShape,
    /// Every lamp (head, tail, indicators, fogs, markers, police beacons, …).
    #[serde(rename = "lamp")]
    pub lamps: Vec<Lamp>,
    /// Bolt-on parts.
    pub parts: Parts,
    /// Wheels and tires.
    pub wheels: WheelStyle,
    /// Paint slot overrides per mesh part name (`door_fl = "accent"`, `hood = "primer"`).
    pub paint_overrides: std::collections::BTreeMap<String, PaintSlot>,
}

/// Personality and brief (render list text; no effect on geometry).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Identity {
    /// One-line character ("the delivery van that is always double-parked").
    pub tagline: String,
    /// A short paragraph: who drives it, how it carries itself.
    pub personality: String,
    /// The silhouette in a sentence (what you recognise it by at 200 m).
    pub silhouette: String,
    /// Signature details (the three things an artist must not lose).
    pub signatures: Vec<String>,
    /// Lighting description (day and night read).
    pub lighting: String,
}

/// Paint colours (`#rrggbb`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Palette {
    /// Signature body colour (player car, police livery base).
    pub base: String,
    /// Second livery colour (stripes, police doors, two-tone roofs, coloured wheels).
    pub accent: String,
    /// Primer / mismatched replacement panels.
    pub primer: String,
    /// Alternative body colours (other players, traffic).
    pub alts: Vec<String>,
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            base: "#8a8f96".into(),
            accent: "#e8e8e4".into(),
            primer: "#6d6f6a".into(),
            alts: Vec::new(),
        }
    }
}

/// Which paint a painted part uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaintSlot {
    /// Body colour.
    Paint,
    /// Accent colour.
    Accent,
    /// Primer.
    Primer,
    /// Black plastic.
    Trim,
    /// Chrome.
    Chrome,
    /// Satin / brushed metal.
    Metal,
}

/// Wheel-arch outline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchShape {
    /// Circular arch.
    #[default]
    Round,
    /// Flat top with rounded shoulders (trucks, vans, 70s cars).
    FlatTop,
    /// Trapezoid (80s boxes, off-roaders).
    Square,
}

/// What sits behind the cab of a truck-profile body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BedKind {
    /// Whatever the lattice says (closed deck: trunk lid / hatch / bed cover).
    #[default]
    Closed,
    /// Open pickup bed (inner walls, floor, tailgate).
    Open,
    /// Tall cargo box (box truck): `bed_height` is the box top.
    Box,
    /// Bare chassis deck between the rails (semi tractor): `bed_height` is the rail top,
    /// `bed_half_w` the rail half width.
    Rails,
    /// Steel tipper body (dump truck): open box with tall sides to `bed_height`.
    Tipper,
    /// Semi-trailer: no cab, the whole length is a dry-van box from `bed_floor` up to
    /// `bed_height` (0 = the lattice top), `bed_half_w` wide (0 = the lattice width). Built by
    /// [`crate::trailer`] instead of the car loft; hardware comes from `[parts.trailer]`.
    Trailer,
}

/// Body shape knobs. Defaults reproduce a plain modern car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BodyShape {
    /// Length of the nose rounding zone.
    pub nose_len: f32,
    /// Plan-view width loss at the very nose (0 = square corners).
    pub nose_plan: f32,
    /// Belt-line drop at the nose (big = wedge / long sloping bonnet).
    pub nose_drop: f32,
    /// Underside rise at the nose (approach chamfer).
    pub nose_lift: f32,
    /// Exponent of the nose rounding (1 = smooth, <1 = full round, >1 = crisp corner).
    pub nose_sharp: f32,
    /// Tail rounding zone length.
    pub tail_len: f32,
    /// Plan-view width loss at the tail.
    pub tail_plan: f32,
    /// Belt drop at the tail.
    pub tail_drop: f32,
    /// Underside rise at the tail.
    pub tail_lift: f32,
    /// Exponent of the tail rounding.
    pub tail_sharp: f32,
    /// Deck rises toward the tail (ducktail / integrated lip).
    pub tail_kick: f32,
    /// Greenhouse inset from the body side at the belt (tumblehome).
    pub tumblehome: f32,
    /// Roof half-width scale (on the lattice roof width).
    pub roof_w: f32,
    /// Lateral roof crown height.
    pub roof_crown: f32,
    /// Longitudinal roof bow: roof ends drop by this much (bubble roof).
    pub roof_bow: f32,
    /// The visible roof ends this far ahead of the lattice roof's rear edge; the rear glass
    /// starts there (turns a long-roof lattice into a sloping hatch).
    pub rear_rake: f32,
    /// The visible roof starts this far behind the lattice roof's front edge (more raked
    /// windscreen).
    pub ws_rake: f32,
    /// Windshield bulge (convex) at mid height.
    pub ws_bow: f32,
    /// Rear glass bulge (convex fastback).
    pub rear_bow: f32,
    /// Outward character-line bulge.
    pub crease: f32,
    /// Character-line height as a fraction from sill to belt.
    pub crease_at: f32,
    /// Shoulder chamfer at the belt.
    pub shoulder: f32,
    /// Rocker chamfer at the bottom.
    pub rocker: f32,
    /// Front fender flare (outward bulge around the front arches).
    pub flare_f: f32,
    /// Rear fender flare (haunches).
    pub flare_r: f32,
    /// Extra arch clearance over the tire radius.
    pub arch_gap: f32,
    /// Arch outline.
    pub arch: ArchShape,
    /// Hood centre bulge (power dome).
    pub hood_bulge: f32,
    /// Belt rises rearward across the doors (wedge side profile).
    pub belt_rise: f32,
    /// Belt kick-up at the rear quarter (over the haunches).
    pub belt_kick: f32,
    /// What sits behind the cab (truck/van bodies).
    pub bed: BedKind,
    /// Box / rail / tipper top height (bed kinds other than `closed` / `open`).
    pub bed_height: f32,
    /// Half width of the rails deck (`rails`) or box (`box`, 0 = body width).
    pub bed_half_w: f32,
    /// Bed / box starts this far behind the cab back (gap between cab and box).
    pub bed_gap: f32,
    /// Box / tipper floor height: below it only the chassis rails show (0 = body down to
    /// the sill).
    pub bed_floor: f32,
    /// Blacked-out pillars (else body colour).
    pub black_pillars: bool,
    /// No side glass behind the front doors (cargo / prisoner / armoured vans).
    pub blind_rear: bool,
}

impl Default for BodyShape {
    fn default() -> Self {
        BodyShape {
            nose_len: 0.35,
            nose_plan: 0.08,
            nose_drop: 0.10,
            nose_lift: 0.06,
            nose_sharp: 1.0,
            tail_len: 0.35,
            tail_plan: 0.08,
            tail_drop: 0.10,
            tail_lift: 0.06,
            tail_sharp: 1.0,
            tail_kick: 0.0,
            tumblehome: 0.10,
            roof_w: 1.0,
            roof_crown: 0.02,
            roof_bow: 0.0,
            rear_rake: 0.0,
            ws_rake: 0.0,
            ws_bow: 0.0,
            rear_bow: 0.0,
            crease: 0.0,
            crease_at: 0.62,
            shoulder: 0.06,
            rocker: 0.06,
            flare_f: 0.0,
            flare_r: 0.0,
            arch_gap: 0.07,
            arch: ArchShape::Round,
            hood_bulge: 0.0,
            belt_rise: 0.0,
            belt_kick: 0.0,
            bed: BedKind::Closed,
            bed_height: 0.0,
            bed_half_w: 0.0,
            bed_gap: 0.0,
            bed_floor: 0.0,
            black_pillars: true,
            blind_rear: false,
        }
    }
}

/// What a lamp does (decides its material and how the renderer lights it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LampKind {
    /// Main headlamp (white, lights up with the headlights).
    Head,
    /// Fog / driving lamp (headlight material).
    Fog,
    /// Daytime running strip (headlight material).
    Drl,
    /// Tail / brake lamp (red).
    Tail,
    /// Centre high-mount brake lamp (red).
    Brake3,
    /// Turn indicator / side repeater (amber).
    Indicator,
    /// Clearance marker (amber; trucks and vans).
    Marker,
    /// Reversing lamp (clear lens, unlit).
    Reverse,
    /// Police beacon, red.
    BeaconRed,
    /// Police beacon, blue.
    BeaconBlue,
}

/// Which face of the body a lamp sits on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Face {
    /// Nose (projected rearward onto the body): `x`, `z`.
    #[default]
    Front,
    /// Tail (projected forward): `x`, `z`.
    Rear,
    /// Body side (projected inward): `y`, `z`.
    Side,
    /// Top surface (projected down): `x`, `y`.
    Top,
}

/// Lens outline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LampShape {
    /// Rectangle.
    #[default]
    Rect,
    /// Circle (w = diameter).
    Round,
    /// Stadium / pill.
    Pill,
    /// Trapezoid: top edge shorter by `slant` on the outboard side.
    Swept,
    /// Narrow slit (thin LED strip).
    Slit,
    /// Triangle pointing inboard (wedge-era taillights).
    Wedge,
}

/// One lamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Lamp {
    /// Function.
    pub kind: LampKind,
    /// Face it sits on.
    pub face: Face,
    /// Outline.
    pub shape: LampShape,
    /// Lateral centre (Front/Rear/Top faces), right side.
    pub x: f32,
    /// Fore-aft centre (Side/Top faces).
    pub y: f32,
    /// Height centre (Front/Rear/Side faces).
    pub z: f32,
    /// Width along the face (x for front/rear/top, y for side).
    pub w: f32,
    /// Height along the face (z; y for top).
    pub h: f32,
    /// Outboard top-corner pull-in for `swept` (m), or tilt for others.
    pub slant: f32,
    /// Stand-off from the body (lens bulge).
    pub proud: f32,
    /// Dark bezel ring width (0 = none).
    pub bezel: f32,
    /// Mirror to the left side (else a single centred lamp at `x`).
    pub mirror: bool,
    /// Pop-up pod: a body-coloured box this tall carries the lamp (Top face).
    pub pod: f32,
}

impl Default for Lamp {
    fn default() -> Self {
        Lamp {
            kind: LampKind::Head,
            face: Face::Front,
            shape: LampShape::Rect,
            x: 0.6,
            y: 0.0,
            z: 0.7,
            w: 0.25,
            h: 0.1,
            slant: 0.0,
            proud: 0.012,
            bezel: 0.0,
            mirror: true,
            pod: 0.0,
        }
    }
}

/// Grille style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrilleStyle {
    /// No grille.
    #[default]
    None,
    /// Horizontal slats.
    Slats,
    /// Vertical bars (big-rig / classic).
    Bars,
    /// Dark mesh panel.
    Mesh,
    /// Egg-crate grid.
    Egg,
}

/// Grille.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Grille {
    /// Style.
    pub style: GrilleStyle,
    /// Half width.
    pub hw: f32,
    /// Centre height.
    pub z: f32,
    /// Height.
    pub h: f32,
    /// Bars / slats count.
    pub bars: u32,
    /// Chrome (else dark trim) bars and surround.
    pub chrome: bool,
    /// Surround frame width (0 = none).
    pub surround: f32,
    /// Lower intake below the bumper line (half width, centre z, height; 0 = none).
    pub intake: [f32; 3],
}

impl Default for Grille {
    fn default() -> Self {
        Grille {
            style: GrilleStyle::None,
            hw: 0.4,
            z: 0.6,
            h: 0.15,
            bars: 4,
            chrome: false,
            surround: 0.0,
            intake: [0.0; 3],
        }
    }
}

/// Bumper style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BumperStyle {
    /// Integrated cover (no extra part).
    #[default]
    Integrated,
    /// Chrome blade that wraps the corners.
    Chrome,
    /// Black rubbing strip.
    Rubber,
    /// Steel truck bumper (painted / chrome per `chrome`).
    Steel,
}

/// A bumper bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Bumper {
    /// Style.
    pub style: BumperStyle,
    /// Centre height.
    pub z: f32,
    /// Height.
    pub h: f32,
    /// Stand-off ahead of / behind the face.
    pub proud: f32,
    /// Wrap length around the corner.
    pub wrap: f32,
    /// Chrome finish (Steel).
    pub chrome: bool,
}

impl Default for Bumper {
    fn default() -> Self {
        Bumper {
            style: BumperStyle::Integrated,
            z: 0.45,
            h: 0.12,
            proud: 0.05,
            wrap: 0.25,
            chrome: false,
        }
    }
}

/// Rear wing / spoiler style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpoilerStyle {
    /// None.
    #[default]
    None,
    /// Low lip on the trunk edge.
    Lip,
    /// Wing on two uprights.
    Wing,
    /// Roof-edge spoiler over a hatch.
    Roof,
}

/// Spoiler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Spoiler {
    /// Style.
    pub style: SpoilerStyle,
    /// Fore-aft position of the blade's leading edge (0 = automatic).
    pub y: f32,
    /// Height of the blade above the surface below it.
    pub rise: f32,
    /// Chord (fore-aft depth).
    pub chord: f32,
    /// Half span.
    pub hw: f32,
    /// Body colour (else black).
    pub painted: bool,
}

impl Default for Spoiler {
    fn default() -> Self {
        Spoiler {
            style: SpoilerStyle::None,
            y: 0.0,
            rise: 0.2,
            chord: 0.22,
            hw: 0.65,
            painted: true,
        }
    }
}

/// Exhaust outlets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExhaustStyle {
    /// Hidden.
    #[default]
    None,
    /// One tip at `x` (right side).
    Single,
    /// One tip each side at ±`x`.
    Dual,
    /// Two tips each side.
    Quad,
    /// Two tips together in the centre.
    Centre,
    /// Vertical stacks behind the cab (trucks) at ±`x`, top at `z`.
    Stacks,
    /// Side pipe under the doors.
    Side,
}

/// Exhaust.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Exhaust {
    /// Style.
    pub style: ExhaustStyle,
    /// Lateral position.
    pub x: f32,
    /// Height (stacks: top height).
    pub z: f32,
    /// Tip diameter.
    pub d: f32,
    /// Fore-aft position (stacks only).
    pub y: f32,
}

impl Default for Exhaust {
    fn default() -> Self {
        Exhaust {
            style: ExhaustStyle::None,
            x: 0.45,
            z: 0.25,
            d: 0.08,
            y: 0.0,
        }
    }
}

/// Mirror style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorStyle {
    /// Modern body-colour shell on the door.
    #[default]
    Shell,
    /// Small chrome bullet on a stalk (60s–70s).
    Bullet,
    /// Big truck mirror on arms.
    Truck,
    /// Slim racing mirror.
    Slim,
}

/// Police light bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightbarStyle {
    /// None.
    #[default]
    None,
    /// Full-width bar on feet.
    Full,
    /// Low-profile slim bar.
    Low,
    /// Two separate domes (old school).
    Domes,
    /// Integrated roof-edge strip (vans, trucks): front + rear.
    Edge,
}

/// Light bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Lightbar {
    /// Style.
    pub style: LightbarStyle,
    /// Fore-aft centre (0 = mid roof).
    pub y: f32,
    /// Half width.
    pub hw: f32,
}

impl Default for Lightbar {
    fn default() -> Self {
        Lightbar {
            style: LightbarStyle::None,
            y: 0.0,
            hw: 0.6,
        }
    }
}

/// Front protection bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardStyle {
    /// None.
    #[default]
    None,
    /// Police push bar (two uprights, two cross bars, rubber pads).
    Push,
    /// Tubular bull bar.
    Bull,
    /// Armoured ram plate (SWAT).
    Ram,
}

/// Truck / utility hardware.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Utility {
    /// Sleeper box behind the cab: length (0 = day cab).
    pub sleeper: f32,
    /// Roof fairing on the cab to this height (0 = none).
    pub fairing: f32,
    /// Fifth-wheel coupling plate on the rails.
    pub fifth_wheel: bool,
    /// Saddle fuel tanks between the axles: (radius, length).
    pub fuel_tanks: [f32; 2],
    /// Mud flaps behind the rear wheels.
    pub mudflaps: bool,
    /// Steps under the doors.
    pub steps: bool,
    /// Roof ladder / rack (vans).
    pub roof_rack: bool,
    /// Roll-up rear door ribs (box truck).
    pub rollup: bool,
    /// Window bars / mesh guards over the glass (SWAT, prisoner van).
    pub window_guards: bool,
    /// Horn trumpets on the cab roof.
    pub air_horns: bool,
    /// Headache rack / cab guard on a tipper.
    pub cab_guard: bool,
    /// Rear ladder on a box body.
    pub ladder: bool,
    /// Roof hatch / turret ring (SWAT).
    pub hatch: bool,
    /// Side vents / louvres on the rear quarters (mid-engine).
    pub side_intakes: bool,
    /// Engine cover louvres on the rear deck (mid-engine).
    pub louvres: bool,
}

impl Default for Utility {
    fn default() -> Self {
        Utility {
            sleeper: 0.0,
            fairing: 0.0,
            fifth_wheel: false,
            fuel_tanks: [0.0, 0.0],
            mudflaps: false,
            steps: false,
            roof_rack: false,
            rollup: false,
            window_guards: false,
            air_horns: false,
            cab_guard: false,
            ladder: false,
            hatch: false,
            side_intakes: false,
            louvres: false,
        }
    }
}

/// Semi-trailer hardware (used only with `bed = "trailer"`). Positions are chassis metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TrailerParts {
    /// Kingpin (y, z): the pin's station and the underside of the upper coupler plate (where
    /// it sits on the tractor's fifth wheel; `wp_sim` hitches at this point).
    pub kingpin: [f32; 2],
    /// Landing gear leg station y (0 = 1.6 m behind the kingpin).
    pub landing_gear_y: f32,
    /// Landing gear lateral leg position |x|.
    pub landing_gear_x: f32,
    /// Landing gear foot pad height above the ground (legs wound up while coupled).
    pub landing_gear_lift: f32,
    /// Plan-view radius of the box's front vertical corners.
    pub corner_r: f32,
    /// Side underride guard rail between the landing gear and the tandem.
    pub side_guard: bool,
    /// Rear underride (ICC) bar centre height (0 = none).
    pub underride_z: f32,
    /// Vertical lock bars per rear door leaf.
    pub lock_bars: u32,
    /// Spacing of the exterior side posts of a sheet-and-post box (0 = smooth sides).
    pub post_pitch: f32,
    /// Height of the side bottom rail extrusion (its bottom is the box floor).
    pub bottom_rail: f32,
    /// Height of the rear door frame sill (below the doors, carries the rear lamps).
    pub rear_sill: f32,
}

impl Default for TrailerParts {
    fn default() -> Self {
        TrailerParts {
            kingpin: [5.2, 1.22],
            landing_gear_y: 0.0,
            landing_gear_x: 0.95,
            landing_gear_lift: 0.25,
            corner_r: 0.12,
            side_guard: true,
            underride_z: 0.55,
            lock_bars: 2,
            post_pitch: 1.2,
            bottom_rail: 0.2,
            rear_sill: 0.3,
        }
    }
}

/// Hood scoop style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoopStyle {
    /// None.
    #[default]
    None,
    /// Forward-facing ram scoop (body colour).
    Ram,
    /// Engine-mounted shaker poking through the hood (black).
    Shaker,
    /// Flat intercooler scoop (rally sedan).
    Flat,
    /// Twin heat-extractor vents (black).
    Vents,
}

/// Bolt-on parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Parts {
    /// Front grille.
    pub grille: Grille,
    /// Front bumper bar.
    pub bumper_f: Bumper,
    /// Rear bumper bar.
    pub bumper_r: Bumper,
    /// Front splitter depth (0 = none).
    pub splitter: f32,
    /// Rear diffuser fins.
    pub diffuser: bool,
    /// Side skirts.
    pub skirts: bool,
    /// Hood scoop.
    pub scoop: ScoopStyle,
    /// Rear spoiler.
    pub spoiler: Spoiler,
    /// Exhaust.
    pub exhaust: Exhaust,
    /// Door mirrors.
    pub mirrors: MirrorStyle,
    /// Police light bar.
    pub lightbar: Lightbar,
    /// Front guard.
    pub guard: GuardStyle,
    /// A-pillar spotlights (police).
    pub spotlights: bool,
    /// Roof antennas (count; police get 2–3).
    pub antennas: u32,
    /// Licence plates front and rear.
    pub plates: bool,
    /// Plate centre heights (front, rear); 0 = automatic (on the bumper covers).
    pub plate_z: [f32; 2],
    /// Door handles (small trim pulls).
    pub handles: bool,
    /// Twin racing stripes over hood, roof and deck: half width of each stripe, gap.
    pub stripes: [f32; 2],
    /// Truck / utility hardware.
    pub utility: Utility,
    /// Semi-trailer hardware (`bed = "trailer"` only).
    pub trailer: TrailerParts,
    /// Dark side-window tint on the rear glass (unmarked police, limo).
    pub dark_rear_glass: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Parts {
            grille: Grille::default(),
            bumper_f: Bumper::default(),
            bumper_r: Bumper::default(),
            splitter: 0.0,
            diffuser: false,
            skirts: false,
            scoop: ScoopStyle::None,
            spoiler: Spoiler::default(),
            exhaust: Exhaust::default(),
            mirrors: MirrorStyle::Shell,
            lightbar: Lightbar::default(),
            guard: GuardStyle::None,
            spotlights: false,
            antennas: 0,
            plates: true,
            plate_z: [0.0, 0.0],
            handles: true,
            stripes: [0.0, 0.0],
            utility: Utility::default(),
            trailer: TrailerParts::default(),
            dark_rear_glass: false,
        }
    }
}

/// Rim design.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RimStyle {
    /// Five broad spokes.
    #[default]
    Star5,
    /// Ten thin spokes.
    Spoke10,
    /// Fine multi-spoke mesh (cross-spoke).
    Mesh,
    /// Plain steel wheel with a small centre cap.
    Steel,
    /// Steel wheel with a full plastic hub cap.
    Hubcap,
    /// Deep dish with a polished lip (70s/80s).
    Dish,
    /// Turbine fan (90s exotic).
    Turbine,
    /// Heavy truck disc with hand holes and a lug ring.
    Truck,
    /// Six-spoke split (modern truck / police).
    Six,
}

/// Wheel styling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WheelStyle {
    /// Rim design.
    pub rim: RimStyle,
    /// Rim finish.
    pub finish: PaintSlot,
    /// Rim radius as a fraction of the tire radius (sidewall height).
    pub rim_frac: f32,
    /// Dual rear tires.
    pub dual_rear: bool,
    /// Dual tires on the front (physics) axle too: trailers, whose "front" axle is the
    /// leading axle of a tandem, not a steer axle.
    pub dual_front: bool,
    /// Extra (visual) rear axles: fore-aft offsets from the physics rear axle, m. The
    /// physics axle itself is drawn only when 0.0 is in the list (tandem: `[-0.66, 0.66]`).
    pub extra_axles: Vec<f32>,
    /// Raised white letters / white sidewall ring.
    pub white_letters: bool,
}

impl Default for WheelStyle {
    fn default() -> Self {
        WheelStyle {
            rim: RimStyle::Star5,
            finish: PaintSlot::Chrome,
            rim_frac: 0.66,
            dual_rear: false,
            dual_front: false,
            extra_axles: Vec::new(),
            white_letters: false,
        }
    }
}

/// Parse `#rrggbb` into 0xRRGGBB.
pub fn parse_rgb(s: &str) -> Result<u32, String> {
    let h = s
        .strip_prefix('#')
        .ok_or_else(|| format!("colour {s:?}: expected #rrggbb"))?;
    if h.len() != 6 {
        return Err(format!("colour {s:?}: expected #rrggbb"));
    }
    u32::from_str_radix(h, 16).map_err(|e| format!("colour {s:?}: {e}"))
}

impl CarStyle {
    /// Parse a TOML style sheet.
    pub fn parse(text: &str) -> Result<CarStyle, String> {
        let s: CarStyle = toml::from_str(text).map_err(|e| e.to_string())?;
        s.check()?;
        Ok(s)
    }

    /// Load `path`; a missing file is an error (every car has a sheet).
    pub fn load(path: &std::path::Path) -> Result<CarStyle, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        CarStyle::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Basic sanity: colours parse, sizes are positive, personality is written.
    pub fn check(&self) -> Result<(), String> {
        for c in [
            &self.palette.base,
            &self.palette.accent,
            &self.palette.primer,
        ]
        .into_iter()
        .chain(self.palette.alts.iter())
        {
            parse_rgb(c)?;
        }
        for (i, l) in self.lamps.iter().enumerate() {
            if l.w <= 0.0 || l.h <= 0.0 {
                return Err(format!("lamp {i}: w and h must be positive"));
            }
        }
        if self.identity.tagline.is_empty() || self.identity.personality.is_empty() {
            return Err("identity.tagline and identity.personality are required".into());
        }
        Ok(())
    }
}
