//! Police styles (owner: wave-2 `style-police`). Spec: `docs/design/vehicles/research_police.md`.
//!
//! Five versions of the fictional Port Vesper Police Department (PVPD) fleet:
//!
//! | Car | Version | Bar | Livery scheme (`render/livery.ts`) |
//! |---|---|---|---|
//! | `sentinel_cruiser` | "sedan", 2010s full-size | low LED (L-2010) | `pvpd_bw` black / white doors + roof |
//! | `sentinel_warden` | "old sedan", boxy 80s | twin rotators (L-80s), 2 spotlights | `pvpd_bw_classic` |
//! | `sentinel_wolfhound` | "patrol", AWD pursuit sedan | low LED, full width | `pvpd_highway` tan / umber + gold |
//! | `sentinel_bastion` | "SUV" | wide LED, set forward | `pvpd_suv` white + navy + cyan band |
//! | `sentinel_vigil` | "sports patrol", muscle coupe | slicktop (hidden LEDs) | `pvpd_pursuit` diagonal split |
//!
//! Equipment (light bars, push bars, spotlights, antennas, hidden LEDs, partitions, spoilers,
//! unit-number decal carriers) lives in [`kit`]; each item is a [`Shape::Custom`] detail that
//! reads the per-car [`kit::Kit`] table by car id and places itself on the styled profile.
//! Light-bar lenses are separate left (x < 0) / right parts so the renderer flashes the two
//! sides independently. Unit-number decal carriers (`decal_unit_l/_r`, `decal_roof`,
//! `decal_trunk`) are thin `livery` patches the renderer re-maps onto a per-unit number texture.

pub mod body;
pub mod kit;

use wp_contracts::{BodyRegion, VehicleDef};

use crate::mesh::{MaterialSlot, PartKind};
use crate::style::{
    ArchShape, BodyStyle, CapKind, CarStyle, Detail, EngineKind, ExhaustKind, Induction,
    LiveryHint, RimKind, Shape, SideSculpt, Silhouette, SuspensionKind, TrimFinish,
};

/// Style for a police car id, or `None` if this module does not own it.
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    let (mut s, body_kit): (CarStyle, crate::style::DetailFn) = match def.id.as_str() {
        "sentinel_cruiser" => (cruiser(), body::cruiser),
        "sentinel_warden" => (warden(), body::warden),
        "sentinel_wolfhound" => (wolfhound(), body::wolfhound),
        "sentinel_bastion" => (bastion(), body::bastion),
        "sentinel_vigil" => (vigil(), body::vigil),
        _ => return None,
    };
    // The car's own body kit (lamps, grille, bumpers, mirrors) goes first; the police
    // equipment sits on top of it.
    s.details.push(Detail::new(
        "body_kit",
        BodyRegion::Cabin,
        MaterialSlot::Trim,
        Shape::Custom(body_kit),
    ));
    add_kit(&mut s, &kit::kit(&def.id));
    Some(s)
}

/// Livery hint: scheme id + its colour tokens (sRGB) for `render/livery.ts`.
fn livery(scheme: &str, colors: &[u32], text: &[&str]) -> Option<LiveryHint> {
    Some(LiveryHint {
        scheme: scheme.to_string(),
        colors: colors
            .iter()
            .map(|c| [(c >> 16) as u8, (c >> 8) as u8, *c as u8])
            .collect(),
        text: text.iter().map(|t| t.to_string()).collect(),
    })
}

/// Plain black calipers (police fleet).
const BLACK_CALIPER: [u8; 3] = [24, 24, 26];

/// §1.1 "sedan": 2010s full-size RWD V8 sedan.
fn cruiser() -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.20,
        nose_h: Some(0.74),
        nose_rake_deg: 4.0,
        nose_plan_round: 0.20,
        tail_plan_round: 0.22,
        hood_crown: 0.02,
        hood_bulge_h: 0.012,
        hood_bulge_w: 0.50,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 6.0,
        rear_glass_rake_deg: Some(63.0),
        roof_dz: -0.06,
        belt_dz: 0.08,
        tumblehome: Some(0.14),
        crease_z: 0.60,
        crease_depth: 0.015,
        haunch: 0.04,
        flare: 0.012,
        ride_dz: 0.01,
        ducktail_h: 0.02,
        belt_kick: 0.07,
        tail_round: 0.18,
        tail_drop: 0.04,
        overhang_trim: (0.0, 0.03),
        c_pillar_w_m: 0.20,
        a_sail_len_m: 0.12,
        roof_len_trim: -0.05,
        // Phase 3: rounded section, tucked sill, rear hips, a crisp shoulder line.
        side_bulge_m: 0.02,
        rocker_in_m: 0.03,
        rear_fender_rise_m: 0.03,
        side_sculpt: vec![SideSculpt {
            y0_m: -2.4,
            y1_m: 2.4,
            z0_m: 0.87,
            z1_m: 0.9,
            dx_m: 0.02,
            falloff_m: 0.035,
        }],
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.livery = livery(
        "pvpd_bw",
        &[0x111317, 0xF3F4F6, 0xB8BCC2],
        &["POLICE", "PORT VESPER", "DIAL 911"],
    );
    steel_wheels(&mut s, 5, CapKind::Small);
    s.mechanical.engine = EngineKind::V8;
    s.mechanical.exhaust = ExhaustKind::Dual;
    s.mechanical.caliper_rgb = Some(BLACK_CALIPER);
    s
}

/// §1.2 "old sedan": boxy 80s body-on-frame V8 sedan.
fn warden() -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.76),
        nose_rake_deg: 1.0,
        nose_plan_round: 0.04,
        tail_plan_round: 0.04,
        hood_crown: 0.005,
        windshield_rake_deg: Some(54.0),
        rear_glass_rake_deg: Some(32.0),
        roof_dz: -0.02,
        roof_crown: 0.008,
        tumblehome: Some(0.10),
        glass_inset: 0.07,
        crease_z: 0.56,
        crease_depth: 0.006,
        sill_tuck: 0.025,
        shoulder_round: 0.015,
        flare_shape: ArchShape::Box,
        ride_dz: 0.03,
        stance_pitch_deg: -0.3,
        tail_round: 0.05,
        tail_drop: 0.02,
        overhang_trim: (0.05, 0.06),
        a_pillar_w_m: 0.065,
        c_pillar_w_m: 0.24,
        drip_rail: TrimFinish::Chrome,
        end_round_m: 0.012,
        arch_lip_m: 0.010,
        glass_frame_m: 0.022,
        // Phase 3: slab sides with a sharp shoulder crease and a slightly tucked sill.
        rocker_in_m: 0.02,
        side_sculpt: vec![SideSculpt {
            y0_m: -2.9,
            y1_m: 2.5,
            z0_m: 0.79,
            z1_m: 0.82,
            dx_m: 0.015,
            falloff_m: 0.03,
        }],
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.livery = livery(
        "pvpd_bw_classic",
        &[0x16171A, 0xEEEEE8],
        &["POLICE", "PORT VESPER"],
    );
    steel_wheels(&mut s, 5, CapKind::DogDish);
    s.wheel.rim = RimKind::DogDish;
    s.wheel.tire_aspect = Some(0.70);
    s.mechanical.engine = EngineKind::V8;
    s.mechanical.rear_suspension = SuspensionKind::SolidAxleCoil;
    s.mechanical.exhaust = ExhaustKind::Single;
    s.mechanical.rear_brakes.caliper = crate::style::CaliperKind::Drum;
    s.mechanical.caliper_rgb = Some(BLACK_CALIPER);
    s
}

/// §1.3 "patrol": AWD twin-turbo V6 pursuit sedan, highway-patrol livery.
fn wolfhound() -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.86),
        nose_rake_deg: 10.0,
        nose_plan_round: 0.28,
        hood_crown: 0.03,
        hood_bulge_h: 0.012,
        hood_bulge_w: 0.60,
        windshield_rake_deg: Some(58.0),
        a_pillar_rake_deg: 7.0,
        rear_glass_rake_deg: Some(50.0),
        roof_len_trim: 0.12,
        roof_dz: -0.07,
        belt_dz: 0.08,
        tumblehome: Some(0.15),
        crease_z: 0.62,
        crease_depth: 0.015,
        haunch: 0.025,
        flare: 0.015,
        ride_dz: 0.02,
        belt_rake: 0.02,
        belt_kick: 0.04,
        ducktail_h: 0.04,
        tail_round: 0.22,
        tail_drop: 0.05,
        overhang_trim: (0.0, -0.03),
        c_pillar_w_m: 0.22,
        cabin_shift_m: 0.10,
        // Phase 3: tall rounded 2010s section, tucked sill, nose and tail taper in plan.
        side_bulge_m: 0.03,
        side_bulge_z: 0.6,
        rocker_in_m: 0.04,
        plan_width: vec![[-2.635, 0.94], [-1.86, 1.0], [1.86, 1.0], [2.485, 0.93]],
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.livery = livery(
        "pvpd_highway",
        &[0xCDBB8E, 0x2B241C, 0xC99A2E, 0xD9AA3A],
        &["HIGHWAY PATROL", "PORT VESPER"],
    );
    steel_wheels(&mut s, 5, CapKind::Small);
    s.mechanical.engine = EngineKind::V6;
    s.mechanical.induction = Induction::Turbo;
    s.mechanical.exhaust = ExhaustKind::Dual;
    s.mechanical.caliper_rgb = Some(BLACK_CALIPER);
    s
}

/// §1.4 "SUV": full-size body-on-frame SUV.
fn bastion() -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(1.10),
        nose_rake_deg: 3.0,
        nose_plan_round: 0.08,
        tail_plan_round: 0.10,
        hood_crown: 0.012,
        hood_bulge_h: 0.015,
        hood_bulge_w: 0.70,
        windshield_rake_deg: Some(57.0),
        a_pillar_rake_deg: 2.0,
        rear_glass_rake_deg: Some(12.0),
        roof_crown: 0.015,
        tumblehome: Some(0.06),
        crease_z: 0.95,
        crease_depth: 0.012,
        sill_tuck: 0.04,
        shoulder_round: 0.04,
        haunch: 0.02,
        flare: 0.045,
        ride_dz: 0.04,
        tail_round: 0.05,
        tail_drop: 0.02,
        end_round_m: 0.02,
        c_pillar_w_m: 0.14,
        a_sail_len_m: 0.16,
        // Phase 3: near-vertical side glass, fender shoulders over the hood, doors set in a
        // little between the flared fenders.
        tumblehome_m: Some(0.05),
        glass_inset: 0.04,
        front_fender_rise_m: 0.04,
        plan_width: vec![[-1.0, 1.0], [-0.55, 0.965], [0.55, 0.965], [1.0, 1.0]],
        ..BodyStyle::preset(Silhouette::Suv)
    });
    s.livery = livery(
        "pvpd_suv",
        &[0xF2F4F5, 0x14223F, 0x1E7FD0, 0x2FA15A],
        &["POLICE", "PORT VESPER", "DIAL 911"],
    );
    steel_wheels(&mut s, 6, CapKind::Centre);
    s.wheel.rim = RimKind::Steel;
    s.wheel.pcd_r_m = 0.0695;
    s.mechanical.engine = EngineKind::V8;
    s.mechanical.rear_suspension = SuspensionKind::SolidAxleCoil;
    s.mechanical.exhaust = ExhaustKind::Single;
    s.mechanical.caliper_rgb = Some(BLACK_CALIPER);
    s
}

/// §1.5 "sports patrol": modern muscle-coupe pursuit car, slicktop.
fn vigil() -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.28,
        nose_h: Some(0.64),
        nose_rake_deg: 4.0,
        nose_plan_round: 0.18,
        tail_plan_round: 0.18,
        hood_crown: 0.02,
        hood_bulge_h: 0.045,
        hood_bulge_w: 0.40,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 10.0,
        roof_len_trim: -0.05,
        roof_dz: -0.10,
        belt_dz: 0.05,
        tumblehome: Some(0.20),
        crease_z: 0.58,
        crease_depth: 0.02,
        haunch: 0.09,
        front_haunch: 0.02,
        flare: 0.025,
        ride_dz: -0.02,
        stance_pitch_deg: 0.2,
        belt_kick: 0.06,
        ducktail_h: 0.03,
        tail_round: 0.14,
        tail_drop: 0.03,
        overhang_trim: (-0.02, -0.04),
        c_pillar_w_m: 0.22,
        // Phase 3: coke-bottle waist, raised front fenders and rear haunches, a cove behind the
        // front wheel.
        plan_width: vec![[-1.0, 1.0], [-0.6, 0.95], [0.5, 0.95], [0.95, 1.0]],
        front_fender_rise_m: 0.03,
        rear_fender_rise_m: 0.05,
        side_bulge_m: 0.02,
        side_sculpt: vec![SideSculpt {
            y0_m: 0.35,
            y1_m: 0.9,
            z0_m: 0.3,
            z1_m: 0.55,
            dx_m: -0.035,
            falloff_m: 0.12,
        }],
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.livery = livery(
        "pvpd_pursuit",
        &[0x0E0F12, 0xF4F5F7, 0x1C6CFF],
        &["POLICE", "PURSUIT", "PORT VESPER", "DIAL 911"],
    );
    s.wheel.rim = RimKind::SplitSpoke;
    s.wheel.spokes = 5;
    s.wheel.rim_material = MaterialSlot::Trim;
    s.wheel.cap = CapKind::Small;
    s.wheel.face_depth_m = 0.035;
    s.wheel.rim_radius_m = None;
    s.mechanical.engine = EngineKind::V8;
    s.mechanical.induction = Induction::Blower;
    s.mechanical.exhaust = ExhaustKind::Quad;
    s.mechanical.front_brakes.caliper = crate::style::CaliperKind::Fixed6;
    s.mechanical.rear_brakes.caliper = crate::style::CaliperKind::Fixed4;
    s.mechanical.caliper_rgb = Some([0x20, 0x50, 0xC0]);
    s
}

/// Satin-black police steel wheels.
fn steel_wheels(s: &mut CarStyle, lugs: u8, cap: CapKind) {
    s.wheel.rim = RimKind::Steel;
    s.wheel.spokes = 0;
    s.wheel.cap = cap;
    s.wheel.lugs = lugs;
    s.wheel.rim_material = MaterialSlot::Trim;
    s.wheel.face_depth_m = 0.03;
}

/// Push every equipment detail the car's kit asks for.
fn add_kit(s: &mut CarStyle, k: &kit::Kit) {
    use kit::*;
    let d = |name: &str, region: BodyRegion, mat: MaterialSlot, f: crate::style::DetailFn| {
        Detail::new(name, region, mat, Shape::Custom(f))
    };
    let lb = |x: Detail| x.kind(PartKind::Lightbar);
    match k.bar {
        Bar::Led { .. } => s.details.push(lb(d(
            "lightbar",
            BodyRegion::Roof,
            MaterialSlot::Trim,
            led_bar,
        ))),
        Bar::Rotator => s.details.push(lb(d(
            "lightbar",
            BodyRegion::Roof,
            MaterialSlot::Trim,
            rotator_bar,
        ))),
        Bar::Slick => {}
    }
    if k.push.is_some() {
        s.details.push(d(
            "pushbar",
            BodyRegion::Accessory,
            MaterialSlot::Grille,
            push_bar,
        ));
    }
    let spot = d(
        "spotlight_l",
        BodyRegion::Cabin,
        if k.spot.chrome {
            MaterialSlot::Chrome
        } else {
            MaterialSlot::Trim
        },
        spotlight,
    );
    s.details.push(spot); // mirrors itself when the kit has two
    if !k.pucks.is_empty() {
        s.details.push(d(
            "antenna_pucks",
            BodyRegion::Roof,
            MaterialSlot::Trim,
            pucks,
        ));
    }
    if !k.whips.is_empty() {
        s.details.push(d(
            "antenna_whip",
            BodyRegion::Trunk,
            MaterialSlot::Trim,
            whips,
        ));
    }
    if k.visor {
        s.details.push(lb(d(
            "visor_bar",
            BodyRegion::Cabin,
            MaterialSlot::Trim,
            visor_bar,
        )));
    }
    if k.rear_bar {
        s.details.push(lb(d(
            "rear_window_bar",
            BodyRegion::Cabin,
            MaterialSlot::Trim,
            rear_window_bar,
        )));
    }
    if k.advisor {
        s.details.push(lb(d(
            "traffic_advisor",
            BodyRegion::Cabin,
            MaterialSlot::Trim,
            traffic_advisor,
        )));
    }
    if k.grille_leds {
        s.details.push(lb(d(
            "grille_leds",
            BodyRegion::FrontBumper,
            MaterialSlot::Grille,
            grille_leds,
        )));
    }
    match k.spoiler {
        Spoiler::None => {}
        Spoiler::Lip => s.details.push(d(
            "spoiler",
            BodyRegion::Trunk,
            MaterialSlot::Livery,
            spoiler_lip,
        )),
        Spoiler::Wing => s.details.push(d(
            "spoiler",
            BodyRegion::Trunk,
            MaterialSlot::Carbon,
            spoiler_wing,
        )),
    }
    if k.splitter {
        s.details.push(d(
            "splitter",
            BodyRegion::FrontBumper,
            MaterialSlot::Carbon,
            splitter,
        ));
    }
    if k.rub_strip {
        s.details.push(d(
            "rub_strip",
            BodyRegion::LeftDoorF,
            MaterialSlot::Chrome,
            rub_strip,
        ));
    }
    if k.tow_hooks {
        s.details.push(d(
            "tow_hook_l",
            BodyRegion::FrameFront,
            MaterialSlot::Metal,
            tow_hook,
        ));
    }
    s.details.push(d(
        "partition",
        BodyRegion::Cabin,
        MaterialSlot::Metal,
        partition,
    ));
    s.details.push(
        d(
            "decal_unit_l",
            k.unit_region,
            MaterialSlot::Livery,
            decal_side,
        )
        .kind(PartKind::DecalCarrier)
        .mirrored(),
    );
    s.details.push(
        d(
            "decal_roof",
            BodyRegion::Roof,
            MaterialSlot::Livery,
            decal_roof,
        )
        .kind(PartKind::DecalCarrier),
    );
    if k.trunk_number {
        s.details.push(
            d(
                "decal_trunk",
                BodyRegion::Trunk,
                MaterialSlot::Livery,
                decal_trunk,
            )
            .kind(PartKind::DecalCarrier),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARS: [&str; 5] = [
        "sentinel_cruiser",
        "sentinel_warden",
        "sentinel_wolfhound",
        "sentinel_bastion",
        "sentinel_vigil",
    ];

    fn mesh(id: &str) -> crate::mesh::CarMesh {
        let car = crate::load_car(&crate::repo_root(), id).expect("car");
        crate::mesh::generate_styled(
            &car.def,
            &car.lattice,
            &style(&car.def).expect("police style"),
        )
    }

    #[test]
    fn every_police_car_has_a_style_with_livery_and_equipment() {
        for id in CARS {
            let m = mesh(id);
            assert!(m.livery.is_some(), "{id}: livery hint");
            let has = |n: &str| m.parts.iter().any(|p| p.name == n);
            for n in ["decal_unit_l", "decal_unit_r", "decal_roof", "partition"] {
                assert!(has(n), "{id}: missing {n}");
            }
            assert!(
                (8000..=22000).contains(&m.tri_count),
                "{id}: {} tris",
                m.tri_count
            );
        }
    }

    #[test]
    fn light_lenses_are_split_left_right() {
        for id in CARS {
            let m = mesh(id);
            let mut sides = (0, 0);
            for p in m.parts.iter().filter(|p| {
                matches!(
                    p.material,
                    MaterialSlot::LightbarRed | MaterialSlot::LightbarBlue
                )
            }) {
                let n = p.positions_mm.len() / 3;
                let cx: i64 = p.positions_mm.iter().step_by(3).map(|&x| x as i64).sum();
                let cx = cx / n.max(1) as i64;
                // Every lens part sits entirely on one side of x = 0.
                let all_left = p.positions_mm.iter().step_by(3).all(|&x| x <= 0);
                let all_right = p.positions_mm.iter().step_by(3).all(|&x| x >= 0);
                assert!(all_left || all_right, "{id}: {} straddles x = 0", p.name);
                if cx < 0 {
                    sides.0 += 1;
                } else {
                    sides.1 += 1;
                }
            }
            assert!(sides.0 > 0 && sides.1 > 0, "{id}: lenses {sides:?}");
        }
    }

    #[test]
    fn slicktop_has_no_roof_bar_and_rotator_has_domes() {
        assert!(!mesh("sentinel_vigil")
            .parts
            .iter()
            .any(|p| p.name.starts_with("lightbar")));
        assert!(mesh("sentinel_warden")
            .parts
            .iter()
            .any(|p| p.name == "lightbar_dome_l"));
    }
}
