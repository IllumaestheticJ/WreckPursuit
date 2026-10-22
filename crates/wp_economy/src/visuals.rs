//! What a build looks like: the parts its mods bolt onto the car, for the renderer.
//!
//! Every mod that changes something you could see on a real car says so, and this module turns
//! an installed set into one [`BuildVisuals`] the browser draws:
//!
//! - **parts** ([`PartVisual`]): a wing, a splitter, rims, a roll cage, exhaust tips… Each part
//!   names the body regions it is mounted on ([`PartKind::regions`]), so the renderer skins it to
//!   those lattice nodes: it dents with the panel under it and leaves the car when that region
//!   detaches (a wing goes with the trunk lid, a lip with the front bumper).
//! - **wheels** ([`WheelVisual`]): tire width / radius and compound per wheel, read from the
//!   *modified* [`VehicleDef`], next to the stock values, so wider tires are drawn wider.
//! - **ride height** ([`BuildVisuals::ride_height_m`]): lowering springs and lift kits.
//!
//! Parts come from two places, so the data never says the same thing twice:
//! 1. derived from effects — [`ModEffect::Cosmetic`] (rims, body kits, hood, roof, paint,
//!    vinyl, tint, lights, interior) and `wheels.tire_id` (tread pattern);
//! 2. a mod's explicit `visuals` list (`ModDef::visuals`) for performance and protection parts
//!    whose effects are numbers (aero, brake kits, cages, bars, exhaust, intercooler,
//!    supercharger, carbon panels).
//!
//! Styles in explicit `visuals` lists are checked against [`PartKind::styles`] when the catalog
//! loads, so a typo there fails the data tests instead of drawing nothing. A cosmetic value with
//! no style here (and the cosmetic kinds [`PartKind::of_cosmetic`] maps to `None`) derives no
//! part: the testbed's cosmetic look draws it from `data/cosmetics/visuals.json`, which
//! `tests/visuals.rs` checks covers every mod. The browser draws each mod once: the parts here
//! first, the look for the mods they leave out.

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, VehicleDef};

use crate::mods::{AppliedMods, CosmeticKind, ModDef, ModEffect, ModOp, ParamValue, VehicleParam};

/// A kind of visible part. Serialized by name (`"RearWing"`), as in `data/mods`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PartKind {
    /// Rim design (replaces the stock rims).
    Rims,
    /// Tire tread / sidewall (style = tire id, any compound).
    Tires,
    /// Big brake kit: large discs and painted calipers behind the rims.
    BrakeKit,
    /// Front splitter / low-drag front lip under the bumper.
    FrontSplitter,
    /// Rear wing on the trunk lid.
    RearWing,
    /// Rear diffuser under the rear bumper.
    Diffuser,
    /// Front bumper body kit.
    FrontBumperKit,
    /// Side skirts / widebody arches.
    SideKit,
    /// Rear bumper body kit.
    RearBumperKit,
    /// Hood vents / scoop.
    HoodStyle,
    /// Roof scoop / rack.
    RoofPiece,
    /// Light covers (tints the lamps).
    Lights,
    /// Paint finish (paint material).
    Paint,
    /// Vinyl livery over the paint.
    Vinyl,
    /// Window tint (glass material).
    WindowTint,
    /// Interior trim colour.
    InteriorTrim,
    /// Roll cage tubes inside the cabin.
    RollCage,
    /// Steel bar across the front bumper (reinforced front bumper).
    BullBar,
    /// Steel bar across the rear bumper (reinforced rear bumper).
    RearBar,
    /// Police push bar on the nose.
    PushBar,
    /// Skid plate under the engine.
    SkidPlate,
    /// Mesh guard in the grille.
    RadiatorGuard,
    /// Bolted armor plates on the doors.
    SideArmor,
    /// Exhaust tips out of the rear bumper.
    Exhaust,
    /// Front-mount intercooler behind the bumper opening.
    Intercooler,
    /// Supercharger blower through the hood.
    Supercharger,
    /// Hood panel material (carbon).
    HoodPanel,
    /// Trunk lid material (carbon).
    TrunkPanel,
}

impl PartKind {
    /// Every kind (stable order, for docs and tests).
    pub const ALL: [PartKind; 28] = [
        PartKind::Rims,
        PartKind::Tires,
        PartKind::BrakeKit,
        PartKind::FrontSplitter,
        PartKind::RearWing,
        PartKind::Diffuser,
        PartKind::FrontBumperKit,
        PartKind::SideKit,
        PartKind::RearBumperKit,
        PartKind::HoodStyle,
        PartKind::RoofPiece,
        PartKind::Lights,
        PartKind::Paint,
        PartKind::Vinyl,
        PartKind::WindowTint,
        PartKind::InteriorTrim,
        PartKind::RollCage,
        PartKind::BullBar,
        PartKind::RearBar,
        PartKind::PushBar,
        PartKind::SkidPlate,
        PartKind::RadiatorGuard,
        PartKind::SideArmor,
        PartKind::Exhaust,
        PartKind::Intercooler,
        PartKind::Supercharger,
        PartKind::HoodPanel,
        PartKind::TrunkPanel,
    ];

    /// Body regions the part is mounted on. The renderer binds each drawn piece to the nearest
    /// of these regions' lattice nodes (dents, hinge swing, detachment): a left skirt to the
    /// left sill, a front flare to its fender. Empty = not on the body: wheel parts follow the
    /// wheels, material changes follow whatever panel they recolour.
    pub fn regions(self) -> &'static [BodyRegion] {
        use BodyRegion as R;
        match self {
            PartKind::Rims
            | PartKind::Tires
            | PartKind::BrakeKit
            | PartKind::Paint
            | PartKind::Vinyl
            | PartKind::WindowTint
            | PartKind::InteriorTrim
            | PartKind::Lights => &[],
            PartKind::FrontSplitter
            | PartKind::FrontBumperKit
            | PartKind::BullBar
            | PartKind::PushBar
            | PartKind::RadiatorGuard
            | PartKind::Intercooler => &[R::FrontBumper],
            PartKind::RearWing | PartKind::TrunkPanel => &[R::Trunk],
            PartKind::Diffuser
            | PartKind::RearBumperKit
            | PartKind::RearBar
            | PartKind::Exhaust => &[R::RearBumper],
            // Skirts sit on the sills; widebody flares on the fenders and quarters.
            PartKind::SideKit => &[
                R::FrameLeft,
                R::FrameRight,
                R::FrontLeftFender,
                R::FrontRightFender,
                R::RearLeftQuarter,
                R::RearRightQuarter,
            ],
            PartKind::HoodStyle | PartKind::Supercharger | PartKind::HoodPanel => &[R::Hood],
            PartKind::RoofPiece => &[R::Roof],
            PartKind::RollCage => &[R::Cabin],
            PartKind::SkidPlate => &[R::FrameFront],
            PartKind::SideArmor => &[R::LeftDoorF, R::LeftDoorR, R::RightDoorF, R::RightDoorR],
        }
    }

    /// Styles the renderer knows for this kind; `None` = any value (tire ids).
    pub fn styles(self) -> Option<&'static [&'static str]> {
        Some(match self {
            PartKind::Rims => &["steel", "mesh", "forged_5spoke"],
            PartKind::Tires => return None,
            PartKind::BrakeKit => &["street", "race"],
            PartKind::FrontSplitter => &["splitter", "lowdrag"],
            PartKind::RearWing => &["street", "gt"],
            PartKind::Diffuser => &["diffuser"],
            PartKind::FrontBumperKit => &["lip_a", "wide_a", "aggro_a"],
            PartKind::SideKit => &["skirt_a", "wide_a"],
            PartKind::RearBumperKit => &["rear_a", "wide_a"],
            PartKind::HoodStyle => &["vented", "scoop"],
            PartKind::RoofPiece => &["scoop", "rack"],
            PartKind::Lights => &["smoked"],
            PartKind::Paint => &["gloss", "metallic", "matte", "pearl"],
            PartKind::Vinyl => &["stripes", "flames", "tribal", "sponsor_generic"],
            PartKind::WindowTint => &["dark"],
            PartKind::InteriorTrim => &["suede_look"],
            PartKind::RollCage => &["bolt_in", "welded"],
            PartKind::BullBar | PartKind::RearBar => &["steel"],
            PartKind::PushBar => &["police"],
            PartKind::SkidPlate => &["plate"],
            PartKind::RadiatorGuard => &["mesh"],
            PartKind::SideArmor => &["plate"],
            PartKind::Exhaust => &["sport", "race"],
            PartKind::Intercooler => &["fmic"],
            PartKind::Supercharger => &["blower_1", "blower_2"],
            PartKind::HoodPanel | PartKind::TrunkPanel => &["carbon"],
        })
    }

    /// The part a cosmetic effect draws; `None` for kinds only the testbed's cosmetic looks
    /// draw (`data/cosmetics/visuals.json`: fog lights, underglow, mud flaps, exhaust tips,
    /// tyre sidewalls, window nets).
    pub fn of_cosmetic(kind: CosmeticKind) -> Option<PartKind> {
        Some(match kind {
            CosmeticKind::BodyKitFront => PartKind::FrontBumperKit,
            CosmeticKind::BodyKitSide => PartKind::SideKit,
            CosmeticKind::BodyKitRear => PartKind::RearBumperKit,
            CosmeticKind::HoodStyle => PartKind::HoodStyle,
            CosmeticKind::RoofStyle => PartKind::RoofPiece,
            CosmeticKind::Lights => PartKind::Lights,
            CosmeticKind::Paint => PartKind::Paint,
            CosmeticKind::Vinyl => PartKind::Vinyl,
            CosmeticKind::Wheels => PartKind::Rims,
            CosmeticKind::WindowTint => PartKind::WindowTint,
            CosmeticKind::Interior => PartKind::InteriorTrim,
            CosmeticKind::FogLights
            | CosmeticKind::Underglow
            | CosmeticKind::MudFlaps
            | CosmeticKind::ExhaustTip
            | CosmeticKind::Sidewall
            | CosmeticKind::WindowNet => return None,
        })
    }

    /// Whether `style` is one the renderer draws for this kind.
    pub fn accepts(self, style: &str) -> bool {
        !style.is_empty() && self.styles().is_none_or(|s| s.contains(&style))
    }
}

/// An explicit visible part on a mod (`"visuals": [{"part": "RearWing", "style": "gt"}]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualPart {
    /// What it is.
    pub part: PartKind,
    /// Which design ([`PartKind::styles`]).
    pub style: String,
}

/// One part to draw on a build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartVisual {
    /// What it is.
    pub part: PartKind,
    /// Which design.
    pub style: String,
    /// Mod that fitted it.
    pub mod_id: String,
    /// Regions it is mounted on ([`PartKind::regions`]).
    pub regions: Vec<BodyRegion>,
}

/// One wheel of a build, stock next to modified (FL, FR, RL, RR).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WheelVisual {
    /// Stock loaded tire radius, m.
    pub base_radius_m: f32,
    /// Modified tire radius, m.
    pub radius_m: f32,
    /// Stock tire width, m.
    pub base_width_m: f32,
    /// Modified tire width, m.
    pub width_m: f32,
    /// Stock tire id.
    pub base_tire_id: String,
    /// Fitted tire id.
    pub tire_id: String,
}

/// Everything the renderer needs to draw a modified car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildVisuals {
    /// Car id.
    pub car_id: String,
    /// Installed mods (sorted).
    pub mods: Vec<String>,
    /// Parts, in mod order (slot, line, id), then derivation order within a mod.
    pub parts: Vec<PartVisual>,
    /// Per wheel, FL, FR, RL, RR.
    pub wheels: Vec<WheelVisual>,
    /// Body height change from the suspension, m (negative = lowered).
    pub ride_height_m: f32,
}

impl BuildVisuals {
    /// Whether a part of `kind` is fitted.
    pub fn has(&self, kind: PartKind) -> bool {
        self.parts.iter().any(|p| p.part == kind)
    }

    /// Style of the fitted part of `kind`, if any (the last one wins, as the renderer does).
    pub fn style(&self, kind: PartKind) -> Option<&str> {
        self.parts
            .iter()
            .rev()
            .find(|p| p.part == kind)
            .map(|p| p.style.as_str())
    }
}

/// Parts one mod draws: derived from its effects, then its explicit `visuals`.
pub fn mod_parts(m: &ModDef) -> Vec<(PartKind, String)> {
    let mut out = Vec::new();
    for e in &m.effects {
        match e {
            // A cosmetic value this renderer has no style for (turbofan rims, satin paint, the
            // newer vinyls, ...) is drawn by the testbed's cosmetic look instead
            // (`data/cosmetics/visuals.json`), so it derives no part here.
            ModEffect::Cosmetic { kind, value } => {
                if let Some(part) = PartKind::of_cosmetic(*kind).filter(|p| p.accepts(value)) {
                    out.push((part, value.clone()))
                }
            }
            ModEffect::Vehicle {
                param: VehicleParam::WheelsTireId,
                value: ParamValue::Text(t),
                ..
            } => out.push((PartKind::Tires, t.clone())),
            _ => {}
        }
    }
    out.extend(m.visuals.iter().map(|v| (v.part, v.style.clone())));
    out
}

/// Ride height change of a set of mods, m (sum of `suspension.ride_height_m` effects).
pub fn ride_height_m(mods: &[&ModDef]) -> f32 {
    mods.iter()
        .flat_map(|m| &m.effects)
        .map(|e| match e {
            ModEffect::Vehicle {
                param: VehicleParam::SuspensionRideHeightM,
                op: ModOp::Add,
                value: ParamValue::Number(v),
            } => *v,
            _ => 0.0,
        })
        .sum()
}

/// What `mods` (already validated and applied: `applied` is [`crate::mods::apply_mods`] of
/// `base` with the same set) look like on the car.
pub fn build_visuals(base: &VehicleDef, applied: &AppliedMods, mods: &[&ModDef]) -> BuildVisuals {
    let mut parts = Vec::new();
    for m in mods {
        for (part, style) in mod_parts(m) {
            parts.push(PartVisual {
                part,
                style,
                mod_id: m.id.clone(),
                regions: part.regions().to_vec(),
            });
        }
    }
    let wheels = base
        .wheels
        .iter()
        .zip(&applied.vehicle.wheels)
        .map(|(b, w)| WheelVisual {
            base_radius_m: b.radius_m,
            radius_m: w.radius_m,
            base_width_m: b.width_m,
            width_m: w.width_m,
            base_tire_id: b.tire_id.clone(),
            tire_id: w.tire_id.clone(),
        })
        .collect();
    let mut ids: Vec<String> = mods.iter().map(|m| m.id.clone()).collect();
    ids.sort();
    BuildVisuals {
        car_id: base.id.clone(),
        mods: ids,
        parts,
        wheels,
        ride_height_m: ride_height_m(mods),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::ModSlot;
    use crate::Economy;

    /// Slots whose every mod must change how the car looks.
    const VISIBLE_SLOTS: [ModSlot; 10] = [
        ModSlot::Wheels,
        ModSlot::Tires,
        ModSlot::Aero,
        ModSlot::BodyFront,
        ModSlot::BodySides,
        ModSlot::BodyRear,
        ModSlot::Hood,
        ModSlot::Roof,
        ModSlot::Paint,
        ModSlot::Vinyls,
    ];

    /// Mods in those slots that are invisible on a real car too.
    const HIDDEN: [&str; 1] = [
        // Inserts inside the tire.
        "tires_runflat",
    ];

    fn econ() -> Economy {
        Economy::embedded()
    }

    fn changes_wheels(m: &ModDef) -> bool {
        m.effects.iter().any(|e| {
            matches!(
                e,
                ModEffect::Vehicle {
                    param: VehicleParam::WheelsWidthM,
                    ..
                }
            )
        })
    }

    /// Mods the testbed's cosmetic look draws pieces for (`data/cosmetics/visuals.json`; an
    /// entry of `{"none": ...}` draws nothing).
    fn drawn_by_cosmetic_look() -> std::collections::BTreeSet<String> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cosmetics/visuals.json"
        );
        let text = std::fs::read_to_string(path).expect("data/cosmetics/visuals.json");
        let v: serde_json::Value = serde_json::from_str(&text).expect("visuals.json parses");
        v["mods"]
            .as_object()
            .expect("visuals.json mods")
            .iter()
            .filter(|(_, pieces)| pieces.as_array().is_some_and(|a| !a.is_empty()))
            .map(|(id, _)| id.clone())
            .collect()
    }

    #[test]
    fn every_visible_slot_mod_draws_something() {
        let e = econ();
        let look = drawn_by_cosmetic_look();
        let mut missing = Vec::new();
        for m in e.catalog.iter() {
            if VISIBLE_SLOTS.contains(&m.slot)
                && !HIDDEN.contains(&m.id.as_str())
                && mod_parts(m).is_empty()
                && !changes_wheels(m)
                && !look.contains(&m.id)
            {
                missing.push(m.id.clone());
            }
        }
        assert!(missing.is_empty(), "mods with no visual: {missing:?}");
    }

    #[test]
    fn bolt_on_hardware_is_drawn() {
        // Parts you would see walking round the car, outside the purely visual slots.
        let e = econ();
        let expect = [
            ("brakes_kit_1", PartKind::BrakeKit),
            ("brakes_kit_2", PartKind::BrakeKit),
            ("prot_cage_1", PartKind::RollCage),
            ("prot_cage_2", PartKind::RollCage),
            ("prot_bumper_front", PartKind::BullBar),
            ("prot_bumper_rear", PartKind::RearBar),
            ("prot_skid_plate", PartKind::SkidPlate),
            ("prot_radiator_guard", PartKind::RadiatorGuard),
            ("prot_armor_plate", PartKind::SideArmor),
            ("prot_push_bar", PartKind::PushBar),
            ("engine_exhaust_1", PartKind::Exhaust),
            ("engine_exhaust_2", PartKind::Exhaust),
            ("cooling_intercooler_1", PartKind::Intercooler),
            ("fi_supercharger_1", PartKind::Supercharger),
            ("fi_supercharger_2", PartKind::Supercharger),
            ("weight_carbon_hood", PartKind::HoodPanel),
            ("weight_carbon_trunk", PartKind::TrunkPanel),
        ];
        for (id, kind) in expect {
            let m = e.catalog.get(id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(
                mod_parts(m).iter().any(|(k, _)| *k == kind),
                "{id} should draw {kind:?}"
            );
        }
    }

    #[test]
    fn styles_and_regions_are_known() {
        let e = econ();
        for m in e.catalog.iter() {
            for (kind, style) in mod_parts(m) {
                assert!(
                    kind.accepts(&style),
                    "{}: {kind:?} has no style {style:?}",
                    m.id
                );
            }
        }
        // Body parts all name a region; wheel / material parts none.
        for k in PartKind::ALL {
            let wheel_or_material = matches!(
                k,
                PartKind::Rims
                    | PartKind::Tires
                    | PartKind::BrakeKit
                    | PartKind::Paint
                    | PartKind::Vinyl
                    | PartKind::WindowTint
                    | PartKind::InteriorTrim
                    | PartKind::Lights
            );
            assert_eq!(k.regions().is_empty(), wheel_or_material, "{k:?}");
        }
    }

    #[test]
    fn build_reflects_wheels_aero_and_ride_height() {
        let e = econ();
        let car = wp_contracts::load_car(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/cars/kestrel_ts.json"),
        )
        .expect("car");
        let ids: Vec<String> = [
            "wheels_forged",
            "tires_semi_slick",
            "tires_wide",
            "aero_splitter",
            "aero_wing_2",
            "susp_coilovers_2",
            "brakes_kit_1",
        ]
        .map(String::from)
        .to_vec();
        let v = e
            .build_visuals(&car.def, &car.lattice, &ids)
            .expect("visuals");
        assert_eq!(v.style(PartKind::Rims), Some("forged_5spoke"));
        assert_eq!(v.style(PartKind::Tires), Some("semi_slick"));
        assert_eq!(v.style(PartKind::RearWing), Some("gt"));
        assert_eq!(v.style(PartKind::FrontSplitter), Some("splitter"));
        assert_eq!(v.style(PartKind::BrakeKit), Some("street"));
        let wing = v
            .parts
            .iter()
            .find(|p| p.part == PartKind::RearWing)
            .unwrap();
        assert_eq!(wing.regions, vec![BodyRegion::Trunk]);
        assert_eq!(wing.mod_id, "aero_wing_2");
        assert_eq!(v.wheels.len(), car.def.wheels.len());
        for w in &v.wheels {
            assert!((w.width_m - w.base_width_m - 0.02).abs() < 1e-6, "{w:?}");
            assert_eq!(w.tire_id, "semi_slick");
        }
        assert!(
            (v.ride_height_m + 0.035).abs() < 1e-6,
            "{}",
            v.ride_height_m
        );
        // Stock car: nothing fitted, stock wheels.
        let stock = e.build_visuals(&car.def, &car.lattice, &[]).unwrap();
        assert!(stock.parts.is_empty());
        assert_eq!(stock.ride_height_m, 0.0);
        assert!(stock.wheels.iter().all(|w| w.width_m == w.base_width_m));
    }

    #[test]
    fn unknown_style_is_refused_at_load() {
        let pack = r#"{"id": "x", "mods": [{"id": "x_wing", "name": "Wing", "slot": "Aero",
            "line": "w", "tier": 1, "price": 10, "effects": [{"Vehicle": {"param":
            "aero.downforce_rear", "op": "Add", "value": 0.1}}], "visuals": [{"part":
            "RearWing", "style": "dragon"}], "tradeoff": "t"}]}"#;
        let p: crate::mods::ModPack = serde_json::from_str(pack).unwrap();
        let err = crate::ModCatalog::from_packs(vec![p])
            .unwrap_err()
            .to_string();
        assert!(err.contains("dragon"), "{err}");
    }
}
