//! Car styles: the knobs a car's procedural mesh is built from.
//!
//! - [`BodyStyle`]: shape knobs of the lofted body (silhouette, nose, hood, greenhouse, sides,
//!   arches, tail). `BodyStyle::default()` reproduces the original generic body.
//! - [`Detail`]: extra geometry (scoops, spoilers, splitters, grilles, light bars …) bound to
//!   a body region and material, so it dents / swings / detaches with its panel.
//! - [`WheelStyle`] / [`MechStyle`]: wheels, tires, brakes, engine, suspension, exhaust
//!   (consumed by [`crate::wheels`] and [`crate::mechanical`]).
//! - [`CarStyle`] bundles them; [`style_for`] picks one per car: the per-car style modules
//!   ([`crate::styles`]) first, else a class-based default ([`class_default`]).
//!
//! Units: metres and degrees, chassis space (+X right, +Y forward, +Z up). Every knob lists
//! its sane range; values outside are clamped by the generator where that matters for
//! validity (never panics).

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, VehicleClass, VehicleDef};

use crate::builder::{Builder, PartKey, Patch};
use crate::mesh::{MaterialSlot, PartKind};
use crate::profile::BodyProfile;

/// Overall body type. Shapes the greenhouse / rear end; lattice facts win where they
/// conflict (a `Pickup` silhouette on a lattice with a trunk lid keeps the lid, an open bed
/// needs a lattice without trunk lid and rear glass).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Silhouette {
    /// Three-box sedan/coupe: roof, sloped rear glass, flat trunk deck (the original look).
    #[default]
    Notchback,
    /// Rear glass and deck form one continuous slope from the roof to the tail.
    Fastback,
    /// Short steep rear: glass runs from the roof to near the tail, chopped (Kamm) tail.
    Hatch,
    /// Roof runs back to the deck start, near-vertical rear glass.
    Wagon,
    /// Straight wedge: hood is one line from the nose lip to the cowl, low nose.
    Wedge,
    /// Cowl raised so hood and windshield read as one long sweep.
    CabForward,
    /// Cab + open bed (bed floor, inner walls, tailgate) when the lattice has no trunk lid.
    Pickup,
    /// Tall box: long flat roof to the tail, vertical rear, short sloped nose.
    Van,
    /// Tall wagon: long roof, near-vertical rear, high belt.
    Suv,
}

/// Wheel-arch (and arch flare) outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ArchShape {
    /// Circular arch.
    #[default]
    Round,
    /// Squared arch: flat top with chamfered corners (trucks, box flares, 80s cars).
    Box,
}

/// Hood scoop / vent variant emitted on the hood (region `Hood`, so it swings with it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HoodScoop {
    /// No scoop.
    #[default]
    None,
    /// Closed raised bulge (body colour) in the middle of the hood.
    Bulge,
    /// Forward-facing scoop: raised box with a dark open front.
    Scoop,
    /// Cowl induction: bulge open at the rear (toward the windshield).
    Cowl,
    /// Shaker: scoop standing on the engine through a hole (drawn as a dark box on a ring).
    Shaker,
    /// Two flat dark vents either side of the centre line.
    Vents,
}

/// Finish of a small body part (handles, mirror housings, drip rails, B-pillars).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrimFinish {
    /// Not drawn.
    None,
    /// Body colour (paint).
    #[default]
    Body,
    /// Black plastic / rubber trim.
    Black,
    /// Chrome.
    Chrome,
}

impl TrimFinish {
    /// Material slot of the finish (`None` for [`TrimFinish::None`]).
    pub fn material(self) -> Option<MaterialSlot> {
        match self {
            TrimFinish::None => None,
            TrimFinish::Body => Some(MaterialSlot::Paint),
            TrimFinish::Black => Some(MaterialSlot::Trim),
            TrimFinish::Chrome => Some(MaterialSlot::Chrome),
        }
    }
}

/// Which side the fuel filler door is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FuelDoor {
    /// No fuel door drawn.
    None,
    /// Left rear quarter (x < 0).
    Left,
    /// Right rear quarter (x > 0).
    #[default]
    Right,
}

/// Every shape knob of the lofted body. Distances in metres, angles in degrees.
/// `Option` knobs default to the value measured from the lattice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyStyle {
    /// Body type (see [`Silhouette`]).
    pub silhouette: Silhouette,

    // --- Cabin placement (visual; the lattice is not changed) ---
    /// Moves windshield base, roof and rear-glass base along +Y (forward). Range −0.60…+0.60
    /// ([`crate::profile::CABIN_SHIFT_RANGE`]). Mid-engine cab-forward cars want ≈ +0.3…+0.5
    /// (roof centre ≈ 42–46 % of the length from the nose), long-hood muscle −0.25…−0.45. The
    /// windshield base never passes 0.30 m behind the hood's front edge. The hood / trunk
    /// mesh then no longer starts exactly at the lattice hinge (it pivots on the lattice hinge
    /// nodes, offset by this amount).
    pub cabin_shift_m: f32,
    /// Visual hood length (nose → windshield base), m. `None` = measured (+ `cabin_shift_m`).
    /// Moves only the cowl/windshield base and header (roof front); the roof rear stays.
    pub hood_length: Option<f32>,

    // --- Nose ---
    /// Height of the nose leading edge (top of the front face), m above ground.
    /// `None` = measured (hood front height − `nose_drop`). Range 0.45…0.80.
    pub nose_h: Option<f32>,
    /// Lean-back of the nose face from vertical, degrees. 0 = upright, 60 = very raked.
    /// Range 0…60. Default 15 (the original rounding).
    pub nose_rake_deg: f32,
    /// Plan-view rounding length of the nose corners, m (the corner is inset by ≈ 0.23 × this).
    /// Range 0.05…0.45. Default 0.35.
    pub nose_plan_round: f32,
    /// Plan-view rounding length of the tail corners, m. Range 0.05…0.45. Default 0.35.
    pub tail_plan_round: f32,

    // --- Hood ---
    /// Centre rise of the hood over its edges, m. Range 0…0.05. Default 0.02.
    pub hood_crown: f32,
    /// Full-length raised centre band on the hood, m. Range 0…0.08. Default 0.
    pub hood_bulge_h: f32,
    /// Width of `hood_bulge_h` / `power_bulge_h` / scoops as a fraction of the hood width.
    /// Range 0.2…0.7. Default 0.45.
    pub hood_bulge_w: f32,
    /// Power bulge: centre bulge rising toward the windshield over the rear 65 % of the hood, m.
    /// Range 0…0.10. Default 0.
    pub power_bulge_h: f32,
    /// Hood scoop / vent variant (see [`HoodScoop`]).
    pub hood_scoop: HoodScoop,
    /// Scoop height (or vent length for `Vents`), m. Range 0.03…0.14. Default 0.06.
    pub hood_scoop_h: f32,
    /// Raises the hood's rear edge (cowl) toward the windshield, m (cab-forward look).
    /// Range −0.05…+0.25. Default 0.
    pub cowl_h: f32,
    /// Hood centre sunk below the front fender tops, m (raised "fender peaks" either side of a
    /// low hood). Fades out toward the nose. Range 0…0.08. Default 0.
    pub hood_valley: f32,
    /// Engine cover / deck centre sunk between the rear haunches, m. Fades out toward the
    /// tail. Range 0…0.08. Default 0.
    pub deck_valley: f32,

    // --- Greenhouse ---
    /// Windshield angle from vertical, degrees. `None` = measured (≈58). Range 45…72.
    /// Moves the roof front edge (header).
    pub windshield_rake_deg: Option<f32>,
    /// Plan-view inward lean of the A-pillar, degrees: the greenhouse narrows toward the
    /// windshield base. Range 0…25. Default 0 (the original straight greenhouse; ≈8 is typical).
    pub a_pillar_rake_deg: f32,
    /// Rear glass angle from vertical, degrees. `None` = measured. Range 20…85. Moves the roof
    /// rear edge (C-pillar top). Ignored by `Fastback` / `Hatch` (their line runs to the tail).
    pub rear_glass_rake_deg: Option<f32>,
    /// Lengthen (+) / shorten (−) the flat roof at its rear edge, m. Range −0.25…+0.15.
    pub roof_len_trim: f32,
    /// Roof height offset, m (− = chopped). Range −0.12…+0.10.
    pub roof_dz: f32,
    /// Centre rise of the roof over its edges, m. Range 0…0.05. Default 0.02.
    pub roof_crown: f32,
    /// Per-side inset of the roof edge vs the body side, m. `None` = measured (≥ 0.12).
    /// Range 0.10…0.30.
    pub tumblehome: Option<f32>,
    /// Inset of the side-glass base from the body side (shoulder ledge width), m.
    /// Range 0.04…0.20. Default 0.10.
    pub glass_inset: f32,
    /// Convex bow of the rear glass / fastback line, m (+ = bulges up). Range −0.03…+0.10.
    pub rear_glass_bow: f32,
    /// Side-view roundness of the greenhouse top, 0…1: 0 = straight windshield, flat roof and
    /// straight rear glass (the original trapezoid); 1 = one smooth arc from the windshield
    /// base over the roof to the rear glass base (or, for `Fastback` / `Hatch`, to the tail).
    /// Default 0.
    pub roof_arc: f32,
    /// Plan-view inward lean of the C-pillars, degrees: the greenhouse narrows toward the rear
    /// glass (teardrop / canopy cabin). Range 0…25. Default 0.
    pub c_pillar_rake_deg: f32,

    // --- Sides ---
    /// Belt-line (door top / shoulder) offset, m. Range −0.06…+0.08.
    pub belt_dz: f32,
    /// Belt rises by this much from the front-door hinge to the tail (wedge), m.
    /// Range −0.10…+0.15.
    pub belt_rake: f32,
    /// Rise of the belt line under the C-pillar (kick-up), m; carries on over the rear deck.
    /// Range 0…0.08.
    pub belt_kick: f32,
    /// Body-side character line height, fraction of the body height H from the ground.
    /// Range 0.25…0.70. Default 0.45 (sill-to-belt middle on most cars).
    pub crease_z: f32,
    /// Character line depth, m: the line stands out, the faces above and below lean in.
    /// Range 0…0.03. Default 0.
    pub crease_depth: f32,
    /// Underside chamfer (sill tuck-under), m. Range 0.01…0.12. Default 0.06.
    pub sill_tuck: f32,
    /// Shoulder rounding at the belt, m. Range 0.01…0.12. Default 0.06.
    pub shoulder_round: f32,
    /// Outward swell of the rear fender over the rear arch, m. Range 0…0.08.
    pub haunch: f32,
    /// Outward swell of the front fender over the front arch, m. Range 0…0.08.
    pub front_haunch: f32,
    /// Wheel-arch lip outward offset (flare), m. Range 0…0.08.
    pub flare: f32,
    /// Arch / flare outline.
    pub flare_shape: ArchShape,
    /// Wheel-arch clearance above the tire, m. Range 0.02…0.15. Default 0.07.
    pub arch_gap: f32,

    // --- Stance ---
    /// Visual body drop (− = lower) relative to the wheels, m. Range −0.05…+0.08.
    pub ride_dz: f32,
    /// Visual nose-down rake of the body, degrees (+ = nose down). Range −0.6…+0.8.
    pub stance_pitch_deg: f32,

    // --- Rear ---
    /// Deck falls by this much toward the tail, m. Range −0.05…+0.25.
    pub deck_drop: f32,
    /// Tail lip rise at the deck's trailing edge, m (negative = Kamm cut down).
    /// Range −0.03…+0.08.
    pub ducktail_h: f32,
    /// Side-view rounding length of the tail, m (small = chopped / Kamm). Range 0.05…0.6.
    /// Default 0.35.
    pub tail_round: f32,
    /// How far the tail top falls inside `tail_round`, m. Range 0…0.25. Default 0.10.
    pub tail_drop: f32,
    /// Visual (front, rear) overhang trim, m (+ lengthens, − shortens the mesh past the
    /// bumper nodes). Range −0.15…+0.10 each.
    pub overhang_trim: (f32, f32),
    /// Pickup bed depth below the rail, m (Pickup only). Range 0.2…0.6. Default 0.45.
    pub bed_depth: f32,

    // --- Greenhouse framing (see `profile::BodyProfile::ring` and `mesh::loft`) ---
    /// A-pillar width seen from the front, m: the body-colour band from the side-glass top edge
    /// over the roof-rail corner to the windshield edge, following the windshield rake up to the
    /// roof. Range 0.04…0.14. Default 0.075.
    pub a_pillar_w_m: f32,
    /// C-pillar (D-pillar on wagons / SUVs / vans) width seen from behind, at the rear glass, m.
    /// Blends into [`BodyStyle::a_pillar_w_m`] along the roof. Range 0.04…0.30. Default 0.10.
    pub c_pillar_w_m: f32,
    /// B-pillar width in side view (between the front and rear side windows), m. Range
    /// 0.05…0.25. Default 0.12.
    pub b_pillar_w_m: f32,
    /// B-pillar finish: [`TrimFinish::Black`] (default) or [`TrimFinish::Body`] (`None` and
    /// `Chrome` draw body colour).
    pub b_pillar: TrimFinish,
    /// Length (along y) of the sail at the A-pillar foot, ahead of the front door glass (the
    /// mirror patch), m. The front door glass runs forward under the A-pillar to it. Range
    /// 0.04…0.40. Default 0.14.
    pub a_sail_len_m: f32,
    /// Sail finish: [`TrimFinish::Black`] (default) or body colour.
    pub a_sail: TrimFinish,
    /// Roof rail: drop from the roof top to the side-glass top edge, m. Range 0.02…0.10.
    /// Default 0.05.
    pub roof_rail_drop_m: f32,
    /// How far every glass panel sits in behind the body surface (inset), m. Range 0…0.03.
    /// Default 0.012.
    pub glass_recess_m: f32,
    /// Width of the black seal / frame round every glass panel, m (seen face-on). Range
    /// 0…0.04. Default 0.018.
    pub glass_frame_m: f32,
    /// Plan-view wrap of the windshield: how far its corners sit behind its centre, m (the
    /// glass curves back at the sides). Range 0…0.20. Default 0.06.
    pub windshield_wrap_m: f32,
    /// Plan-view wrap of the rear glass (corners ahead of its centre), m. Range 0…0.20.
    /// Default 0.04.
    pub rear_glass_wrap_m: f32,
    /// Length of the black cowl / wiper panel at the windshield base (plan, along y), m.
    /// 0 = none. Range 0…0.20. Default 0.07.
    pub cowl_len_m: f32,
    /// Wiper blades parked on the windshield above the cowl. Range 0…2. Default 2.
    pub wipers: u8,
    /// Drip rail / roof rail line along the roof edge and down the A- and C-pillars.
    /// Default [`TrimFinish::Black`].
    pub drip_rail: TrimFinish,

    // --- Body details common to every car (see `trim.rs`) ---
    /// Door handles (one per door). Default [`TrimFinish::Body`] on a black recess.
    pub door_handles: TrimFinish,
    /// Door-mirror housing finish (the base and glass are always black / mirror).
    /// `None` = no generic mirrors. Default [`TrimFinish::Body`].
    pub mirrors: TrimFinish,
    /// Panel seams as thin dark lines: door cut lines, hood / trunk shut lines, bumper splits.
    /// Default true.
    pub panel_seams: bool,
    /// Width of a panel seam line, m. Range 0.003…0.012. Default 0.006.
    pub seam_w_m: f32,
    /// Fuel filler door side. Default [`FuelDoor::Right`].
    pub fuel_door: FuelDoor,
    /// Wheel-arch lip: radial width of the body-colour lip round each wheel opening, m.
    /// 0 = none. Range 0…0.04. Default 0.016.
    pub arch_lip_m: f32,
    /// Black liners inside the wheel arches (closes the open wheel wells). Default true.
    pub arch_liners: bool,
    /// Edge rounding of the nose and tail faces, m: the last loft ring at each end is set in
    /// by this much all round, so bumpers read rounded instead of slab-cut. 0 = sharp.
    /// Range 0…0.05. Default 0.03.
    pub end_round_m: f32,
}

impl Default for BodyStyle {
    fn default() -> Self {
        BodyStyle {
            silhouette: Silhouette::Notchback,
            cabin_shift_m: 0.0,
            hood_length: None,
            nose_h: None,
            nose_rake_deg: 15.0,
            nose_plan_round: 0.35,
            tail_plan_round: 0.35,
            hood_crown: 0.02,
            hood_bulge_h: 0.0,
            hood_bulge_w: 0.45,
            power_bulge_h: 0.0,
            hood_scoop: HoodScoop::None,
            hood_scoop_h: 0.06,
            cowl_h: 0.0,
            hood_valley: 0.0,
            deck_valley: 0.0,
            windshield_rake_deg: None,
            a_pillar_rake_deg: 0.0,
            rear_glass_rake_deg: None,
            roof_len_trim: 0.0,
            roof_dz: 0.0,
            roof_crown: 0.02,
            tumblehome: None,
            glass_inset: 0.10,
            rear_glass_bow: 0.0,
            roof_arc: 0.0,
            c_pillar_rake_deg: 0.0,
            belt_dz: 0.0,
            belt_rake: 0.0,
            belt_kick: 0.0,
            crease_z: 0.45,
            crease_depth: 0.0,
            sill_tuck: 0.06,
            shoulder_round: 0.06,
            haunch: 0.0,
            front_haunch: 0.0,
            flare: 0.0,
            flare_shape: ArchShape::Round,
            arch_gap: 0.07,
            ride_dz: 0.0,
            stance_pitch_deg: 0.0,
            deck_drop: 0.0,
            ducktail_h: 0.0,
            tail_round: 0.35,
            tail_drop: 0.10,
            overhang_trim: (0.0, 0.0),
            bed_depth: 0.45,
            a_pillar_w_m: 0.075,
            c_pillar_w_m: 0.10,
            b_pillar_w_m: 0.12,
            b_pillar: TrimFinish::Black,
            a_sail_len_m: 0.14,
            a_sail: TrimFinish::Black,
            roof_rail_drop_m: 0.05,
            glass_recess_m: 0.012,
            glass_frame_m: 0.018,
            windshield_wrap_m: 0.06,
            rear_glass_wrap_m: 0.04,
            cowl_len_m: 0.07,
            wipers: 2,
            drip_rail: TrimFinish::Black,
            door_handles: TrimFinish::Body,
            mirrors: TrimFinish::Body,
            panel_seams: true,
            seam_w_m: 0.006,
            fuel_door: FuelDoor::Right,
            arch_lip_m: 0.016,
            arch_liners: true,
            end_round_m: 0.03,
        }
    }
}

impl BodyStyle {
    /// A starting point for a silhouette: [`BodyStyle::default`] with knobs typical for it.
    pub fn preset(silhouette: Silhouette) -> BodyStyle {
        let mut s = BodyStyle {
            silhouette,
            ..BodyStyle::default()
        };
        match silhouette {
            Silhouette::Notchback => {}
            Silhouette::Fastback => {
                s.rear_glass_bow = 0.04;
                s.ducktail_h = 0.02;
                s.tail_round = 0.25;
            }
            Silhouette::Hatch => {
                s.tail_round = 0.15;
                s.tail_drop = 0.05;
            }
            Silhouette::Wagon => {
                s.tail_round = 0.15;
                s.tail_drop = 0.05;
            }
            Silhouette::Wedge => {
                s.nose_rake_deg = 45.0;
                s.belt_rake = 0.06;
                s.tail_round = 0.12;
                s.tail_drop = 0.04;
                s.hood_crown = 0.01;
            }
            Silhouette::CabForward => {
                s.cowl_h = 0.10;
                s.nose_rake_deg = 30.0;
            }
            Silhouette::Pickup => {
                s.tail_round = 0.08;
                s.tail_drop = 0.02;
                s.flare_shape = ArchShape::Box;
                s.nose_rake_deg = 8.0;
            }
            Silhouette::Van => {
                s.tail_round = 0.06;
                s.tail_drop = 0.02;
                s.nose_rake_deg = 25.0;
                s.roof_crown = 0.03;
            }
            Silhouette::Suv => {
                s.tail_round = 0.10;
                s.tail_drop = 0.03;
                s.flare_shape = ArchShape::Box;
                s.nose_rake_deg = 10.0;
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------------------
// Details
// ---------------------------------------------------------------------------------------

/// What [`Shape::Custom`] detail functions get to look at.
pub struct DetailCtx<'a> {
    /// The car.
    pub def: &'a VehicleDef,
    /// The styled body profile (anchors + loft surface queries).
    pub profile: &'a BodyProfile,
}

/// Custom detail emitter: gets the context and the detail's part key, may emit any parts.
pub type DetailFn = fn(&DetailCtx, &PartKey, &mut Builder);

/// Geometry of a [`Detail`]. Coordinates are chassis space (for mirrored details: the LEFT
/// side, x < 0; the right copy is generated).
#[derive(Debug, Clone)]
pub enum Shape {
    /// Axis-aligned box between corners `lo` and `hi` (the PLAN's `box`).
    Box {
        /// Min corner.
        lo: [f32; 3],
        /// Max corner.
        hi: [f32; 3],
    },
    /// Axis-aligned box with chamfered edges of size `bevel`.
    BeveledBox {
        /// Min corner.
        lo: [f32; 3],
        /// Max corner.
        hi: [f32; 3],
        /// Chamfer, m.
        bevel: f32,
    },
    /// Cylinder / cone frustum from `a` to `b`.
    Cylinder {
        /// Start centre.
        a: [f32; 3],
        /// End centre.
        b: [f32; 3],
        /// Radius at `a`.
        r_a: f32,
        /// Radius at `b`.
        r_b: f32,
        /// Segments around.
        segments: usize,
        /// Close both ends.
        caps: bool,
    },
    /// Torus.
    Torus {
        /// Centre.
        center: [f32; 3],
        /// Axis (normal of the ring plane).
        axis: [f32; 3],
        /// Ring radius.
        major: f32,
        /// Tube radius.
        minor: f32,
        /// Segments around the ring, around the tube.
        segments: (usize, usize),
    },
    /// Surface of revolution: `profile` points are (radius, distance along `axis`).
    Lathe {
        /// Base centre.
        center: [f32; 3],
        /// Axis.
        axis: [f32; 3],
        /// (radius, along) points.
        profile: Vec<[f32; 2]>,
        /// Segments around.
        segments: usize,
    },
    /// Closed 2-D profile swept along a polyline. Profile x runs along `side` = path tangent ×
    /// `up`, profile y along `up` (re-orthogonalised per path point).
    Extrude {
        /// Closed profile, CCW.
        profile: Vec<[f32; 2]>,
        /// Path points.
        path: Vec<[f32; 3]>,
        /// Up hint.
        up: [f32; 3],
        /// Close both ends.
        caps: bool,
    },
    /// Thin shell offset from the loft surface (scoops, flares, skirts, trim, decal carriers).
    Patch(Patch),
    /// Anything else: a function that emits into the builder (may use several keys).
    Custom(DetailFn),
}

/// One extra piece of geometry, bound to a body region and material.
#[derive(Debug, Clone)]
pub struct Detail {
    /// Part name (lower_snake_case, unique per car). Mirrored copies swap `_l`/`_r`.
    pub name: String,
    /// Region it is skinned to (dents / swings / detaches with).
    pub region: BodyRegion,
    /// Part kind (default [`PartKind::Detail`]).
    pub kind: PartKind,
    /// Material slot.
    pub material: MaterialSlot,
    /// Geometry.
    pub shape: Shape,
    /// Also emit the mirror image across x = 0 (region mirrored, `_l` → `_r` in the name).
    pub mirror: bool,
    /// Rigid: every vertex follows one lattice node (weight 255) instead of a 4-node blend.
    pub rigid: bool,
}

impl Detail {
    /// A non-mirrored, skinned detail of kind [`PartKind::Detail`].
    pub fn new(name: &str, region: BodyRegion, material: MaterialSlot, shape: Shape) -> Detail {
        Detail {
            name: name.to_string(),
            region,
            kind: PartKind::Detail,
            material,
            shape,
            mirror: false,
            rigid: false,
        }
    }

    /// Also emit the mirrored copy.
    pub fn mirrored(mut self) -> Detail {
        self.mirror = true;
        self
    }

    /// Bind rigidly to one node.
    pub fn rigid(mut self) -> Detail {
        self.rigid = true;
        self
    }

    /// Override the part kind.
    pub fn kind(mut self, kind: PartKind) -> Detail {
        self.kind = kind;
        self
    }
}

// ---------------------------------------------------------------------------------------
// Wheels
// ---------------------------------------------------------------------------------------

/// Rim design (names from `research_mechanical.md` §4.2). The foundation draws `Star` (the
/// original 10-point star) for every kind; the mechanical agent implements the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RimKind {
    /// Original flat 10-point star (far-LOD fallback).
    #[default]
    Star,
    /// Classic 5-spoke.
    FiveSpoke,
    /// Deep-dish mesh / cross-spoke.
    Mesh,
    /// Turbine / fan blades.
    Turbine,
    /// Multi-spoke (10×).
    MultiSpoke,
    /// Split Y spokes.
    SplitSpoke,
    /// 6-spoke tuner.
    SixSpoke,
    /// Steel wheel + full hubcap.
    SteelHubcap,
    /// Steel wheel + small centre cap (police).
    Steel,
    /// Steel wheel + trim ring + dog-dish cap.
    DogDish,
    /// Slotted mag (muscle).
    SlotMag,
    /// Monoblock forged (supercar).
    Monoblock,
    /// Truck 6-window.
    Truck,
    /// Basic OEM alloy.
    OemAlloy,
}

/// Centre cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CapKind {
    /// None.
    #[default]
    None,
    /// Small cap r 0.03.
    Small,
    /// Centre cap r 0.045.
    Centre,
    /// Full-face hubcap.
    Hubcap,
    /// Dog-dish r 0.12.
    DogDish,
    /// Centre-lock hex nut.
    CentreLock,
}

/// Wheel and tire look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelStyle {
    /// Rim design.
    pub rim: RimKind,
    /// Spoke count (0 = disc / steelie). Range 0…16.
    pub spokes: u8,
    /// Rim (bead seat) radius, m. `None` = 0.64 × wheel radius (the original look).
    pub rim_radius_m: Option<f32>,
    /// Recess of the spoke face from the outer lip, m. Range 0.005…0.07.
    pub face_depth_m: f32,
    /// Radial width of the outer lip, m. Range 0.01…0.045.
    pub lip_width_m: f32,
    /// Axial drop of spokes from hub to rim (concave), m. Range 0…0.04.
    pub spoke_concave_m: f32,
    /// Segments around the wheel (tire, rim lathe, disc share it). Range 12…32. Default 20.
    pub segments: u8,
    /// Centre cap.
    pub cap: CapKind,
    /// Lug count (0 = hidden).
    pub lugs: u8,
    /// Bolt circle radius, m.
    pub pcd_r_m: f32,
    /// Rim material (`Metal`, `Chrome`, `Trim` for black, `Carbon` …). `Metal` rims and
    /// plastic hubcaps are drawn in the `rim` slot, so [`WheelStyle::rim_rgb`] can tint them
    /// without touching discs or hardware.
    pub rim_material: MaterialSlot,
    /// Rim / hubcap colour (sRGB) written as `tints.rim` (gold, bronze, white …). `None` = the
    /// renderer's metal colour.
    pub rim_rgb: Option<[u8; 3]>,
    /// Sidewall height / section width. `None` = from the rim radius.
    pub tire_aspect: Option<f32>,
    /// Visual front camber, degrees (+ = top leans in). Range −1…6.
    pub front_camber_deg: f32,
    /// Visual rear camber, degrees (+ = top leans in). Range −1…8.
    pub rear_camber_deg: f32,
}

impl Default for WheelStyle {
    fn default() -> Self {
        WheelStyle {
            rim: RimKind::Star,
            spokes: 5,
            rim_radius_m: None,
            face_depth_m: 0.02,
            lip_width_m: 0.012,
            spoke_concave_m: 0.0,
            segments: 20,
            cap: CapKind::None,
            lugs: 5,
            pcd_r_m: 0.057,
            rim_material: MaterialSlot::Metal,
            rim_rgb: None,
            tire_aspect: None,
            front_camber_deg: 0.0,
            rear_camber_deg: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Mechanical
// ---------------------------------------------------------------------------------------

/// Engine type (drawn by [`crate::mechanical`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EngineKind {
    /// No engine geometry.
    None,
    /// Inline 4.
    #[default]
    Inline4,
    /// Inline 6.
    Inline6,
    /// V6.
    V6,
    /// V8.
    V8,
    /// V10.
    V10,
    /// V12.
    V12,
    /// Flat 4 / flat 6 (boxer).
    Flat,
    /// Rotary.
    Rotary,
    /// Electric motor + battery box.
    Electric,
}

/// Where the engine sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EngineLayout {
    /// Under the hood.
    #[default]
    Front,
    /// Behind the cabin, ahead of the rear axle.
    Mid,
    /// Behind the rear axle.
    Rear,
}

/// Engine top-end extras.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Induction {
    /// Naturally aspirated air box.
    #[default]
    Natural,
    /// Turbo(s) + intercooler.
    Turbo,
    /// Roots blower through the hood.
    Blower,
    /// Individual throttle bodies / velocity stacks.
    Stacks,
}

/// Suspension layout (per axle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SuspensionKind {
    /// MacPherson strut.
    #[default]
    Strut,
    /// Double wishbone / SLA.
    DoubleWishbone,
    /// Multi-link.
    Multilink,
    /// Live axle on coils.
    SolidAxleCoil,
    /// Live axle on leaf springs.
    SolidAxleLeaf,
    /// Torsion beam.
    TorsionBeam,
    /// Race coilovers (wishbones + coloured springs).
    Coilover,
}

/// Caliper type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CaliperKind {
    /// Single-piston floating caliper.
    #[default]
    Floating,
    /// 4-piston fixed.
    Fixed4,
    /// 6-piston fixed.
    Fixed6,
    /// Drum brake (no visible caliper; a drum instead of a disc).
    Drum,
}

/// Brakes of one axle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrakeStyle {
    /// Disc radius as a fraction of the rim radius. Range 0.6…0.95. Default 0.80.
    pub disc_radius_frac: f32,
    /// Disc thickness, m. Range 0.01…0.04.
    pub disc_thickness_m: f32,
    /// Vented disc.
    pub vented: bool,
    /// Drilled / slotted face.
    pub drilled: bool,
    /// Caliper type.
    pub caliper: CaliperKind,
}

impl Default for BrakeStyle {
    fn default() -> Self {
        BrakeStyle {
            disc_radius_frac: 0.80,
            disc_thickness_m: 0.024,
            vented: true,
            drilled: false,
            caliper: CaliperKind::Floating,
        }
    }
}

/// Exhaust outlets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExhaustKind {
    /// Hidden.
    None,
    /// One tip, left.
    #[default]
    Single,
    /// One tip per side.
    Dual,
    /// Two tips per side.
    Quad,
    /// Centre exit.
    Centre,
    /// Side pipes under the doors.
    SidePipes,
    /// Four tips in a row in the centre of the valance (90s mid-engine exotic).
    CentreQuad,
    /// Four big tips high in the centre of a mesh rear panel (supercar).
    HighCentre,
    /// One tip per side, turned down under the valance.
    DualDown,
    /// Single exit behind the right rear wheel (trucks, SUVs).
    SideExit,
    /// Bare bent pipe under the rear bumper, no tip.
    Bare,
}

/// Engine, suspension, brakes and exhaust.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MechStyle {
    /// Engine type.
    pub engine: EngineKind,
    /// Engine position.
    pub layout: EngineLayout,
    /// Induction extras.
    pub induction: Induction,
    /// Emit the detailed engine (only for cars whose engine shows through glass / scoops /
    /// an open hood often); otherwise a cheap block.
    pub show_engine_detail: bool,
    /// Front suspension.
    pub front_suspension: SuspensionKind,
    /// Rear suspension.
    pub rear_suspension: SuspensionKind,
    /// Front brakes.
    pub front_brakes: BrakeStyle,
    /// Rear brakes.
    pub rear_brakes: BrakeStyle,
    /// Caliper colour (sRGB). `None` = renderer default (dark grey).
    pub caliper_rgb: Option<[u8; 3]>,
    /// Spring colour for coilovers (sRGB). `None` = metal.
    pub spring_rgb: Option<[u8; 3]>,
    /// Exhaust outlets.
    pub exhaust: ExhaustKind,
    /// Exhaust tip radius, m. Range 0.025…0.06.
    pub exhaust_tip_r_m: f32,
}

impl Default for MechStyle {
    fn default() -> Self {
        MechStyle {
            engine: EngineKind::Inline4,
            layout: EngineLayout::Front,
            induction: Induction::Natural,
            show_engine_detail: false,
            front_suspension: SuspensionKind::Strut,
            rear_suspension: SuspensionKind::Multilink,
            front_brakes: BrakeStyle::default(),
            rear_brakes: BrakeStyle {
                disc_radius_frac: 0.72,
                disc_thickness_m: 0.012,
                vented: false,
                ..BrakeStyle::default()
            },
            caliper_rgb: None,
            spring_rgb: None,
            exhaust: ExhaustKind::Single,
            exhaust_tip_r_m: 0.035,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Livery / car style
// ---------------------------------------------------------------------------------------

/// Hint for the renderer's livery painter (`render/livery.ts`). Written into the mesh file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveryHint {
    /// Scheme id the livery painter knows (e.g. `"police_two_tone"`).
    pub scheme: String,
    /// Colours (sRGB), meaning defined by the scheme.
    pub colors: Vec<[u8; 3]>,
    /// Text strings (meaning defined by the scheme; invented names only).
    pub text: Vec<String>,
}

/// Detail / triangle-budget tier of a car.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tier {
    /// Hero / performance / police: 8k–22k triangles, finer loft.
    #[default]
    Hero,
    /// Traffic: 4k–11k triangles, coarser loft.
    Traffic,
}

impl Tier {
    /// Triangle budget (min, max) of the whole mesh.
    pub fn tri_budget(self) -> (u32, u32) {
        match self {
            Tier::Hero => (8000, 22000),
            Tier::Traffic => (4000, 11000),
        }
    }
}

/// Everything that shapes one car's mesh.
#[derive(Debug, Clone)]
pub struct CarStyle {
    /// Body shape.
    pub body: BodyStyle,
    /// Extra geometry.
    pub details: Vec<Detail>,
    /// Wheels and tires.
    pub wheel: WheelStyle,
    /// Engine, suspension, brakes, exhaust.
    pub mechanical: MechStyle,
    /// Livery hint for the renderer.
    pub livery: Option<LiveryHint>,
    /// Per-car material colour overrides (sRGB) written to the mesh (`tints`).
    pub tints: Vec<(MaterialSlot, [u8; 3])>,
    /// Budget tier. Set by [`style_for`] from the module that styled the car
    /// (performance / police → Hero, traffic → Traffic, class default → by class).
    pub tier: Tier,
    /// Second paint colour (sRGB) for details in the `accent` slot (stripes, accent bars),
    /// written as `tints.accent`. `None` = the renderer's default accent (dark red).
    pub accent_rgb: Option<[u8; 3]>,
    /// Underglow colour (sRGB). `Some` adds emissive strips under both sills (`underglow`
    /// slot, `tints.underglow`), lit by the renderer only while the headlights are on.
    pub underglow_rgb: Option<[u8; 3]>,
}

impl CarStyle {
    /// A style with the given body and defaults for everything else.
    pub fn with_body(body: BodyStyle) -> CarStyle {
        CarStyle {
            body,
            details: Vec::new(),
            wheel: WheelStyle::default(),
            mechanical: MechStyle::default(),
            livery: None,
            tints: Vec::new(),
            tier: Tier::Hero,
            accent_rgb: None,
            underglow_rgb: None,
        }
    }
}

/// Tier of a car that no style module claims, from its class.
pub fn class_tier(class: VehicleClass) -> Tier {
    match class {
        VehicleClass::Beater | VehicleClass::Compact | VehicleClass::Truck => Tier::Traffic,
        _ => Tier::Hero,
    }
}

/// Class-based default style (used when no style module returns one).
pub fn class_default(def: &VehicleDef) -> CarStyle {
    let tall = def.body_size_m[2] >= 1.95;
    let body = match def.class {
        VehicleClass::Truck if tall => BodyStyle::preset(Silhouette::Van),
        VehicleClass::Truck => BodyStyle::preset(Silhouette::Pickup),
        VehicleClass::PoliceSuv => BodyStyle::preset(Silhouette::Suv),
        _ => BodyStyle::default(),
    };
    let mut mechanical = MechStyle::default();
    match def.class {
        VehicleClass::Exotic => {
            mechanical.layout = EngineLayout::Mid;
            mechanical.engine = EngineKind::V8;
            mechanical.exhaust = ExhaustKind::Dual;
        }
        VehicleClass::Muscle | VehicleClass::PoliceCruiser | VehicleClass::PoliceInterceptor => {
            mechanical.engine = EngineKind::V8;
            mechanical.exhaust = ExhaustKind::Dual;
        }
        VehicleClass::Truck | VehicleClass::PoliceSuv => {
            mechanical.engine = EngineKind::V8;
            mechanical.rear_suspension = SuspensionKind::SolidAxleLeaf;
        }
        _ => {}
    }
    CarStyle {
        mechanical,
        tier: class_tier(def.class),
        ..CarStyle::with_body(body)
    }
}

/// The style of a car: `styles::performance`, then `styles::traffic`, then `styles::police`
/// (first `Some` wins), else [`class_default`]. Sets [`CarStyle::tier`].
pub fn style_for(def: &VehicleDef) -> CarStyle {
    if let Some(mut s) = crate::styles::performance::style(def) {
        s.tier = Tier::Hero;
        return s;
    }
    if let Some(mut s) = crate::styles::traffic::style(def) {
        s.tier = Tier::Traffic;
        return s;
    }
    if let Some(mut s) = crate::styles::police::style(def) {
        s.tier = Tier::Hero;
        return s;
    }
    class_default(def)
}
