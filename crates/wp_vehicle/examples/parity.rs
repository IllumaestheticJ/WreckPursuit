//! Reference-game parity suite (owner direction 2026-10-20: driving should feel like the owner's
//! reference game). Measures WreckPursuit on a flat pad with the owner's keyboard settings and
//! writes one markdown table: metric | car | WreckPursuit | reference | delta | owner.
//!
//! ```text
//! cargo run --release -p wp_vehicle --example parity            # print the table
//! cargo run --release -p wp_vehicle --example parity -- --write # also update docs/reference/wreckfest-parity.md
//! just parity                                                    # same as --write
//! ```
//!
//! **Cars** (closest roster analogues of the owner's clips): `rampart_50` (heavy RWD muscle
//! car, clips 1 and 4), `shiro_turbo` (light RWD sports car, clip 2), `kestrel_ts` (FWD
//! touring / sport sedan, clip 3).
//!
//! **Driver** = the owner's settings: keyboard preset (`AssistProfile::keyboard_default`) with
//! manual gears, ABS on, ESC off, TCS off (or full where a row says so), steering speed
//! sensitivity 75 % (`steer_speed_sensitivity` 0.75; see `docs/integration/wf-parity.md` for
//! the mapping). Keys go through a copy of the testbed's keyboard layer
//! (`apps/testbed-web/src/input.ts`): ramps updated once per 60 Hz frame (steer 4/s, throttle
//! 12/s, brake 6/s in, 10/s out) and the per-device controls shaping (dead zone, saturation,
//! sensitivity; the owner's values are the identity). Where a manoeuvre needs a constant speed
//! the throttle comes from an analogue cruise controller instead of the key (said in the row).
//!
//! Reference values live in `docs/reference/wreckfest-parity-reference.toml` (the lead adds
//! them as clips arrive); rows without one show "—".
//!
//! Deterministic (f32, fixed step, no wall clock), < 10 s release on one core.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use wp_contracts::{AssistProfile, DriveLayout, LoadedCar, VehicleInput, PHYSICS_DT};
use wp_vehicle::math::{atan2, V3};
use wp_vehicle::rig::Rig;

const TICKS_PER_S: usize = 240;
/// Physics ticks per testbed frame (the testbed reads input at 60 Hz).
const TICKS_PER_FRAME: u64 = 4;
const MPH: f32 = 0.447_04;
const RPM_PER_RADPS: f32 = 60.0 / std::f32::consts::TAU;

/// The three roster analogues: (id, what it stands in for). The parity-only test cars built to
/// the reference specs come from `examples/parity_cars.toml` (see [`cars`]).
const CARS: [(&str, &str); 3] = [
    ("rampart_50", "heavy RWD muscle (clips 1, 4)"),
    ("shiro_turbo", "light RWD sports (clip 2)"),
    ("kestrel_ts", "FWD touring (clip 3)"),
];

// ----- input layer (mirror of apps/testbed-web/src/input.ts) ------------------------------

/// Per-device controls shaping, mirror of `apps/ui-kit/src/menu/controls.ts`
/// (`shapeSteer` / `shapePedal`). Fractions 0..=1.
#[derive(Debug, Clone, Copy)]
struct DeviceControls {
    steer_deadzone: f32,
    steer_saturation: f32,
    steer_sensitivity: f32,
    /// Becomes `AssistProfile::steer_speed_sensitivity`.
    steer_speed_sensitivity: f32,
    pedal_deadzone: f32,
    pedal_saturation: f32,
}

/// The owner's keyboard values: dead zone 0, saturation 100 %, sensitivity 100 %, speed
/// sensitivity 75 %, pedals dead zone 0 / saturation 100 %.
const OWNER_KEYBOARD: DeviceControls = DeviceControls {
    steer_deadzone: 0.0,
    steer_saturation: 1.0,
    steer_sensitivity: 1.0,
    steer_speed_sensitivity: 0.75,
    pedal_deadzone: 0.0,
    pedal_saturation: 1.0,
};

fn shape_t(a: f32, dz: f32, sat: f32) -> f32 {
    if a <= dz {
        0.0
    } else {
        ((a - dz) / (sat - dz).max(1e-3)).min(1.0)
    }
}

fn shape_steer(x: f32, c: &DeviceControls) -> f32 {
    let t = shape_t(x.abs(), c.steer_deadzone, c.steer_saturation);
    let exp = 1.0 + 2.0 * (1.0 - c.steer_sensitivity);
    js_sign(x) * t.powf(exp)
}

fn shape_pedal(x: f32, c: &DeviceControls) -> f32 {
    shape_t(x, c.pedal_deadzone, c.pedal_saturation)
}

/// JavaScript `Math.sign` (0 for 0, unlike `f32::signum`).
fn js_sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `ramp()` from input.ts, quirks included: a release (target 0) counts as "other sign" and
/// moves at `up`.
fn ramp(cur: f32, target: f32, up: f32, down: f32, dt: f32) -> f32 {
    let rate = if target.abs() > cur.abs() || js_sign(target) != js_sign(cur) {
        up
    } else {
        down
    };
    let d = target - cur;
    let step = rate * dt;
    if d.abs() <= step {
        target
    } else {
        cur + js_sign(d) * step
    }
}

/// Keys held this tick (steer +1 = right).
#[derive(Debug, Clone, Copy, Default)]
struct Keys {
    left: bool,
    right: bool,
    throttle: bool,
    brake: bool,
    handbrake: bool,
    /// Analogue throttle instead of the key (cruise controller), 0..=1.
    cruise: Option<f32>,
    gear_up: bool,
    gear_down: bool,
}

/// The testbed's keyboard layer: ramps per 60 Hz frame, then the controls shaping.
struct Keyboard {
    controls: DeviceControls,
    steer: f32,
    throttle: f32,
    brake: f32,
    frame: VehicleInput,
    tick: u64,
}

impl Keyboard {
    fn new(controls: DeviceControls) -> Self {
        Self {
            controls,
            steer: 0.0,
            throttle: 0.0,
            brake: 0.0,
            frame: VehicleInput::default(),
            tick: 0,
        }
    }

    fn input(&mut self, k: &Keys) -> VehicleInput {
        if self.tick.is_multiple_of(TICKS_PER_FRAME) {
            let dt = TICKS_PER_FRAME as f32 * PHYSICS_DT;
            let target = (k.right as i32 - k.left as i32) as f32;
            self.steer = ramp(self.steer, target, 4.0, 6.0, dt);
            self.throttle = match k.cruise {
                Some(v) => v,
                None => ramp(self.throttle, k.throttle as i32 as f32, 12.0, 12.0, dt),
            };
            self.brake = ramp(self.brake, k.brake as i32 as f32, 6.0, 10.0, dt);
            self.frame = VehicleInput {
                steer: shape_steer(self.steer, &self.controls),
                throttle: shape_pedal(self.throttle, &self.controls),
                brake: shape_pedal(self.brake, &self.controls),
                handbrake: if k.handbrake { 1.0 } else { 0.0 },
                ..Default::default()
            };
        }
        self.tick += 1;
        VehicleInput {
            gear_up: k.gear_up,
            gear_down: k.gear_down,
            ..self.frame
        }
    }
}

/// The owner's assists: keyboard preset, manual gears, ABS on, ESC off, TCS as given.
fn owner_assists(tcs: bool) -> AssistProfile {
    AssistProfile {
        abs: true,
        tcs,
        esc: false,
        auto_gears: false,
        auto_clutch: true,
        steer_speed_sensitivity: speed_sensitivity(),
        ..AssistProfile::keyboard_default()
    }
}

/// Steering speed sensitivity used: the owner's 75 %, or `PARITY_SPEED_SENS` (0..=1) for an
/// A/B run (e.g. `PARITY_SPEED_SENS=1` = the old keyboard preset).
fn speed_sensitivity() -> f32 {
    std::env::var("PARITY_SPEED_SENS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| v.clamp(0.0, 1.0))
        .unwrap_or(OWNER_KEYBOARD.steer_speed_sensitivity)
}

// ----- one run ---------------------------------------------------------------------------

/// A car on the pad with the keyboard driver.
struct Run {
    rig: Rig,
    kb: Keyboard,
    assists: AssistProfile,
    heading: f32,
    prev_heading: f32,
    /// Shift cooldown, ticks.
    shift_wait: usize,
    gears: i8,
    redline: f32,
}

impl Run {
    fn new(car: &LoadedCar, tcs: bool) -> Self {
        let rig = Rig::new(car);
        let gears = car.def.drivetrain.gear_ratios.len() as i8;
        Self {
            rig,
            kb: Keyboard::new(OWNER_KEYBOARD),
            assists: owner_assists(tcs),
            heading: 0.0,
            prev_heading: 0.0,
            shift_wait: 0,
            gears,
            redline: car.def.engine.redline_rpm,
        }
    }

    /// Rolling straight along +Y at `speed`.
    fn rolling(car: &LoadedCar, tcs: bool, speed: f32) -> Self {
        let mut r = Self::new(car, tcs);
        r.rig.teleport([0.0; 3], 0.0, speed, 0.0);
        r
    }

    fn t(&self) -> f32 {
        self.rig.time()
    }
    fn speed(&self) -> f32 {
        self.rig.state().speed_mps
    }
    fn gear(&self) -> i8 {
        self.rig.vehicle.gear()
    }
    fn pos(&self) -> [f32; 3] {
        self.rig.state().pos
    }
    /// Yaw rate, rad/s (+ = left).
    fn yaw_rate(&self) -> f32 {
        self.rig.state().ang_vel[2]
    }
    /// Body slip at the CG, degrees (+ = moving toward the left of the heading).
    fn slip_deg(&self) -> f32 {
        let s = self.rig.state();
        if V3::from(s.lin_vel).length() < 1.0 {
            return 0.0;
        }
        let ch = self.rig.vehicle.chassis();
        let v = V3::from(s.lin_vel);
        atan2(v.dot(ch.rot * -V3::X), v.dot(ch.rot * V3::Y)).to_degrees()
    }
    /// Heading change since the start (unwrapped), degrees (+ = left).
    fn heading_deg(&self) -> f32 {
        self.heading.to_degrees()
    }
    /// Largest driven-wheel |slip ratio|.
    fn driven_slip(&self) -> f32 {
        let w = self.rig.state().wheel_states;
        let driven: &[usize] = match self.rig.vehicle.def().drivetrain.layout {
            DriveLayout::Fwd => &[0, 1],
            DriveLayout::Rwd => &[2, 3],
            _ => &[0, 1, 2, 3],
        };
        driven
            .iter()
            .map(|&i| w[i].slip_ratio.abs())
            .fold(0.0, f32::max)
    }

    /// Analogue cruise toward `target` m/s (P-controller on the throttle; brake if well over).
    fn cruise(&self, target: f32) -> (Option<f32>, bool) {
        let err = target - self.speed();
        if err > -1.5 {
            (Some((0.3 + 0.4 * err).clamp(0.0, 1.0)), false)
        } else {
            (Some(0.0), true)
        }
    }

    /// Manual upshift at the redline (the driver's habit for full-throttle runs), once the
    /// driven wheels have hooked up (driven slip ratio < 0.3): a burnout at the limiter in 1st
    /// is not a shift point.
    fn auto_upshift(&mut self, k: &mut Keys) {
        if self.shift_wait > 0 {
            self.shift_wait -= 1;
            return;
        }
        let rpm = self.rig.state().engine_rpm;
        let hooked = self.driven_slip() < 0.3;
        if rpm >= self.redline && hooked && self.gear() >= 1 && self.gear() < self.gears {
            k.gear_up = true;
            self.shift_wait = TICKS_PER_S * 3 / 4;
        }
    }

    /// Shift toward `gear` one step at a time.
    fn select_gear(&mut self, k: &mut Keys, gear: i8) {
        if self.shift_wait > 0 {
            self.shift_wait -= 1;
            return;
        }
        if self.gear() > gear {
            k.gear_down = true;
            self.shift_wait = TICKS_PER_S * 3 / 4;
        } else if self.gear() < gear {
            k.gear_up = true;
            self.shift_wait = TICKS_PER_S * 3 / 4;
        }
    }

    fn step(&mut self, k: &Keys) {
        let input = self.kb.input(k);
        self.rig.step(&input, &self.assists);
        let f = self.rig.vehicle.chassis().rot * V3::Y;
        let h = atan2(-f.x, f.y);
        let mut d = h - self.prev_heading;
        if d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        } else if d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        self.heading += d;
        self.prev_heading = h;
    }

    fn spun(&self) -> bool {
        self.slip_deg().abs() > 90.0
    }
}

fn ticks(s: f32) -> usize {
    (s * TICKS_PER_S as f32).round() as usize
}

// ----- manoeuvres ------------------------------------------------------------------------

/// Standing start, throttle key held, manual upshifts at the redline: times to 60 / 100 mph.
fn accel(car: &LoadedCar, tcs: bool) -> (Option<f32>, Option<f32>) {
    let mut r = Run::new(car, tcs);
    let (mut t60, mut t100) = (None, None);
    for _ in 0..ticks(40.0) {
        let mut k = Keys {
            throttle: true,
            ..Default::default()
        };
        r.auto_upshift(&mut k);
        r.step(&k);
        if t60.is_none() && r.speed() >= 60.0 * MPH {
            t60 = Some(r.t());
        }
        if r.speed() >= 100.0 * MPH {
            t100 = Some(r.t());
            break;
        }
    }
    (t60, t100)
}

/// Launch wheelspin: standing start for 4 s, throttle key held (or feathered: six taps of
/// 0.2 s on / 0.15 s off, then held). Returns (time with driven slip ratio > 0.2, s; peak
/// driven slip ratio; speed at 4 s, mph).
fn launch(car: &LoadedCar, tcs: bool, feather: bool) -> (f32, f32, f32) {
    let mut r = Run::new(car, tcs);
    let (mut spin, mut peak) = (0.0f32, 0.0f32);
    for tick in 0..ticks(4.0) {
        let t = tick as f32 * PHYSICS_DT;
        let on = if feather && t < 2.1 {
            (t % 0.35) < 0.2
        } else {
            true
        };
        let mut k = Keys {
            throttle: on,
            ..Default::default()
        };
        r.auto_upshift(&mut k);
        r.step(&k);
        let s = r.driven_slip();
        peak = peak.max(s);
        if s > 0.2 {
            spin += PHYSICS_DT;
        }
    }
    (spin, peak, r.speed() / MPH)
}

/// 60–0 mph on the brake key, ABS on or off: (time s, distance m).
fn braking(car: &LoadedCar, abs: bool) -> (f32, f32) {
    let mut r = Run::rolling(car, false, 60.0 * MPH);
    r.assists.abs = abs;
    let p0 = r.pos();
    let t0 = r.t();
    for _ in 0..ticks(10.0) {
        r.step(&Keys {
            brake: true,
            ..Default::default()
        });
        if r.speed() < 0.3 {
            break;
        }
    }
    let p = r.pos();
    (
        r.t() - t0,
        ((p[0] - p0[0]).powi(2) + (p[1] - p0[1]).powi(2)).sqrt(),
    )
}

/// Steady circle: the right key held at `mph` (cruise throttle) for 6 s; means over the last
/// 2 s. Returns (|yaw rate| °/s, radius m, |body slip| °).
fn circle(car: &LoadedCar, mph: f32) -> (f32, f32, f32) {
    let target = mph * MPH;
    let mut r = Run::rolling(car, false, target);
    let (mut yaw, mut slip, mut v, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for tick in 0..ticks(6.0) {
        let (cruise, brake) = r.cruise(target);
        r.step(&Keys {
            right: true,
            cruise,
            brake,
            ..Default::default()
        });
        if tick >= ticks(4.0) {
            yaw += r.yaw_rate().abs();
            slip += r.slip_deg().abs();
            v += r.speed().abs();
            n += 1.0;
        }
    }
    let (yaw, slip, v) = (yaw / n, slip / n, v / n);
    (yaw.to_degrees(), v / yaw.max(1e-3), slip)
}

/// Key tap at speed: straight at `mph` (cruise), the right key for `tap_s`, then nothing.
/// Returns (peak |yaw rate| °/s, lateral offset 2 s after the tap started, m; heading change
/// after 2 s, °).
fn tap(car: &LoadedCar, mph: f32, tap_s: f32) -> (f32, f32, f32) {
    let target = mph * MPH;
    let mut r = Run::rolling(car, false, target);
    for _ in 0..ticks(0.5) {
        let (cruise, brake) = r.cruise(target);
        r.step(&Keys {
            cruise,
            brake,
            ..Default::default()
        });
    }
    let x0 = r.pos()[0];
    let h0 = r.heading_deg();
    let mut peak = 0.0f32;
    for tick in 0..ticks(2.0) {
        let (cruise, brake) = r.cruise(target);
        r.step(&Keys {
            right: tick < ticks(tap_s),
            cruise,
            brake,
            ..Default::default()
        });
        peak = peak.max(r.yaw_rate().abs());
    }
    (
        peak.to_degrees(),
        (r.pos()[0] - x0).abs(),
        (r.heading_deg() - h0).abs(),
    )
}

/// Lift-off in a 60 mph corner: the right key held at 60 mph (cruise) for 4 s, then the
/// throttle released (keyboard ramp), key still held. Returns (yaw-rate kick: peak |yaw rate|
/// in 1 s after the lift minus the steady value, °/s; peak |body slip| °; spun).
fn lift_off(car: &LoadedCar) -> (f32, f32, bool) {
    let target = 60.0 * MPH;
    let mut r = Run::rolling(car, false, target);
    let mut steady = 0.0f32;
    for tick in 0..ticks(4.0) {
        let (cruise, brake) = r.cruise(target);
        r.step(&Keys {
            right: true,
            cruise,
            brake,
            ..Default::default()
        });
        if tick >= ticks(3.5) {
            steady = steady.max(r.yaw_rate().abs());
        }
    }
    let (mut peak, mut slip, mut spun) = (0.0f32, 0.0f32, false);
    for _ in 0..ticks(1.5) {
        r.step(&Keys {
            right: true,
            ..Default::default()
        });
        if r.t() < 5.0 + 1e-3 {
            peak = peak.max(r.yaw_rate().abs());
        }
        slip = slip.max(r.slip_deg().abs());
        spun |= r.spun();
    }
    ((peak - steady).to_degrees(), slip, spun)
}

/// Power-on hairpin exit: 20 mph in `gear`, the right key held on the cruise for 2 s (a
/// tight circle), then full throttle key with the key held 1 s, then the steering released
/// (no countersteer) for 1.5 s. Returns (peak |body slip| °, spun, speed gain mph).
fn hairpin_exit(car: &LoadedCar, gear: i8) -> (f32, bool, f32) {
    let target = 20.0 * MPH;
    let mut r = Run::rolling(car, false, target);
    for _ in 0..ticks(2.0) {
        let (cruise, brake) = r.cruise(target);
        let mut k = Keys {
            right: true,
            cruise,
            brake,
            ..Default::default()
        };
        r.select_gear(&mut k, gear);
        r.step(&k);
    }
    let v0 = r.speed();
    let (mut slip, mut spun) = (0.0f32, false);
    for tick in 0..ticks(2.5) {
        r.step(&Keys {
            right: tick < ticks(1.0),
            throttle: true,
            ..Default::default()
        });
        slip = slip.max(r.slip_deg().abs());
        spun |= r.spun();
    }
    (slip, spun, (r.speed() - v0) / MPH)
}

/// Handbrake turn: straight at `mph` (cruise 1 s), then the right key + handbrake for
/// `hb_s` (throttle off), then both released and coasting. Returns (rotation at the
/// handbrake release °, total rotation 1.5 s after the release °, peak |body slip| °).
fn handbrake(car: &LoadedCar, mph: f32, hb_s: f32) -> (f32, f32, f32) {
    let target = mph * MPH;
    let mut r = Run::rolling(car, false, target);
    for _ in 0..ticks(1.0) {
        let (cruise, brake) = r.cruise(target);
        r.step(&Keys {
            cruise,
            brake,
            ..Default::default()
        });
    }
    let h0 = r.heading_deg();
    let mut at_release = 0.0;
    let mut slip = 0.0f32;
    for tick in 0..ticks(hb_s + 1.5) {
        let pull = tick < ticks(hb_s);
        r.step(&Keys {
            right: pull,
            handbrake: pull,
            ..Default::default()
        });
        if tick + 1 == ticks(hb_s) {
            at_release = (r.heading_deg() - h0).abs();
        }
        slip = slip.max(r.slip_deg().abs());
    }
    (at_release, (r.heading_deg() - h0).abs(), slip)
}

/// FWD power-on straightening (clip 3): 50 mph, the right key + handbrake 0.3 s off the
/// throttle, then full throttle and a 0.35 s left tap. Returns (peak |body slip| °, time from
/// throttle-on until |body slip| stays under 5° for 0.25 s, s).
fn hb_power_straighten(car: &LoadedCar) -> (f32, Option<f32>) {
    let target = 50.0 * MPH;
    let mut r = Run::rolling(car, false, target);
    for _ in 0..ticks(1.0) {
        let (cruise, brake) = r.cruise(target);
        r.step(&Keys {
            cruise,
            brake,
            ..Default::default()
        });
    }
    let mut slip = 0.0f32;
    for _ in 0..ticks(0.3) {
        r.step(&Keys {
            right: true,
            handbrake: true,
            ..Default::default()
        });
        slip = slip.max(r.slip_deg().abs());
    }
    let t_on = r.t();
    let (mut calm_since, mut straight) = (None::<f32>, None);
    for tick in 0..ticks(4.0) {
        r.step(&Keys {
            left: tick < ticks(0.35),
            throttle: true,
            ..Default::default()
        });
        let s = r.slip_deg().abs();
        slip = slip.max(s);
        if s < 5.0 {
            let since = *calm_since.get_or_insert(r.t());
            if straight.is_none() && r.t() - since >= 0.25 {
                straight = Some(since - t_on);
            }
        } else {
            calm_since = None;
        }
    }
    (slip, straight)
}

/// Countersteer key held + full throttle (clip 1's technique; Drift Lab C's "Naive" driver):
/// 30 mph in 2nd, the right key + full throttle until 12° of body slip (at most 2 s), then
/// the left key + full throttle held for 4 s. Returns (time at 10–50° body slip, s; mean
/// |slip| while drifting °; max |slip| °; spun).
fn counter_drift(car: &LoadedCar) -> (f32, f32, f32, bool) {
    let target = 30.0 * MPH;
    let mut r = Run::rolling(car, false, target);
    for _ in 0..ticks(1.0) {
        let (cruise, brake) = r.cruise(target);
        let mut k = Keys {
            cruise,
            brake,
            ..Default::default()
        };
        r.select_gear(&mut k, 2);
        r.step(&k);
    }
    let mut entered = false;
    for _ in 0..ticks(2.0) {
        r.step(&Keys {
            right: true,
            throttle: true,
            ..Default::default()
        });
        if r.slip_deg().abs() >= 12.0 {
            entered = true;
            break;
        }
    }
    let (mut drift, mut sum, mut max, mut spun) = (0.0f32, 0.0f32, 0.0f32, false);
    if !entered {
        return (0.0, 0.0, r.slip_deg().abs(), false);
    }
    for _ in 0..ticks(4.0) {
        r.step(&Keys {
            left: true,
            throttle: true,
            ..Default::default()
        });
        let s = r.slip_deg().abs();
        max = max.max(s);
        spun |= r.spun();
        if (10.0..=50.0).contains(&s) && !spun {
            drift += PHYSICS_DT;
            sum += s * PHYSICS_DT;
        }
    }
    (
        drift,
        if drift > 0.0 { sum / drift } else { 0.0 },
        max,
        spun,
    )
}

/// Braking from 80 mph on the brake key with a 0.2 s right tap 0.3 s in (clip 5: ABS + rear
/// bias stays controllable). Returns (peak |body slip| °, spun, heading change at the stop °).
fn brake_tap(car: &LoadedCar, abs: bool) -> (f32, bool, f32) {
    let mut r = Run::rolling(car, false, 80.0 * MPH);
    r.assists.abs = abs;
    let (mut slip, mut spun) = (0.0f32, false);
    for tick in 0..ticks(10.0) {
        r.step(&Keys {
            brake: true,
            right: (ticks(0.3)..ticks(0.5)).contains(&tick),
            ..Default::default()
        });
        slip = slip.max(r.slip_deg().abs());
        spun |= r.spun();
        if r.speed().abs() < 1.0 {
            break;
        }
    }
    (slip, spun, r.heading_deg().abs())
}

/// Handbrake stabs on the throttle (clip 5): 50 mph in 2nd on the cruise for 1 s, then the
/// throttle key + left key held for 2 s with handbrake stabs of 0.25 s (at 0.3 s) and 0.1 s
/// (at 0.9 s) when `stabs`. Returns (rotation at 1.2 s °, speed at 1.2 s m/s, time in 1.0–2.0 s
/// with the driven wheels' drive force under half its pre-stab value s, spun within 2 s). The
/// driver never countersteers, so a later spin is about the power-on exit (see the hairpin
/// rows), not the stabs; rotation and speed are read just after the last stab.
fn hb_stabs(car: &LoadedCar, stabs: bool) -> (f32, f32, f32, bool) {
    let target = 50.0 * MPH;
    let mut r = Run::rolling(car, false, target);
    for _ in 0..ticks(1.0) {
        let (cruise, brake) = r.cruise(target);
        let mut k = Keys {
            cruise,
            brake,
            ..Default::default()
        };
        r.select_gear(&mut k, 2);
        r.step(&k);
    }
    let h0 = r.heading_deg();
    let (mut fx_before, mut gap, mut spun) = (0.0f32, 0.0f32, false);
    let (mut rot, mut v) = (0.0f32, 0.0f32);
    for tick in 0..ticks(2.0) {
        let hb = stabs
            && ((ticks(0.3)..ticks(0.55)).contains(&tick)
                || (ticks(0.9)..ticks(1.0)).contains(&tick));
        r.step(&Keys {
            left: true,
            throttle: true,
            handbrake: hb,
            ..Default::default()
        });
        let fx = driven_fx(&r);
        if tick < ticks(0.3) {
            fx_before = fx_before.max(fx);
        } else if tick >= ticks(1.0) && fx < 0.5 * fx_before {
            gap += PHYSICS_DT;
        }
        spun |= r.spun();
        if tick + 1 == ticks(1.2) {
            rot = (r.heading_deg() - h0).abs();
            v = r.speed();
        }
    }
    (rot, v, gap, spun)
}

/// Summed longitudinal force of the driven wheels, N.
fn driven_fx(r: &Run) -> f32 {
    let tel = r.rig.vehicle.telemetry();
    match r.rig.vehicle.def().drivetrain.layout {
        DriveLayout::Fwd => tel.fx[0] + tel.fx[1],
        DriveLayout::Rwd => tel.fx[2] + tel.fx[3],
        _ => tel.fx.iter().sum(),
    }
}

/// Upshift 2→3 at full throttle: time from the key press until the driven wheels' summed
/// longitudinal force is back to half its pre-shift value, s.
fn shift_gap(car: &LoadedCar) -> Option<f32> {
    let mut r = Run::new(car, true);
    // (press time, pre-shift force, force has dipped under half yet)
    let mut pressed: Option<(f32, f32, bool)> = None;
    for _ in 0..ticks(30.0) {
        let mut k = Keys {
            throttle: true,
            ..Default::default()
        };
        match pressed {
            None => {
                r.auto_upshift(&mut k);
                if k.gear_up && r.gear() == 2 {
                    pressed = Some((r.t(), driven_fx(&r), false));
                }
            }
            Some((t0, fx0, dipped)) => {
                let fx = driven_fx(&r);
                if !dipped && fx < 0.5 * fx0 {
                    pressed = Some((t0, fx0, true));
                } else if dipped && fx >= 0.5 * fx0 {
                    return Some(r.t() - t0);
                }
            }
        }
        r.step(&k);
    }
    None
}

/// Speed at the rev limiter in each gear, mph (driven wheel's loaded radius).
fn gear_top_speeds(car: &LoadedCar) -> Vec<f32> {
    let dt = &car.def.drivetrain;
    let driven = match dt.layout {
        DriveLayout::Fwd => 0,
        _ => 2,
    };
    let radius = car.def.wheels[driven].radius_m;
    let limit = car.def.engine.rev_limit_rpm;
    dt.gear_ratios
        .iter()
        .map(|g| limit / RPM_PER_RADPS / (g * dt.final_drive) * radius / MPH)
        .collect()
}

/// Full throttle on a long straight (TCS on, manual upshifts at the redline): top speed mph.
fn top_speed(car: &LoadedCar) -> f32 {
    let mut r = Run::new(car, true);
    let mut best = 0.0f32;
    let mut last_gain_t = 0.0;
    for _ in 0..ticks(90.0) {
        let mut k = Keys {
            throttle: true,
            ..Default::default()
        };
        r.auto_upshift(&mut k);
        r.step(&k);
        if r.speed() > best + 0.05 {
            best = r.speed();
            last_gain_t = r.t();
        }
        if r.t() - last_gain_t > 4.0 {
            break;
        }
    }
    best / MPH
}

// ----- test cars and the tune mapping ---------------------------------------------------

/// One parity-only test car from `examples/parity_cars.toml`.
#[derive(Debug, Clone, serde::Deserialize)]
struct ParityCar {
    id: String,
    #[serde(rename = "ref")]
    reference: String,
    role: String,
    base: String,
    mass_kg: f32,
    power_kw: f32,
    power_rpm: f32,
    torque_nm: f32,
    torque_rpm: f32,
    gear_ratios: Vec<f32>,
    final_drive: f32,
}

#[derive(Debug, serde::Deserialize)]
struct ParityCars {
    car: Vec<ParityCar>,
}

/// Full-load torque curve with `torque_nm` at `torque_rpm` and `power_kw` at `power_rpm` (the
/// power peak), falling off past it; redline 8 % above the power peak, limiter +300 rpm.
fn reference_curve(c: &ParityCar, idle: f32) -> (Vec<(f32, f32)>, f32, f32) {
    let tp = c.power_kw * 1000.0 / (c.power_rpm / RPM_PER_RADPS);
    let redline = c.power_rpm * 1.08;
    let limit = redline + 300.0;
    let low = idle.max(800.0) + 200.0;
    let curve = vec![
        (low, 0.55 * c.torque_nm),
        (0.6 * c.torque_rpm, 0.85 * c.torque_nm),
        (c.torque_rpm, c.torque_nm),
        (0.5 * (c.torque_rpm + c.power_rpm), 0.5 * (c.torque_nm + tp)),
        (c.power_rpm, tp),
        (redline, 0.95 * tp * c.power_rpm / redline),
        (limit, 0.8 * tp * c.power_rpm / redline),
    ];
    (curve, redline, limit)
}

/// Build a parity car from its roster base (see the header of `parity_cars.toml`).
fn build_parity_car(root: &str, c: &ParityCar) -> LoadedCar {
    let mut car = wp_contracts::load_car(format!("{root}/data/cars/{}.json", c.base))
        .expect("base car loads");
    let d = &mut car.def;
    let k = c.mass_kg / d.mass_kg;
    d.id = c.id.clone();
    d.display_name = format!("Parity {}", c.reference);
    d.mass_kg = c.mass_kg;
    for v in &mut d.inertia_kgm2 {
        *v *= k;
    }
    for s in &mut d.suspension {
        s.spring_n_per_m *= k;
        s.damper_bump_ns_per_m *= k;
        s.damper_rebound_ns_per_m *= k;
        s.arb_n_per_m *= k;
    }
    d.brakes.max_torque_front_nm *= k;
    d.brakes.max_torque_rear_nm *= k;
    d.brakes.handbrake_rear_nm *= k;
    let (curve, redline, limit) = reference_curve(c, d.engine.idle_rpm);
    d.engine.torque_curve_nm = curve;
    d.engine.redline_rpm = redline;
    d.engine.rev_limit_rpm = limit;
    d.drivetrain.gear_ratios = c.gear_ratios.clone();
    d.drivetrain.final_drive = c.final_drive;
    d.drivetrain.clutch_max_nm = d.drivetrain.clutch_max_nm.max(1.35 * c.torque_nm);
    wp_contracts::validate_vehicle_def(d).expect("parity car is a valid VehicleDef");
    car
}

/// The reference game's free tune screen: four 5-step sliders, 3 = stock. This is the
/// **proposed** mapping onto our car data (docs/reference/wreckfest-parity.md → "Tune screen");
/// the suite applies it to measure the owner's tune.
#[derive(Debug, Clone, Copy)]
struct Tune {
    /// Suspension, 1 soft … 5 stiff.
    suspension: u8,
    /// Gear ratio, 1 short … 5 long.
    gearing: u8,
    /// Differential, 1 open … 3 stock (limited) … 5 locked.
    diff: u8,
    /// Brake balance, 1 rear … 5 front.
    brake_balance: u8,
}

/// Spring / anti-roll bar scale per suspension step (dampers get the square root, which keeps
/// the damping ratio).
const TUNE_SPRING: [f32; 5] = [0.70, 0.85, 1.0, 1.18, 1.40];
/// Final-drive scale per gearing step (long = numerically lower).
const TUNE_FINAL: [f32; 5] = [1.12, 1.06, 1.0, 0.94, 0.88];
/// Front brake-bias offset per balance step.
const TUNE_BIAS: [f32; 5] = [-0.10, -0.05, 0.0, 0.05, 0.10];

/// Apply a [`Tune`] to `car` (its id gets a `_tuned` suffix, so a stock
/// `differentials.toml` entry no longer overrides the tuned differential).
fn apply_tune(car: &LoadedCar, t: Tune) -> LoadedCar {
    use wp_contracts::DiffDef;
    let mut car = car.clone();
    let d = &mut car.def;
    let step = |v: u8| (v.clamp(1, 5) - 1) as usize;
    let ks = TUNE_SPRING[step(t.suspension)];
    for s in &mut d.suspension {
        s.spring_n_per_m *= ks;
        s.arb_n_per_m *= ks;
        s.damper_bump_ns_per_m *= ks.sqrt();
        s.damper_rebound_ns_per_m *= ks.sqrt();
    }
    d.drivetrain.final_drive *= TUNE_FINAL[step(t.gearing)];
    d.brakes.bias_front = (d.brakes.bias_front + TUNE_BIAS[step(t.brake_balance)]).clamp(0.3, 0.8);
    // The differential of the driven axle as the car runs it today (differentials.toml wins
    // over the car file), reduced to the contract's Open / Lsd / Locked.
    let stock = match wp_vehicle::DiffSetup::for_car(d).rear {
        wp_vehicle::AxleDiff::Open => DiffDef::Open,
        wp_vehicle::AxleDiff::Locked => DiffDef::Locked,
        wp_vehicle::AxleDiff::Clutch {
            preload_nm,
            accel,
            decel,
        } => DiffDef::Lsd {
            preload_nm,
            ramp_power: accel,
            ramp_coast: decel,
        },
        _ => d.drivetrain.diff_rear,
    };
    let lsd = |pre: f32, power: f32, coast: f32| DiffDef::Lsd {
        preload_nm: pre,
        ramp_power: power,
        ramp_coast: coast,
    };
    let (pre, pw, co) = match stock {
        DiffDef::Lsd {
            preload_nm,
            ramp_power,
            ramp_coast,
        } => (preload_nm, ramp_power, ramp_coast),
        _ => (60.0, 0.4, 0.25),
    };
    let rear = match step(t.diff) {
        0 => DiffDef::Open,
        1 => lsd(0.5 * pre, 0.6 * pw, 0.6 * co),
        2 => lsd(pre, pw, co),
        3 => lsd(2.5 * pre, pw.max(0.75), co.max(0.6)),
        _ => DiffDef::Locked,
    };
    if !matches!(d.drivetrain.layout, DriveLayout::Fwd) {
        d.drivetrain.diff_rear = rear;
    }
    d.id = format!("{}_tuned", d.id);
    car
}

/// Every car the suite measures: the roster analogues, the parity-only reference cars, and
/// REF-RWD-A with the owner's clip-5 tune. (id, description, car).
fn cars(root: &str) -> Vec<(String, String, LoadedCar)> {
    let mut out = Vec::new();
    for (id, what) in CARS {
        let car = wp_contracts::load_car(format!("{root}/data/cars/{id}.json")).expect("car loads");
        out.push((id.to_string(), what.to_string(), car));
    }
    let path = format!("{}/examples/parity_cars.toml", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("parity_cars.toml");
    let list: ParityCars = toml::from_str(&text).expect("parity_cars.toml parses");
    for c in &list.car {
        let car = build_parity_car(root, c);
        out.push((c.id.clone(), format!("{}: {}", c.reference, c.role), car));
    }
    // The owner's run-5 tune on REF-RWD-A: stiff 5/5, long 5/5, diff 4/5, brake balance 2/5.
    let a = out
        .iter()
        .find(|(id, _, _)| id == "parity_rwd_a")
        .map(|(_, _, c)| c.clone())
        .expect("parity_rwd_a defined");
    let tuned = apply_tune(
        &a,
        Tune {
            suspension: 5,
            gearing: 5,
            diff: 4,
            brake_balance: 2,
        },
    );
    out.push((
        tuned.def.id.clone(),
        "REF-RWD-A with the owner's tune (stiff 5, long 5, diff 4, brakes 2)".to_string(),
        tuned,
    ));
    out
}

// ----- table -----------------------------------------------------------------------------

/// One measured row.
struct Row {
    id: String,
    label: String,
    car: String,
    value: String,
    num: Option<f32>,
    owner: &'static str,
}

const A: &str = "wp_vehicle (A)";
const A_STEER: &str = "wp_vehicle steering (A) + testbed input";
const DRIFT: &str = "wp_vehicle drift (Drift Lab C)";
const CARS_H: &str = "data/cars (H)";
const TCS: &str = "wp_vehicle assists (A, row 105)";
const ABS: &str = "wp_vehicle brakes / ABS levels (A)";

/// Rows of one car.
struct Rows {
    out: Vec<Row>,
    car: String,
}

impl Rows {
    /// A numeric row (`None` = not reached, shown as "n/a").
    fn num(&mut self, id: &str, label: &str, num: Option<f32>, prec: usize, owner: &'static str) {
        let value = match num {
            Some(v) => format!("{v:.prec$}"),
            None => "n/a".into(),
        };
        self.out.push(Row {
            id: id.into(),
            label: label.into(),
            car: self.car.clone(),
            value,
            num,
            owner,
        });
    }

    /// A text row.
    fn text(&mut self, id: &str, label: &str, value: String, owner: &'static str) {
        self.out.push(Row {
            id: id.into(),
            label: label.into(),
            car: self.car.clone(),
            value,
            num: None,
            owner,
        });
    }
}

fn rows_for(id: &str, car: &LoadedCar) -> Vec<Row> {
    let mut rows = Rows {
        out: Vec::new(),
        car: id.to_string(),
    };
    let (t60, t100) = accel(car, false);
    rows.num("accel_0_60_s", "0–60 mph, s (TCS off)", t60, 2, A);
    rows.num("accel_0_100_s", "0–100 mph, s (TCS off)", t100, 2, A);
    let (t60f, t100f) = accel(car, true);
    rows.num("accel_0_60_tcs_s", "0–60 mph, s (TCS full)", t60f, 2, A);
    rows.num("accel_0_100_tcs_s", "0–100 mph, s (TCS full)", t100f, 2, A);

    for (tcs, name) in [(Some(false), "off"), (None, "half"), (Some(true), "full")] {
        match tcs {
            Some(on) => {
                let (spin, peak, _) = launch(car, on, false);
                rows.num(
                    &format!("launch_spin_{name}_s"),
                    &format!("launch: time at slip ratio > 0.2, s (TCS {name})"),
                    Some(spin),
                    2,
                    TCS,
                );
                rows.num(
                    &format!("launch_peak_slip_{name}"),
                    &format!("launch: peak driven slip ratio (TCS {name})"),
                    Some(peak),
                    2,
                    TCS,
                );
                let (fspin, fpeak, _) = launch(car, on, true);
                rows.num(
                    &format!("launch_feather_spin_{name}_s"),
                    &format!(
                        "feathered launch (6 taps then hold): time at slip > 0.2, s (TCS {name})"
                    ),
                    Some(fspin),
                    2,
                    TCS,
                );
                rows.num(
                    &format!("launch_feather_peak_slip_{name}"),
                    &format!("feathered launch: peak driven slip ratio (TCS {name})"),
                    Some(fpeak),
                    2,
                    TCS,
                );
            }
            None => {
                let msg = "n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105)".to_string();
                rows.text(
                    "launch_spin_half_s",
                    "launch: time at slip ratio > 0.2, s (TCS half)",
                    msg.clone(),
                    TCS,
                );
                rows.text(
                    "launch_feather_spin_half_s",
                    "feathered launch: time at slip > 0.2, s (TCS half)",
                    msg,
                    TCS,
                );
            }
        }
    }

    let (bt, bd) = braking(car, true);
    rows.num(
        "brake_60_0_s",
        "60–0 mph braking time, s (ABS full)",
        Some(bt),
        2,
        A,
    );
    rows.num(
        "brake_60_0_m",
        "60–0 mph braking distance, m (ABS full)",
        Some(bd),
        1,
        A,
    );
    let (bt0, bd0) = braking(car, false);
    rows.num(
        "brake_60_0_noabs_s",
        "60–0 mph braking time, s (ABS off)",
        Some(bt0),
        2,
        ABS,
    );
    rows.num(
        "brake_60_0_noabs_m",
        "60–0 mph braking distance, m (ABS off)",
        Some(bd0),
        1,
        ABS,
    );
    rows.text(
        "brake_60_0_abs_half_s",
        "60–0 mph braking time, s (ABS half)",
        "n/a — no ABS level yet (bool `abs`)".into(),
        ABS,
    );
    for abs in [true, false] {
        let name = if abs { "full" } else { "off" };
        let (slip, spun, stop) = brake_tap(car, abs);
        rows.num(
            &format!("brake_tap_80_{name}_slip_deg"),
            &format!("braking from 80 mph + 0.2 s tap (ABS {name}): peak body slip °"),
            Some(slip),
            1,
            ABS,
        );
        rows.text(
            &format!("brake_tap_80_{name}_spun"),
            &format!("braking from 80 mph + 0.2 s tap (ABS {name}): spun"),
            yes_no(spun),
            ABS,
        );
        rows.num(
            &format!("brake_tap_80_{name}_heading_deg"),
            &format!("braking from 80 mph + 0.2 s tap (ABS {name}): heading change at the stop °"),
            Some(stop),
            1,
            ABS,
        );
    }

    for mph in [30.0, 60.0] {
        let (yaw, radius, slip) = circle(car, mph);
        let m = mph as i32;
        rows.num(
            &format!("circle_{m}_yaw_dps"),
            &format!("key held, {m} mph circle: yaw rate °/s (cruise throttle)"),
            Some(yaw),
            1,
            A_STEER,
        );
        rows.num(
            &format!("circle_{m}_radius_m"),
            &format!("key held, {m} mph circle: radius m"),
            Some(radius),
            1,
            A_STEER,
        );
        rows.num(
            &format!("circle_{m}_slip_deg"),
            &format!("key held, {m} mph circle: body slip °"),
            Some(slip),
            1,
            A_STEER,
        );
    }

    for mph in [60.0, 100.0] {
        for tap_s in [0.1, 0.2, 0.4] {
            let (yaw, off, head) = tap(car, mph, tap_s);
            let m = mph as i32;
            let key = format!("tap_{tap_s:.1}_{m}");
            rows.num(
                &format!("{key}_yaw_dps"),
                &format!("{tap_s:.1} s tap at {m} mph: peak yaw rate °/s"),
                Some(yaw),
                1,
                A_STEER,
            );
            rows.num(
                &format!("{key}_offset_m"),
                &format!("{tap_s:.1} s tap at {m} mph: lateral offset after 2 s, m"),
                Some(off),
                2,
                A_STEER,
            );
            rows.num(
                &format!("{key}_heading_deg"),
                &format!("{tap_s:.1} s tap at {m} mph: heading change after 2 s, °"),
                Some(head),
                1,
                A_STEER,
            );
        }
    }

    let (kick, lslip, lspun) = lift_off(car);
    rows.num(
        "liftoff_60_kick_dps",
        "lift-off, 60 mph corner (key held): yaw-rate kick °/s",
        Some(kick),
        1,
        A,
    );
    rows.num(
        "liftoff_60_slip_deg",
        "lift-off, 60 mph corner: peak body slip °",
        Some(lslip),
        1,
        A,
    );
    rows.text(
        "liftoff_60_spun",
        "lift-off, 60 mph corner: spun",
        yes_no(lspun),
        A,
    );

    for gear in [1, 2] {
        let (slip, spun, gain) = hairpin_exit(car, gear);
        rows.num(
            &format!("hairpin_g{gear}_slip_deg"),
            &format!("power-on exit, 20 mph hairpin, gear {gear}: peak body slip °"),
            Some(slip),
            1,
            A,
        );
        rows.text(
            &format!("hairpin_g{gear}_spun"),
            &format!("power-on exit, 20 mph hairpin, gear {gear}: spun"),
            yes_no(spun),
            A,
        );
        rows.num(
            &format!("hairpin_g{gear}_gain_mph"),
            &format!("power-on exit, gear {gear}: speed gained in 2.5 s, mph"),
            Some(gain),
            1,
            A,
        );
    }

    for (mph, hb_s) in [(30.0, 0.3), (30.0, 1.0), (10.0, 1.0), (65.0, 0.3)] {
        let (at_rel, total, slip) = handbrake(car, mph, hb_s);
        let m = mph as i32;
        let key = format!("hb_{hb_s:.1}_{m}");
        rows.num(
            &format!("{key}_release_deg"),
            &format!("handbrake {hb_s:.1} s + key at {m} mph: rotation at release °"),
            Some(at_rel),
            0,
            DRIFT,
        );
        rows.num(
            &format!("{key}_total_deg"),
            &format!("handbrake {hb_s:.1} s + key at {m} mph: rotation 1.5 s after release °"),
            Some(total),
            0,
            DRIFT,
        );
        rows.num(
            &format!("{key}_slip_deg"),
            &format!("handbrake {hb_s:.1} s + key at {m} mph: peak body slip °"),
            Some(slip),
            0,
            DRIFT,
        );
    }

    let (pslip, straight) = hb_power_straighten(car);
    rows.num(
        "hb_power_slip_deg",
        "50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip °",
        Some(pslip),
        0,
        A,
    );
    rows.num(
        "hb_power_straight_s",
        "… time from throttle-on to straight (< 5° slip), s",
        straight,
        2,
        A,
    );

    let stab = hb_stabs(car, true);
    let ctrl = hb_stabs(car, false);
    rows.num(
        "hb_stab_extra_deg",
        "50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, °",
        Some(stab.0 - ctrl.0),
        0,
        DRIFT,
    );
    rows.num(
        "hb_stab_speed_loss_mph",
        "… speed lost vs no stabs at 1.2 s, mph",
        Some((ctrl.1 - stab.1) / MPH),
        1,
        DRIFT,
    );
    rows.num(
        "hb_stab_drive_gap_s",
        "… time the drive force is under half in the second after the last stab, s",
        Some(stab.2),
        2,
        DRIFT,
    );
    rows.text(
        "hb_stab_spun",
        "… spun within 2 s (no countersteer)",
        yes_no(stab.3),
        DRIFT,
    );
    rows.text(
        "hb_stab_control_spun",
        "… control run without stabs spun within 2 s",
        yes_no(ctrl.3),
        DRIFT,
    );

    if !matches!(car.def.drivetrain.layout, DriveLayout::Fwd) {
        let (drift, mean, max, spun) = counter_drift(car);
        rows.num(
            "drift_counter_s",
            "counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s",
            Some(drift),
            2,
            DRIFT,
        );
        rows.num(
            "drift_counter_mean_deg",
            "counter key + full throttle: mean drift angle °",
            Some(mean),
            0,
            DRIFT,
        );
        rows.num(
            "drift_counter_max_deg",
            "counter key + full throttle: max body slip °",
            Some(max),
            0,
            DRIFT,
        );
        rows.text(
            "drift_counter_spun",
            "counter key + full throttle: spun",
            yes_no(spun),
            DRIFT,
        );
    }

    let tops = gear_top_speeds(car);
    for (i, v) in tops.iter().enumerate() {
        rows.num(
            &format!("gear_{}_top_mph", i + 1),
            &format!("gear {} top speed at the limiter, mph", i + 1),
            Some(*v),
            0,
            CARS_H,
        );
    }
    rows.num(
        "top_speed_mph",
        "top speed (measured, full throttle), mph",
        Some(top_speed(car)),
        0,
        CARS_H,
    );
    rows.num(
        "shift_time_s",
        "shift time (car data `shift_time_s`), s",
        Some(car.def.drivetrain.shift_time_s),
        2,
        CARS_H,
    );
    rows.num(
        "shift_gap_s",
        "2→3 upshift: drive back to 50 %, s (measured)",
        shift_gap(car),
        2,
        CARS_H,
    );
    rows.out
}

fn yes_no(b: bool) -> String {
    if b { "yes" } else { "no" }.into()
}

// ----- reference values ------------------------------------------------------------------

/// One reference value from `docs/reference/wreckfest-parity-reference.toml`.
#[derive(Debug, Clone, serde::Deserialize)]
struct Reference {
    metric: String,
    /// Car id, several ids separated by commas, or `"*"` for every car.
    car: String,
    /// `"1.3"`, `"30–45"` / `"30-45"`, `"≈2.2"`, `"no"`, or any text.
    value: String,
    /// Where it comes from (clip, derivation).
    #[serde(default)]
    source: String,
}

#[derive(Debug, Default, serde::Deserialize)]
struct RefFile {
    #[serde(default, rename = "ref")]
    refs: Vec<Reference>,
}

/// A numeric reading of a reference value: (low, high).
fn parse_range(s: &str) -> Option<(f32, f32)> {
    let t = s.trim();
    if let Some(v) = t.strip_prefix('≥').or_else(|| t.strip_prefix(">=")) {
        return Some((v.trim().parse().ok()?, f32::INFINITY));
    }
    if let Some(v) = t.strip_prefix('≤').or_else(|| t.strip_prefix("<=")) {
        return Some((f32::NEG_INFINITY, v.trim().parse().ok()?));
    }
    let t = t.trim_start_matches(['≈', '~']).trim();
    let t = t.replace('–', "-");
    if let Some((a, b)) = t.split_once('-') {
        if !a.trim().is_empty() {
            return Some((a.trim().parse().ok()?, b.trim().parse().ok()?));
        }
    }
    let v: f32 = t.parse().ok()?;
    Some((v, v))
}

fn delta(num: Option<f32>, value: &str, reference: &str) -> String {
    if let (Some(v), Some((lo, hi))) = (num, parse_range(reference)) {
        if lo != hi && (lo..=hi).contains(&v) {
            return "in range".into();
        }
        let d = if v < lo { v - lo } else { v - hi };
        let base = if v < lo { lo } else { hi };
        let pct = if base.is_finite() && base.abs() > 1e-3 {
            format!(" ({:+.0} %)", 100.0 * d / base)
        } else {
            String::new()
        };
        return format!("{d:+.2}{pct}");
    }
    let r = reference.trim().to_ascii_lowercase();
    let v = value.trim().to_ascii_lowercase();
    if (r == "yes" || r == "no") && (v == "yes" || v == "no") {
        return if r == v {
            "match".into()
        } else {
            "differs".into()
        };
    }
    "—".into()
}

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let root = format!("{}/../..", env!("CARGO_MANIFEST_DIR"));
    let ref_path = format!("{root}/docs/reference/wreckfest-parity-reference.toml");
    let refs: RefFile = std::fs::read_to_string(&ref_path)
        .ok()
        .map(|t| toml::from_str(&t).expect("reference toml parses"))
        .unwrap_or_default();
    let mut by_key: BTreeMap<(String, String), Reference> = BTreeMap::new();
    for r in refs.refs {
        // `car` may list several ids: "rampart_50, parity_rwd_d".
        for car in r.car.split(',') {
            by_key.insert((r.metric.clone(), car.trim().to_string()), r.clone());
        }
    }

    let mut table = String::new();
    writeln!(
        table,
        "| metric | car | WreckPursuit | reference | delta | owner crate |"
    )
    .unwrap();
    writeln!(table, "|---|---|---|---|---|---|").unwrap();
    for (id, what, car) in cars(&root) {
        eprintln!("measuring {id} ({what}) …");
        for row in rows_for(&id, &car) {
            let r = by_key
                .get(&(row.id.clone(), row.car.to_string()))
                .or_else(|| by_key.get(&(row.id.clone(), "*".to_string())));
            let (refv, d) = match r {
                Some(r) => {
                    let src = if r.source.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", r.source)
                    };
                    (
                        format!("{}{src}", r.value),
                        delta(row.num, &row.value, &r.value),
                    )
                }
                None => ("—".into(), "—".into()),
            };
            writeln!(
                table,
                "| {} `{}` | {} | {} | {} | {} | {} |",
                row.label, row.id, row.car, row.value, refv, d, row.owner
            )
            .unwrap();
        }
    }
    print!("{table}");

    if write {
        let doc_path = format!("{root}/docs/reference/wreckfest-parity.md");
        let doc = std::fs::read_to_string(&doc_path).expect("parity doc exists");
        let (begin, end) = ("<!-- parity:begin -->", "<!-- parity:end -->");
        let (Some(a), Some(b)) = (doc.find(begin), doc.find(end)) else {
            panic!("{doc_path} has no {begin} / {end} markers");
        };
        let new = format!("{}{begin}\n{table}{}", &doc[..a], &doc[b..]);
        std::fs::write(&doc_path, new).expect("write parity doc");
        eprintln!("updated {doc_path}");
    }
}
