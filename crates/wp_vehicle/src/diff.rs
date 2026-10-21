//! Differentials: axle (front / rear) and centre, solved once per drivetrain sub-step.
//!
//! Every diff is described by the torque `X` it moves across: the left (or front) side
//! receives `T_in·share − X`, the right (or rear) side `T_in·share + X`. A positive speed
//! difference `Δω = ω_left − ω_right` makes the internal friction push torque from the faster
//! to the slower side (`X > 0`). Each type bounds `|X|` differently, and `X` never exceeds the
//! torque that would equalise the two speeds in one sub-step (`k_eq·Δω`), so a diff that is
//! "stuck" behaves as locked and a slipping one transmits its friction capacity — a
//! stick-slip clutch model that is stable at any lock.
//!
//! | type | half-difference bound `|X|` |
//! |---|---|
//! | [`AxleDiff::Open`] | 0 (equal torque, free speed difference) |
//! | [`AxleDiff::Clutch`] | `½·(preload + lock·|T_in|)`, lock = `accel` under drive, `decel` on the overrun (engine braking): **1-way** `decel = 0`, **1.5-way** `0 < decel < accel`, **2-way** `decel = accel` |
//! | [`AxleDiff::Viscous`] | `½·min(c·|Δω|, max)` — speed-difference based, no torque sensing |
//! | [`AxleDiff::Helical`] | `½·min(L·|T_in|, (TBR−1)·R_fast) + ½·preload`, `L = (TBR−1)/(TBR+1)`; `R_fast` = torque the faster (low-traction) wheel actually resists (tire + brake). One wheel in the air resists ~0 → the helical diff acts open (the classic caveat; braking the spinning wheel restores the bias, which is how brake-based traction control helps it) |
//! | [`AxleDiff::Active`] | electronically commanded clutch: `½·min(max, preload + accel·|T_in| + gain·excess)` on drive, `½·min(max, decel·|T_in|)` on the overrun; `excess` = speed difference beyond the cornering geometry. Opens on entry for turn-in, locks a spinning inside wheel on exit |
//! | [`AxleDiff::Locked`] | unbounded (welded / spool) |
//!
//! Lock percentages use the standard definition `lock = (T_slow − T_fast)/T_in`, i.e. a torque
//! bias ratio `TBR = (1 + lock)/(1 − lock)`: 25 % ≈ TBR 1.7, 50 % ≈ TBR 3, 100 % = locked.
//!
//! Centre diffs ([`CentreDiff`]) use the same rules between the front and rear axle (speeds
//! compared at the tire surface, so unequal tire sizes do not bind). `Default` keeps the
//! layout's own behaviour (fixed torque split for `Awd`, the clutch coupling for
//! `AwdOnDemand`).
//!
//! Setups come from `data/cars/differentials.toml` (keyed by car id, owned by workstream 09
//! after this round), falling back to the contract `DiffDef`s in the car file
//! ([`DiffSetup::from_def`]); [`crate::VehicleExtras::diffs`] overrides both (tests, mods).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use wp_contracts::{DiffDef, VehicleDef};

/// One axle differential (front or rear). Lock fractions are 0..=1, torques N·m, speed
/// coefficients N·m per rad/s of left-right wheel-speed difference.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AxleDiff {
    /// Open differential: equal torque both sides.
    Open,
    /// Clutch-pack (plate) LSD with ramps: preload plus a lock fraction of the input torque,
    /// `accel` under drive, `decel` on the overrun.
    Clutch {
        /// Static preload (breakaway) torque, wheel-to-wheel difference, N·m.
        preload_nm: f32,
        /// Lock fraction under drive torque (accel ramp), 0..=1.
        accel: f32,
        /// Lock fraction under coast / engine-braking torque (decel ramp), 0..=1.
        decel: f32,
    },
    /// Viscous coupling between the sides: torque grows with the speed difference.
    Viscous {
        /// Wheel-torque difference per rad/s of speed difference, N·m/(rad/s).
        nm_per_rads: f32,
        /// Saturation of the wheel-torque difference, N·m.
        max_nm: f32,
    },
    /// Helical (worm or parallel-axis gear) torque-sensing diff.
    Helical {
        /// Torque bias ratio under drive (slow-side : fast-side torque), ≥ 1.
        tbr: f32,
        /// Torque bias ratio on the overrun (often lower), ≥ 1.
        tbr_coast: f32,
        /// Small internal-friction preload, N·m (usually 0..30).
        #[serde(default)]
        preload_nm: f32,
    },
    /// Electronically controlled clutch ("e-diff").
    Active {
        /// Maximum clutch capacity (wheel-torque difference), N·m.
        max_nm: f32,
        /// Base capacity whenever driving, N·m (straight-line stability).
        preload_nm: f32,
        /// Lock fraction of the input torque under drive, 0..=1.
        accel: f32,
        /// Lock fraction on the overrun (low: free on corner entry), 0..=1.
        decel: f32,
        /// Extra capacity per rad/s of speed difference beyond the cornering geometry,
        /// N·m/(rad/s) (locks a spinning inside wheel).
        slip_gain: f32,
    },
    /// Locked / welded / spool: both sides turn together.
    Locked,
}

/// Centre differential / coupling of an AWD car (ignored for 2WD layouts).
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CentreDiff {
    /// The layout's own behaviour: fixed torque split (`Awd`) or the on-demand clutch
    /// coupling (`AwdOnDemand`).
    #[default]
    Default,
    /// Viscous coupling on top of the static split (for `AwdOnDemand`: `base_front_split`,
    /// the clutch coupling is replaced).
    Viscous {
        /// Axle-torque difference per rad/s of (radius-normalised) axle speed difference.
        nm_per_rads: f32,
        /// Saturation, N·m.
        max_nm: f32,
    },
    /// Helical (torque-sensing) centre with a torque bias ratio around the static split.
    Helical {
        /// Torque bias ratio, ≥ 1.
        tbr: f32,
    },
    /// Locked centre (part-time 4WD): front and rear turn together.
    Locked,
}

impl CentreDiff {
    /// Short human label.
    pub fn label(&self) -> &'static str {
        match self {
            CentreDiff::Default => "layout default",
            CentreDiff::Viscous { .. } => "viscous",
            CentreDiff::Helical { .. } => "helical",
            CentreDiff::Locked => "locked",
        }
    }
}

/// The differentials of one car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffSetup {
    /// Front axle (ignored for RWD).
    pub front: AxleDiff,
    /// Rear axle (ignored for FWD).
    pub rear: AxleDiff,
    /// Centre (AWD only).
    pub centre: CentreDiff,
}

/// How a clutch LSD's ramps are usually named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClutchWays {
    /// Locks under drive only.
    OneWay,
    /// Locks under drive, less on the overrun.
    OneAndHalfWay,
    /// Locks equally both ways.
    TwoWay,
}

impl AxleDiff {
    /// From the contract `DiffDef` (car file): `Lsd { preload_nm, ramp_power, ramp_coast }` is a
    /// clutch LSD with `accel = ramp_power`, `decel = ramp_coast` (standard lock fractions).
    pub fn from_contract(d: &DiffDef) -> Self {
        match *d {
            DiffDef::Open => AxleDiff::Open,
            DiffDef::Locked => AxleDiff::Locked,
            DiffDef::Lsd {
                preload_nm,
                ramp_power,
                ramp_coast,
            } => AxleDiff::Clutch {
                preload_nm,
                accel: ramp_power,
                decel: ramp_coast,
            },
        }
    }

    /// 1 / 1.5 / 2-way classification of a clutch LSD (`None` for other types).
    pub fn clutch_ways(&self) -> Option<ClutchWays> {
        match *self {
            AxleDiff::Clutch { accel, decel, .. } => Some(if decel <= 0.0 {
                ClutchWays::OneWay
            } else if decel < accel {
                ClutchWays::OneAndHalfWay
            } else {
                ClutchWays::TwoWay
            }),
            _ => None,
        }
    }

    /// Rough steady lock under power, 0 (open) ..= 1 (locked); used to scale assists.
    pub fn power_lock(&self) -> f32 {
        match *self {
            AxleDiff::Open => 0.0,
            AxleDiff::Clutch { accel, .. } => accel.clamp(0.0, 1.0),
            AxleDiff::Viscous { .. } => 0.3,
            AxleDiff::Helical { tbr, .. } => ((tbr - 1.0) / (tbr + 1.0)).clamp(0.0, 1.0),
            AxleDiff::Active { accel, .. } => (accel + 0.2).clamp(0.0, 1.0),
            AxleDiff::Locked => 1.0,
        }
    }

    /// Short human label (telemetry, docs).
    pub fn label(&self) -> &'static str {
        match self {
            AxleDiff::Open => "open",
            AxleDiff::Clutch { .. } => match self.clutch_ways() {
                Some(ClutchWays::OneWay) => "clutch 1-way",
                Some(ClutchWays::OneAndHalfWay) => "clutch 1.5-way",
                _ => "clutch 2-way",
            },
            AxleDiff::Viscous { .. } => "viscous",
            AxleDiff::Helical { .. } => "helical",
            AxleDiff::Active { .. } => "active e-diff",
            AxleDiff::Locked => "locked",
        }
    }
}

/// What one diff needs to know for one sub-step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DiffInput {
    /// Torque entering the diff (sum of both sides), N·m, in the wheels' rotation sense.
    pub t_in: f32,
    /// Left (or front) side speed, rad/s.
    pub omega_l: f32,
    /// Right (or rear) side speed, rad/s.
    pub omega_r: f32,
    /// Torque per rad/s of `ω_l − ω_r` that would equalise the sides in one sub-step.
    pub k_eq: f32,
    /// Torque the left side's tire + brake resists, N·m (≥ 0, in the drive sense).
    pub react_l: f32,
    /// Torque the right side's tire + brake resists, N·m.
    pub react_r: f32,
    /// Speed difference `ω_l − ω_r` the cornering geometry asks for, rad/s.
    pub dw_kin: f32,
}

impl DiffInput {
    fn dw(&self) -> f32 {
        self.omega_l - self.omega_r
    }

    /// True when the input torque drives the wheels (not the overrun).
    fn driving(&self) -> bool {
        self.t_in * (self.omega_l + self.omega_r) >= 0.0
    }

    /// Reaction of the faster side (the one that would spin).
    fn react_fast(&self) -> f32 {
        if self.dw() >= 0.0 {
            self.react_l
        } else {
            self.react_r
        }
    }
}

/// Clamp the equalising torque to a half-difference capacity.
fn stick_slip(inp: &DiffInput, cap: f32) -> f32 {
    let cap = cap.max(0.0);
    (inp.k_eq * inp.dw()).clamp(-cap, cap)
}

/// Torque transfer `X` of an axle diff for one sub-step (see the module docs).
pub fn axle_transfer(diff: &AxleDiff, inp: &DiffInput) -> f32 {
    let t = inp.t_in.abs();
    match *diff {
        AxleDiff::Open => 0.0,
        AxleDiff::Locked => inp.k_eq * inp.dw(),
        AxleDiff::Clutch {
            preload_nm,
            accel,
            decel,
        } => {
            let lock = if inp.driving() { accel } else { decel };
            stick_slip(inp, 0.5 * (preload_nm + lock.clamp(0.0, 1.0) * t))
        }
        AxleDiff::Viscous {
            nm_per_rads,
            max_nm,
        } => {
            let dw = inp.dw();
            let x = 0.5 * (nm_per_rads * dw.abs()).min(max_nm);
            stick_slip(inp, x)
        }
        AxleDiff::Helical {
            tbr,
            tbr_coast,
            preload_nm,
        } => {
            let driving = inp.driving();
            let tbr = if driving { tbr } else { tbr_coast }.max(1.0);
            let lock = (tbr - 1.0) / (tbr + 1.0);
            let mut cap = lock * t;
            if driving {
                // The bias multiplies the torque the low-traction (faster) side resists.
                cap = cap.min((tbr - 1.0) * inp.react_fast().max(0.0));
            }
            stick_slip(inp, 0.5 * (cap + preload_nm))
        }
        AxleDiff::Active {
            max_nm,
            preload_nm,
            accel,
            decel,
            slip_gain,
        } => {
            let cmd = if inp.driving() {
                let excess = ((inp.dw() - inp.dw_kin).abs() - ACTIVE_SLIP_DEADBAND).max(0.0);
                preload_nm + accel * t + slip_gain * excess
            } else {
                decel * t
            };
            stick_slip(inp, 0.5 * cmd.min(max_nm))
        }
    }
}

/// Speed difference (rad/s) beyond the cornering geometry before an active diff adds
/// slip-based lock.
pub const ACTIVE_SLIP_DEADBAND: f32 = 1.0;

/// Torque transfer of a centre diff (front = "left", rear = "right"); 0 for `Default`.
pub fn centre_transfer(diff: &CentreDiff, inp: &DiffInput) -> f32 {
    match *diff {
        CentreDiff::Default => 0.0,
        CentreDiff::Locked => inp.k_eq * inp.dw(),
        CentreDiff::Viscous {
            nm_per_rads,
            max_nm,
        } => axle_transfer(
            &AxleDiff::Viscous {
                nm_per_rads,
                max_nm,
            },
            inp,
        ),
        CentreDiff::Helical { tbr } => axle_transfer(
            &AxleDiff::Helical {
                tbr,
                tbr_coast: tbr,
                preload_nm: 0.0,
            },
            inp,
        ),
    }
}

/// One car's entry in `data/cars/differentials.toml`. Missing axles fall back to the car
/// file's contract `DiffDef`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffEntry {
    /// Front axle.
    #[serde(default)]
    pub front: Option<AxleDiff>,
    /// Rear axle.
    #[serde(default)]
    pub rear: Option<AxleDiff>,
    /// Centre (AWD).
    #[serde(default)]
    pub centre: Option<CentreDiff>,
    /// Why this car has this setup (one line, analog names only).
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffFile {
    cars: BTreeMap<String, DiffEntry>,
}

/// The differential table embedded at build time from `data/cars/differentials.toml`.
pub const DIFF_TABLE_TOML: &str = include_str!("../../../data/cars/differentials.toml");

/// Parse a differential table (the embedded one is [`diff_table`]).
pub fn parse_diff_table(text: &str) -> Result<BTreeMap<String, DiffEntry>, String> {
    toml::from_str::<DiffFile>(text)
        .map(|f| f.cars)
        .map_err(|e| e.to_string())
}

/// The embedded differential table keyed by car id (empty if it fails to parse; a unit test
/// guards that).
pub fn diff_table() -> &'static BTreeMap<String, DiffEntry> {
    static TABLE: OnceLock<BTreeMap<String, DiffEntry>> = OnceLock::new();
    TABLE.get_or_init(|| parse_diff_table(DIFF_TABLE_TOML).unwrap_or_default())
}

impl DiffSetup {
    /// Only the contract `DiffDef`s of the car file (no table lookup).
    pub fn from_def(def: &VehicleDef) -> Self {
        Self {
            front: AxleDiff::from_contract(&def.drivetrain.diff_front),
            rear: AxleDiff::from_contract(&def.drivetrain.diff_rear),
            centre: CentreDiff::Default,
        }
    }

    /// The car's setup: its `differentials.toml` entry where present, else the car file.
    pub fn for_car(def: &VehicleDef) -> Self {
        let mut s = Self::from_def(def);
        if let Some(e) = diff_table().get(&def.id) {
            if let Some(f) = e.front {
                s.front = f;
            }
            if let Some(r) = e.rear {
                s.rear = r;
            }
            if let Some(c) = e.centre {
                s.centre = c;
            }
        }
        s
    }

    /// All open, default centre.
    pub const OPEN: Self = Self {
        front: AxleDiff::Open,
        rear: AxleDiff::Open,
        centre: CentreDiff::Default,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inp(t_in: f32, wl: f32, wr: f32) -> DiffInput {
        DiffInput {
            t_in,
            omega_l: wl,
            omega_r: wr,
            k_eq: 1.0e5,
            react_l: 1000.0,
            react_r: 1000.0,
            dw_kin: 0.0,
        }
    }

    #[test]
    fn open_and_locked() {
        assert_eq!(axle_transfer(&AxleDiff::Open, &inp(1000.0, 10.0, 0.0)), 0.0);
        let x = axle_transfer(&AxleDiff::Locked, &inp(1000.0, 10.0, 9.0));
        assert_eq!(x, 1.0e5);
    }

    #[test]
    fn clutch_ramps_by_direction() {
        let d = AxleDiff::Clutch {
            preload_nm: 100.0,
            accel: 0.5,
            decel: 0.0,
        };
        // Drive: ½(100 + 0.5·1000) = 300; overrun (torque against rotation): ½·100 = 50.
        assert_eq!(axle_transfer(&d, &inp(1000.0, 10.0, 0.0)), 300.0);
        assert_eq!(axle_transfer(&d, &inp(-1000.0, 10.0, 0.0)), 50.0);
        // Below the capacity the sides stick together (equalising torque only).
        let small = DiffInput {
            k_eq: 10.0,
            ..inp(1000.0, 10.0, 9.0)
        };
        assert_eq!(axle_transfer(&d, &small), 10.0);
        assert_eq!(d.clutch_ways(), Some(ClutchWays::OneWay));
        let two = AxleDiff::Clutch {
            preload_nm: 0.0,
            accel: 0.4,
            decel: 0.4,
        };
        assert_eq!(two.clutch_ways(), Some(ClutchWays::TwoWay));
        assert_eq!(two.label(), "clutch 2-way");
    }

    #[test]
    fn viscous_follows_speed_difference_only() {
        let d = AxleDiff::Viscous {
            nm_per_rads: 40.0,
            max_nm: 500.0,
        };
        assert_eq!(axle_transfer(&d, &inp(0.0, 10.0, 5.0)), 100.0);
        assert_eq!(axle_transfer(&d, &inp(3000.0, 10.0, 5.0)), 100.0);
        assert_eq!(axle_transfer(&d, &inp(0.0, 100.0, 0.0)), 250.0);
    }

    #[test]
    fn helical_bias_and_unloaded_wheel_caveat() {
        let d = AxleDiff::Helical {
            tbr: 3.0,
            tbr_coast: 2.0,
            preload_nm: 0.0,
        };
        // TBR 3 → lock 0.5 → ½·0.5·2000 = 500: slow side 1500, fast side 500 (ratio 3).
        assert_eq!(axle_transfer(&d, &inp(2000.0, 10.0, 5.0)), 500.0);
        // Fast wheel in the air: it resists nothing, so no bias (acts open).
        let air = DiffInput {
            react_l: 0.0,
            ..inp(2000.0, 10.0, 5.0)
        };
        assert_eq!(axle_transfer(&d, &air), 0.0);
        // Braking the spinning wheel (300 N·m resisted) restores bias: ½·2·300 = 300.
        let braked = DiffInput {
            react_l: 300.0,
            ..inp(2000.0, 10.0, 5.0)
        };
        assert_eq!(axle_transfer(&d, &braked), 300.0);
    }

    #[test]
    fn active_locks_excess_slip_but_not_geometry() {
        let d = AxleDiff::Active {
            max_nm: 2000.0,
            preload_nm: 0.0,
            accel: 0.0,
            decel: 0.0,
            slip_gain: 100.0,
        };
        // Speed difference equal to the cornering geometry: no lock.
        let corner = DiffInput {
            dw_kin: -3.0,
            ..inp(1000.0, 7.0, 10.0)
        };
        assert_eq!(axle_transfer(&d, &corner), 0.0);
        // Inside wheel spinning 5 rad/s past the geometry: ½·100·(5 − 1) = 200.
        let spin = DiffInput {
            dw_kin: -3.0,
            ..inp(1000.0, 12.0, 10.0)
        };
        assert_eq!(axle_transfer(&d, &spin), 200.0);
    }

    #[test]
    fn embedded_table_parses_and_names_real_cars() {
        let t = parse_diff_table(DIFF_TABLE_TOML).expect("differentials.toml parses");
        assert!(!t.is_empty());
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/cars");
        for (id, e) in &t {
            let p = format!("{dir}/{id}.json");
            assert!(std::path::Path::new(&p).exists(), "{id}: no car file");
            assert!(!e.reason.trim().is_empty(), "{id}: no reason");
            let def = wp_contracts::load_vehicle_def(&p).unwrap();
            let s = DiffSetup::for_car(&def);
            for a in [s.front, s.rear] {
                if let AxleDiff::Clutch { accel, decel, .. } = a {
                    assert!((0.0..=1.0).contains(&accel) && (0.0..=accel).contains(&decel));
                }
                if let AxleDiff::Helical { tbr, tbr_coast, .. } = a {
                    assert!(tbr >= 1.0 && tbr_coast >= 1.0);
                }
            }
        }
    }

    #[test]
    fn contract_lsd_maps_to_clutch() {
        let d = DiffDef::Lsd {
            preload_nm: 80.0,
            ramp_power: 0.45,
            ramp_coast: 0.25,
        };
        assert_eq!(
            AxleDiff::from_contract(&d),
            AxleDiff::Clutch {
                preload_nm: 80.0,
                accel: 0.45,
                decel: 0.25
            }
        );
    }
}
