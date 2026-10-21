//! The free per-car tune (reference-game style tune screen, parity gap G10): four sliders of
//! five steps — suspension, gearing, differential, brake balance — with step 3 = stock.
//!
//! [`apply`] maps a [`CarTune`] onto a car's `VehicleDef` and its [`DiffSetup`] once, before
//! the vehicle is built ([`tuned`] does both from the car file). It is deterministic, costs
//! nothing per tick, and Arcade and Simulation use the same path. The step tables live in
//! `data/tune/steps.toml` ([`TuneSteps`], embedded at build time like the differential
//! table); the file header documents every row.
//!
//! What each slider changes (and nothing else):
//!
//! | slider | fields |
//! |---|---|
//! | suspension 1 soft … 5 stiff | `spring_n_per_m`, `arb_n_per_m` × scale; both dampers × √scale |
//! | gearing 1 short … 5 long | `drivetrain.final_drive` × scale |
//! | diff 1 open … 3 stock … 5 locked | the driven axle's [`AxleDiff`] (rear for RWD / AWD, front for FWD) |
//! | brake balance 1 rear … 5 front | `brakes.bias_front` + offset, clamped |
//!
//! A slider at stock leaves its fields bit-for-bit untouched, so a stock tune builds exactly
//! the car the file describes.
//!
//! [`CarTune`] is defined here until the integration lead moves it into `wp_contracts`
//! (request WP-4 in `docs/integration/tune.md`); its serde shape is the proposed contract's.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use wp_contracts::{DriveLayout, VehicleDef};

use crate::diff::{AxleDiff, DiffSetup};

/// Number of steps on each slider.
pub const TUNE_STEPS: u8 = 5;

/// Stock position of every slider.
pub const TUNE_STOCK: u8 = 3;

/// The player's tune of one car: four sliders, each `1..=5`, 3 = stock. Out-of-range values
/// are clamped when applied ([`CarTune::clamped`]). Missing fields read as stock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarTune {
    /// Suspension, 1 soft … 5 stiff.
    #[serde(default = "tune_stock")]
    pub suspension: u8,
    /// Gear ratio, 1 short … 5 long.
    #[serde(default = "tune_stock")]
    pub gearing: u8,
    /// Differential, 1 open … 3 stock … 5 locked.
    #[serde(default = "tune_stock")]
    pub diff: u8,
    /// Brake balance, 1 rear … 5 front.
    #[serde(default = "tune_stock")]
    pub brake_balance: u8,
}

fn tune_stock() -> u8 {
    TUNE_STOCK
}

impl CarTune {
    /// Every slider at stock.
    pub const STOCK: Self = Self {
        suspension: TUNE_STOCK,
        gearing: TUNE_STOCK,
        diff: TUNE_STOCK,
        brake_balance: TUNE_STOCK,
    };

    /// True when applying the tune changes nothing (every slider at stock after clamping).
    pub fn is_stock(&self) -> bool {
        self.clamped() == Self::STOCK
    }

    /// Every slider clamped to `1..=TUNE_STEPS`.
    pub fn clamped(self) -> Self {
        let c = |v: u8| v.clamp(1, TUNE_STEPS);
        Self {
            suspension: c(self.suspension),
            gearing: c(self.gearing),
            diff: c(self.diff),
            brake_balance: c(self.brake_balance),
        }
    }

    /// True when every slider is in `1..=TUNE_STEPS`.
    pub fn is_valid(&self) -> bool {
        *self == self.clamped()
    }
}

impl Default for CarTune {
    fn default() -> Self {
        Self::STOCK
    }
}

/// The differential rows of [`TuneSteps`] (see `data/tune/steps.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffSteps {
    /// Clutch preload used when the stock diff is not a clutch LSD, N·m.
    pub base_preload_nm: f32,
    /// Clutch drive ramp used when the stock diff is not a clutch LSD, 0..=1.
    pub base_accel: f32,
    /// Clutch coast ramp used when the stock diff is not a clutch LSD, 0..=1.
    pub base_decel: f32,
    /// Step 2: preload scale.
    pub light_preload_scale: f32,
    /// Step 2: drive ramp scale.
    pub light_accel_scale: f32,
    /// Step 2: coast ramp scale.
    pub light_decel_scale: f32,
    /// Step 4: preload scale.
    pub tight_preload_scale: f32,
    /// Step 4: drive ramp floor.
    pub tight_accel_min: f32,
    /// Step 4: coast ramp floor.
    pub tight_decel_min: f32,
}

/// One car's overrides in `data/tune/steps.toml` (`[cars.<id>]`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarSteps {
    /// Suspension scales, step 1 first.
    #[serde(default)]
    pub suspension: Option<[f32; 5]>,
    /// Final-drive scales, step 1 first.
    #[serde(default)]
    pub gearing: Option<[f32; 5]>,
    /// Front brake-bias offsets, step 1 first.
    #[serde(default)]
    pub brake_balance: Option<[f32; 5]>,
}

/// The step tables of `data/tune/steps.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuneSteps {
    /// Spring / anti-roll bar scale per suspension step (dampers get the square root).
    pub suspension: [f32; 5],
    /// Final-drive scale per gearing step (long = numerically lower).
    pub gearing: [f32; 5],
    /// Front brake-bias offset per balance step.
    pub brake_balance: [f32; 5],
    /// Lowest front bias a tune can reach.
    pub bias_min: f32,
    /// Highest front bias a tune can reach.
    pub bias_max: f32,
    /// Differential rows.
    pub diff: DiffSteps,
    /// Per-car overrides keyed by car id.
    #[serde(default)]
    pub cars: BTreeMap<String, CarSteps>,
}

/// The step tables embedded at build time from `data/tune/steps.toml`.
pub const TUNE_STEPS_TOML: &str = include_str!("../../../data/tune/steps.toml");

impl TuneSteps {
    /// Parse and check a step table: every list's stock entry must be the identity, scales
    /// must be positive, and `bias_min < bias_max` inside 0..1.
    pub fn parse(text: &str) -> Result<Self, String> {
        let s: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        s.check("", &s.suspension, &s.gearing, &s.brake_balance)?;
        for (id, c) in &s.cars {
            s.check(
                id,
                &c.suspension.unwrap_or(s.suspension),
                &c.gearing.unwrap_or(s.gearing),
                &c.brake_balance.unwrap_or(s.brake_balance),
            )?;
        }
        if !(0.0 < s.bias_min && s.bias_min < s.bias_max && s.bias_max < 1.0) {
            return Err(format!(
                "bias_min {} / bias_max {} must satisfy 0 < min < max < 1",
                s.bias_min, s.bias_max
            ));
        }
        Ok(s)
    }

    fn check(
        &self,
        car: &str,
        susp: &[f32; 5],
        gear: &[f32; 5],
        bias: &[f32; 5],
    ) -> Result<(), String> {
        let at = |what: &str| {
            if car.is_empty() {
                what.to_string()
            } else {
                format!("cars.{car}.{what}")
            }
        };
        let stock = (TUNE_STOCK - 1) as usize;
        for (name, list) in [("suspension", susp), ("gearing", gear)] {
            if list[stock] != 1.0 {
                return Err(format!("{}: step 3 must be 1.0 (stock)", at(name)));
            }
            if list.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
                return Err(format!("{}: scales must be positive", at(name)));
            }
        }
        if bias[stock] != 0.0 {
            return Err(format!(
                "{}: step 3 must be 0.0 (stock)",
                at("brake_balance")
            ));
        }
        Ok(())
    }

    /// The suspension, gearing and brake-balance lists for car `id` (its overrides, else the
    /// shared rows).
    pub fn for_car(&self, id: &str) -> ([f32; 5], [f32; 5], [f32; 5]) {
        let c = self.cars.get(id).copied().unwrap_or_default();
        (
            c.suspension.unwrap_or(self.suspension),
            c.gearing.unwrap_or(self.gearing),
            c.brake_balance.unwrap_or(self.brake_balance),
        )
    }
}

/// The embedded step tables (a unit test guards that they parse).
pub fn tune_steps() -> &'static TuneSteps {
    static STEPS: OnceLock<TuneSteps> = OnceLock::new();
    STEPS.get_or_init(|| TuneSteps::parse(TUNE_STEPS_TOML).expect("data/tune/steps.toml parses"))
}

/// The differential the tune gives an axle whose stock diff is `stock` (`step` 1..=5).
pub fn tuned_axle(stock: AxleDiff, step: u8, d: &DiffSteps) -> AxleDiff {
    let (pre, accel, decel) = match stock {
        AxleDiff::Clutch {
            preload_nm,
            accel,
            decel,
        } => (preload_nm, accel, decel),
        _ => (d.base_preload_nm, d.base_accel, d.base_decel),
    };
    match step.clamp(1, TUNE_STEPS) {
        1 => AxleDiff::Open,
        2 => AxleDiff::Clutch {
            preload_nm: pre * d.light_preload_scale,
            accel: (accel * d.light_accel_scale).clamp(0.0, 1.0),
            decel: (decel * d.light_decel_scale).clamp(0.0, 1.0),
        },
        3 => stock,
        4 => AxleDiff::Clutch {
            preload_nm: pre * d.tight_preload_scale,
            accel: accel.max(d.tight_accel_min).clamp(0.0, 1.0),
            decel: decel.max(d.tight_decel_min).clamp(0.0, 1.0),
        },
        _ => AxleDiff::Locked,
    }
}

/// Apply `tune` to a car's definition and differential setup (see the module docs). `diffs`
/// must be the setup the car would otherwise spawn with ([`DiffSetup::for_car`]). Sliders at
/// stock leave their fields untouched.
pub fn apply(def: &mut VehicleDef, diffs: &mut DiffSetup, tune: CarTune, steps: &TuneSteps) {
    let t = tune.clamped();
    let i = |v: u8| (v - 1) as usize;
    let (susp, gear, bias) = steps.for_car(&def.id);
    if t.suspension != TUNE_STOCK {
        let k = susp[i(t.suspension)];
        let kd = k.sqrt();
        for s in &mut def.suspension {
            s.spring_n_per_m *= k;
            s.arb_n_per_m *= k;
            s.damper_bump_ns_per_m *= kd;
            s.damper_rebound_ns_per_m *= kd;
        }
    }
    if t.gearing != TUNE_STOCK {
        def.drivetrain.final_drive *= gear[i(t.gearing)];
    }
    if t.brake_balance != TUNE_STOCK {
        def.brakes.bias_front = (def.brakes.bias_front + bias[i(t.brake_balance)])
            .clamp(steps.bias_min, steps.bias_max);
    }
    if t.diff != TUNE_STOCK {
        if matches!(def.drivetrain.layout, DriveLayout::Fwd) {
            diffs.front = tuned_axle(diffs.front, t.diff, &steps.diff);
        } else {
            diffs.rear = tuned_axle(diffs.rear, t.diff, &steps.diff);
        }
    }
}

/// The car `def` with `tune` applied, and the differential setup to build it with (pass it as
/// [`crate::VehicleExtras::diffs`]). Uses the embedded step tables.
pub fn tuned(def: &VehicleDef, tune: CarTune) -> (VehicleDef, DiffSetup) {
    let mut d = def.clone();
    let mut diffs = DiffSetup::for_car(def);
    apply(&mut d, &mut diffs, tune, tune_steps());
    (d, diffs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(id: &str) -> VehicleDef {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/cars/");
        wp_contracts::load_car(format!("{root}{id}.json"))
            .unwrap()
            .def
    }

    #[test]
    fn embedded_steps_parse() {
        let s = tune_steps();
        assert_eq!(s.suspension[2], 1.0);
        assert!(s.cars.contains_key("hauler_1500"));
    }

    #[test]
    fn stock_is_identity() {
        for id in ["rampart_50", "vireo_xs", "lindwurm_4t", "hauler_1500"] {
            let def = car(id);
            let (d, diffs) = tuned(&def, CarTune::STOCK);
            assert_eq!(d, def, "{id}");
            assert_eq!(diffs, DiffSetup::for_car(&def), "{id}");
        }
    }

    #[test]
    fn each_slider_changes_only_its_fields() {
        let def = car("rampart_50");
        let stock_diffs = DiffSetup::for_car(&def);

        let (d, diffs) = tuned(
            &def,
            CarTune {
                suspension: 5,
                ..CarTune::STOCK
            },
        );
        assert_eq!(diffs, stock_diffs);
        let k = tune_steps().suspension[4];
        for (a, b) in d.suspension.iter().zip(&def.suspension) {
            assert_eq!(a.spring_n_per_m, b.spring_n_per_m * k);
            assert_eq!(a.arb_n_per_m, b.arb_n_per_m * k);
            assert_eq!(a.damper_bump_ns_per_m, b.damper_bump_ns_per_m * k.sqrt());
            assert_eq!(
                a.damper_rebound_ns_per_m,
                b.damper_rebound_ns_per_m * k.sqrt()
            );
            assert_eq!(a.travel_m, b.travel_m);
            assert_eq!(a.bump_stop_n_per_m, b.bump_stop_n_per_m);
        }
        let mut only = d.clone();
        only.suspension = def.suspension.clone();
        assert_eq!(only, def);

        let (d, diffs) = tuned(
            &def,
            CarTune {
                gearing: 1,
                ..CarTune::STOCK
            },
        );
        assert_eq!(diffs, stock_diffs);
        assert_eq!(d.drivetrain.final_drive, def.drivetrain.final_drive * 1.12);
        let mut only = d.clone();
        only.drivetrain.final_drive = def.drivetrain.final_drive;
        assert_eq!(only, def);

        let (d, diffs) = tuned(
            &def,
            CarTune {
                brake_balance: 1,
                ..CarTune::STOCK
            },
        );
        assert_eq!(diffs, stock_diffs);
        assert!((d.brakes.bias_front - (def.brakes.bias_front - 0.10)).abs() < 1e-6);
        let mut only = d.clone();
        only.brakes.bias_front = def.brakes.bias_front;
        assert_eq!(only, def);

        let (d, diffs) = tuned(
            &def,
            CarTune {
                diff: 5,
                ..CarTune::STOCK
            },
        );
        assert_eq!(d, def);
        assert_eq!(diffs.rear, AxleDiff::Locked);
        assert_eq!(diffs.front, stock_diffs.front);
        assert_eq!(diffs.centre, stock_diffs.centre);
    }

    #[test]
    fn diff_steps_follow_the_table() {
        let d = &tune_steps().diff;
        // rampart_50: stock 1.5-way clutch (80 N·m, 0.35 / 0.20) in differentials.toml.
        let stock = DiffSetup::for_car(&car("rampart_50")).rear;
        assert_eq!(tuned_axle(stock, 1, d), AxleDiff::Open);
        assert_eq!(tuned_axle(stock, 3, d), stock);
        assert_eq!(tuned_axle(stock, 5, d), AxleDiff::Locked);
        match (tuned_axle(stock, 2, d), tuned_axle(stock, 4, d)) {
            (
                AxleDiff::Clutch {
                    preload_nm: p2,
                    accel: a2,
                    decel: d2,
                },
                AxleDiff::Clutch {
                    preload_nm: p4,
                    accel: a4,
                    decel: d4,
                },
            ) => {
                assert!(
                    (p2 - 40.0).abs() < 1e-4
                        && (a2 - 0.21).abs() < 1e-6
                        && (d2 - 0.12).abs() < 1e-6
                );
                assert!((p4 - 200.0).abs() < 1e-4 && a4 == 0.75 && d4 == 0.60);
            }
            other => panic!("{other:?}"),
        }
        // A helical stock diff scales the base clutch instead.
        let helical = DiffSetup::for_car(&car("hakone_gt")).rear;
        assert!(matches!(helical, AxleDiff::Helical { .. }));
        assert_eq!(
            tuned_axle(helical, 2, d),
            AxleDiff::Clutch {
                preload_nm: 30.0,
                accel: 0.4 * 0.6,
                decel: 0.25 * 0.6
            }
        );
    }

    #[test]
    fn fwd_changes_the_front_axle() {
        let def = car("vireo_xs");
        let (_, diffs) = tuned(
            &def,
            CarTune {
                diff: 5,
                ..CarTune::STOCK
            },
        );
        assert_eq!(diffs.front, AxleDiff::Locked);
        assert_eq!(diffs.rear, DiffSetup::for_car(&def).rear);
    }

    #[test]
    fn brake_bias_is_clamped() {
        let mut def = car("rampart_50");
        def.brakes.bias_front = 0.78;
        let (d, _) = tuned(
            &def,
            CarTune {
                brake_balance: 5,
                ..CarTune::STOCK
            },
        );
        assert_eq!(d.brakes.bias_front, tune_steps().bias_max);
    }

    #[test]
    fn per_car_override_narrows_the_truck_range() {
        let def = car("hauler_1500");
        let (d, _) = tuned(
            &def,
            CarTune {
                suspension: 5,
                ..CarTune::STOCK
            },
        );
        let k = d.suspension[0].spring_n_per_m / def.suspension[0].spring_n_per_m;
        assert!((k - 1.16).abs() < 1e-4, "{k}");
    }

    #[test]
    fn parse_rejects_a_non_identity_stock_step() {
        let bad = TUNE_STEPS_TOML.replace(
            "gearing = [1.12, 1.06, 1.00, 0.94, 0.88]",
            "gearing = [1.12, 1.06, 1.01, 0.94, 0.88]",
        );
        assert!(TuneSteps::parse(&bad).unwrap_err().contains("gearing"));
    }

    #[test]
    fn car_tune_serde_defaults_and_clamps() {
        let t: CarTune = serde_json::from_str(r#"{"gearing": 5}"#).unwrap();
        assert_eq!(
            t,
            CarTune {
                gearing: 5,
                ..CarTune::STOCK
            }
        );
        assert!(serde_json::from_str::<CarTune>(r#"{"camber": 2}"#).is_err());
        let wild = CarTune {
            suspension: 0,
            gearing: 9,
            diff: 3,
            brake_balance: 5,
        };
        assert!(!wild.is_valid());
        assert!(wild.clamped().is_valid());
        assert!(CarTune {
            suspension: 3,
            gearing: 3,
            diff: 3,
            brake_balance: 3
        }
        .is_stock());
    }
}
