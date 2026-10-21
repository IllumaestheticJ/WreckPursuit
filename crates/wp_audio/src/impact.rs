//! Physically driven impact synthesis parameters (owner row 74: "more accurate, harder-hitting
//! impact sounds").
//!
//! The director still plays the recorded impact layers ([`crate::collision`]); on top of them it
//! sends one `impact/body` voice per hit whose synth parameters come from here. The backend
//! (`apps/testbed-web/src/audio/impacts/`) renders them as:
//!
//! - a **modal body-panel bank**: plate modes of the struck region (door, hood, roof, bumper
//!   plastic, …). Frequencies and ring times depend on the region and on how damaged that panel
//!   already is (dented panels ring shorter, are less harmonic and buzz);
//! - a **chassis thump**: a low, mass-dependent pitch-dropping sine for heavy hits;
//! - a **bass-boosted transient** and a brief **sidechain duck** of the other buses for big hits;
//! - for broken glass, a shatter onset and a tinkling debris tail.
//!
//! Crunch (granular metal) is a separate looped voice per vehicle whose density follows the
//! lattice's plastic deformation power ([`CrunchState`]), so it lasts exactly as long as the
//! body is being crushed. Scrapes get friction parameters ([`scrape_rate_hz`]).
//!
//! Everything here is a pure function of the event and the vehicle state: same inputs, same
//! parameters (the cosmetic `seed` comes from the director's seeded RNG).

use core::f32::consts::{LN_2, LOG2_E};
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, MaterialKind};
use wp_math::clamp01;

/// `log2(x)` from the float's exponent and a short odd series on its mantissa: basic
/// arithmetic only, so it is bit-identical on every target (the determinism lint forbids libm
/// transcendentals). Absolute error < 1e-4. `x <= 0` → a large negative number.
pub fn det_log2(x: f32) -> f32 {
    if x.is_nan() || x <= 0.0 {
        return -126.0;
    }
    let bits = x.to_bits();
    let e = ((bits >> 23) & 0xFF) as i32 - 127;
    let m = f32::from_bits((bits & 0x007F_FFFF) | 0x3F80_0000); // [1, 2)
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    // ln(m) = 2 (t + t³/3 + t⁵/5 + t⁷/7 + t⁹/9); log2 = ln / ln 2.
    let ln_m = 2.0 * t * (1.0 + t2 * (1.0 / 3.0 + t2 * (0.2 + t2 * (1.0 / 7.0 + t2 / 9.0))));
    e as f32 + ln_m * LOG2_E
}

/// `2^y` (deterministic, relative error < 1e-5; clamped to the normal f32 range).
pub fn det_exp2(y: f32) -> f32 {
    let y = y.clamp(-125.0, 127.0);
    let i = if y >= 0.0 { y as i32 } else { y as i32 - 1 };
    let i = if (i as f32) > y { i - 1 } else { i };
    // ln 2 · frac; then e^f, f ∈ [0, ln 2), by Taylor to f⁷.
    let f = (y - i as f32) * LN_2;
    let ef = 1.0
        + f * (1.0
            + f * (0.5
                + f * (1.0 / 6.0
                    + f * (1.0 / 24.0 + f * (1.0 / 120.0 + f * (1.0 / 720.0 + f / 5040.0))))));
    ef * f32::from_bits(((i + 127) as u32) << 23)
}

/// `x^a` for `x > 0` (deterministic).
pub fn det_pow(x: f32, a: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    det_exp2(a * det_log2(x))
}

/// Natural log ratio `ln(a / b)` (deterministic).
fn ln_ratio(a: f32, b: f32) -> f32 {
    (det_log2(a) - det_log2(b)) * LN_2
}

/// Tuning of the impact synthesis (`audio_config.json` → `impact_synth`, optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImpactSynthConfig {
    /// Collision energy (½·m_eff·v², J) at which the excitation strength starts, J.
    pub energy_floor_j: f32,
    /// Collision energy giving full strength, J.
    pub energy_full_j: f32,
    /// Normal impulse of one contact tick giving full strength on its own, N·s.
    pub impulse_full_ns: f32,
    /// Reference vehicle mass for size scaling (panel modes, thump pitch), kg.
    pub ref_mass_kg: f32,
    /// Strength above which the chassis thump starts.
    pub thump_onset: f32,
    /// Strength above which the bass transient and the sidechain duck start.
    pub heavy_onset: f32,
    /// Deepest sidechain duck of the other buses (fraction of their level removed), 0..1.
    pub duck_depth: f32,
    /// Crunch: plastic power at which the crunch starts, W.
    pub crunch_floor_w: f32,
    /// Crunch: plastic power giving the densest crunch, W.
    pub crunch_full_w: f32,
    /// Crunch: reservoir time constant (how long the crunch rings on after the crush), s.
    pub crunch_tau_s: f32,
    /// Crunch voice gain at full density.
    pub crunch_gain: f32,
    /// Share of the collision energy assumed to become plastic work when the caller gives no
    /// measured deformation energy.
    pub plastic_share_estimate: f32,
    /// Stick-slip rate of a scrape at rest, Hz.
    pub scrape_rate_base_hz: f32,
    /// Stick-slip rate added per m/s of sliding speed, Hz.
    pub scrape_rate_per_mps: f32,
    /// Contact impulse per tick at which a scrape is pressed hardest, N·s.
    pub scrape_force_full_ns: f32,
}

impl Default for ImpactSynthConfig {
    fn default() -> Self {
        Self {
            energy_floor_j: 300.0,
            energy_full_j: 400_000.0,
            impulse_full_ns: 6000.0,
            ref_mass_kg: 1400.0,
            thump_onset: 0.35,
            heavy_onset: 0.55,
            duck_depth: 0.6,
            crunch_floor_w: 8_000.0,
            crunch_full_w: 2_000_000.0,
            crunch_tau_s: 0.12,
            crunch_gain: 1.0,
            plastic_share_estimate: 0.5,
            scrape_rate_base_hz: 18.0,
            scrape_rate_per_mps: 9.0,
            scrape_force_full_ns: 600.0,
        }
    }
}

/// Acoustic class of a struck body region: plate size / stiffness / damping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PanelClass {
    /// Plastic bumper cover over a beam.
    Bumper,
    /// Hood: large, braced, double-skinned.
    Hood,
    /// Front fender / rear quarter: small curved panels.
    Wing,
    /// Door: medium panel, intrusion beam and deadener (thunky).
    Door,
    /// Roof: the largest, least damped panel.
    Roof,
    /// Trunk / tailgate lid.
    Trunk,
    /// Glass (unbroken).
    Glass,
    /// Frame rails, cabin structure: dull, dense.
    Structure,
}

impl PanelClass {
    /// Class of a lattice region.
    pub fn of(region: BodyRegion) -> Self {
        use BodyRegion as R;
        match region {
            R::FrontBumper | R::RearBumper => Self::Bumper,
            R::Hood => Self::Hood,
            R::FrontLeftFender | R::FrontRightFender | R::RearLeftQuarter | R::RearRightQuarter => {
                Self::Wing
            }
            R::LeftDoorF | R::LeftDoorR | R::RightDoorF | R::RightDoorR => Self::Door,
            R::Roof => Self::Roof,
            R::Trunk => Self::Trunk,
            R::Windshield | R::RearGlass => Self::Glass,
            _ => Self::Structure,
        }
    }

    /// Code sent to the backend (`region` param).
    pub fn code(self) -> f32 {
        self as u8 as f32
    }

    /// (fundamental Hz, ring time constant s, plastic share 0..1) for an undamaged panel of a
    /// reference-size car. Fundamentals follow plate size (roof lowest), ring times follow
    /// damping (deadened doors short, the roof long, plastic barely rings).
    pub fn base(self) -> (f32, f32, f32) {
        match self {
            Self::Bumper => (260.0, 0.045, 1.0),
            Self::Hood => (105.0, 0.22, 0.0),
            Self::Wing => (190.0, 0.15, 0.0),
            Self::Door => (135.0, 0.11, 0.0),
            Self::Roof => (82.0, 0.32, 0.0),
            Self::Trunk => (120.0, 0.2, 0.0),
            Self::Glass => (520.0, 0.07, 0.0),
            Self::Structure => (165.0, 0.08, 0.0),
        }
    }
}

/// One hit as the director sees it (a debounced `SimEvent::Contact` side).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// Struck region on this vehicle (`None` = world geometry / unknown).
    pub region: Option<BodyRegion>,
    /// Material at the contact point.
    pub material: MaterialKind,
    /// Closing speed, m/s.
    pub speed_mps: f32,
    /// Normal impulse of the contact tick, N·s.
    pub impulse_ns: f32,
    /// Effective (reduced) mass of the collision, kg: the car's mass against the world.
    pub eff_mass_kg: f32,
    /// Mass of the struck vehicle, kg (panel size, thump pitch).
    pub mass_kg: f32,
    /// Damage of the struck panel before this hit, 0..1 (`RegionDamaged` severity / health).
    pub panel_damage: f32,
    /// The listener sits in this vehicle's cabin.
    pub interior: bool,
    /// Cosmetic seed for the backend's grain / mode scatter.
    pub seed: u32,
}

/// Excitation strength 0..1 of a hit: log of the collision energy, or the tick's impulse if
/// that is larger (a stationary car shoved hard).
pub fn strength(cfg: &ImpactSynthConfig, speed_mps: f32, impulse_ns: f32, eff_mass_kg: f32) -> f32 {
    let v = speed_mps.max(0.0);
    let e = 0.5 * eff_mass_kg.max(1.0) * v * v;
    let lo = cfg.energy_floor_j.max(1.0);
    let hi = cfg.energy_full_j.max(lo * 1.01);
    let by_energy = clamp01(ln_ratio(e.max(lo), lo) / ln_ratio(hi, lo));
    let by_impulse = clamp01(impulse_ns.max(0.0) / cfg.impulse_full_ns.max(1.0)).sqrt() * 0.8;
    by_energy.max(by_impulse)
}

/// Reduced mass of two bodies (a static world body has infinite mass).
pub fn reduced_mass(a_kg: Option<f32>, b_kg: Option<f32>) -> f32 {
    match (a_kg, b_kg) {
        (Some(a), Some(b)) => a * b / (a + b).max(1.0),
        (Some(m), None) | (None, Some(m)) => m,
        (None, None) => 1000.0,
    }
}

/// Size factor for panel modes and thump pitch: bigger vehicles, bigger and lower panels.
fn size_scale(cfg: &ImpactSynthConfig, mass_kg: f32) -> f32 {
    det_pow(cfg.ref_mass_kg / mass_kg.max(100.0), 0.2).clamp(0.7, 1.25)
}

/// Synth parameters of the `impact/body` voice for one hit (keys documented in
/// `apps/testbed-web/src/audio/impacts/README.md`).
pub fn body_params(cfg: &ImpactSynthConfig, h: &Hit) -> BTreeMap<String, f32> {
    let class = match (h.region, h.material) {
        (Some(r), _) => PanelClass::of(r),
        (None, MaterialKind::Plastic) => PanelClass::Bumper,
        (None, MaterialKind::Glass) => PanelClass::Glass,
        (None, _) => PanelClass::Structure,
    };
    let (mut f0, mut ring, mut plastic) = class.base();
    if h.material == MaterialKind::Plastic {
        plastic = plastic.max(0.8);
    }
    let s = strength(cfg, h.speed_mps, h.impulse_ns, h.eff_mass_kg);
    let heavy = clamp01((s - cfg.heavy_onset) / (1.0 - cfg.heavy_onset).max(0.05));
    if class == PanelClass::Bumper {
        // A hard hit goes through the plastic cover into the steel crash beam, rails and hood
        // behind it: lower, longer, metallic.
        plastic *= 1.0 - 0.75 * heavy;
        f0 += (140.0 - f0) * heavy;
        ring += (0.12 - ring) * heavy;
    }
    let d = clamp01(h.panel_damage);
    let size = size_scale(cfg, h.mass_kg);
    // Hard, fast contacts are short and bright; plastic and soft hits dull.
    let bright = clamp01(0.2 + h.speed_mps / 28.0 + 0.25 * s) * (1.0 - 0.45 * plastic);
    let contact_ms = (9.0 - 6.0 * clamp01(h.speed_mps / 25.0)) * (1.0 + plastic);
    let thump = clamp01((s - cfg.thump_onset) / (1.0 - cfg.thump_onset).max(0.05));
    let mut p = BTreeMap::new();
    let mut put = |k: &str, v: f32| {
        p.insert(k.to_string(), v);
    };
    put("kind", 0.0);
    put("region", class.code());
    put("strength", s);
    put("bright", bright);
    put("contact_ms", contact_ms);
    // A dented panel loses its curvature stiffness (modes drop), rings shorter, its modes
    // scatter (no longer one coherent plate) and loose edges buzz against their neighbours.
    put("f0", f0 * size * (1.0 - 0.12 * d));
    put(
        "ring_s",
        ring * (1.0 - 0.65 * d) * (1.0 + 0.4 * (1.0 - plastic) * s),
    );
    put("inharm", 0.015 + 0.1 * d);
    put("buzz", clamp01((d - 0.08) * 1.3) * (1.0 - 0.5 * plastic));
    put("plastic", plastic);
    put("modes", if s > 0.25 { 6.0 } else { 4.0 });
    put("thump", thump);
    put("thump_hz", (52.0 * det_pow(size, 1.25)).clamp(30.0, 72.0));
    put("transient", heavy);
    put("duck", heavy * cfg.duck_depth);
    put("duck_s", 0.08 + 0.25 * heavy);
    put("interior", if h.interior { 1.0 } else { 0.0 });
    put("seed", (h.seed & 0xFFFF) as f32);
    p
}

/// Synth parameters of an `impact/body` voice for broken glass (shatter + debris tail).
pub fn glass_params(
    region: BodyRegion,
    strength: f32,
    interior: bool,
    seed: u32,
) -> BTreeMap<String, f32> {
    // The windshield is laminated: it crazes with a duller crunch and sheds fewer pieces than
    // tempered side / rear glass, which bursts into a long tinkling tail.
    let laminated = region == BodyRegion::Windshield;
    let s = clamp01(strength);
    let mut p = BTreeMap::new();
    let mut put = |k: &str, v: f32| {
        p.insert(k.to_string(), v);
    };
    put("kind", 1.0);
    put("region", PanelClass::Glass.code());
    put("strength", s);
    put("bright", if laminated { 0.6 } else { 0.9 });
    put(
        "tinkles",
        if laminated { 14.0 } else { 26.0 } * (0.6 + 0.4 * s),
    );
    put("tail_s", if laminated { 0.8 } else { 1.4 });
    put("transient", 0.0);
    put("duck", 0.0);
    put("interior", if interior { 1.0 } else { 0.0 });
    put("seed", (seed & 0xFFFF) as f32);
    p
}

/// Per-vehicle deformation reservoir for the granular crunch loop.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CrunchState {
    /// Measured total plastic energy last frame, J (`None` until the first measurement).
    pub last_plastic_j: Option<f32>,
    /// Plastic work not yet "sounded", J.
    pub reservoir_j: f32,
}

impl CrunchState {
    /// Feed this frame's measured total plastic energy (J). A drop (repair) re-bases silently.
    pub fn measure(&mut self, total_plastic_j: f32) {
        if let Some(last) = self.last_plastic_j {
            let add = total_plastic_j - last;
            if add > 0.0 {
                self.reservoir_j += add;
            }
        }
        self.last_plastic_j = Some(total_plastic_j);
    }

    /// Add estimated plastic work (no measurement available), J.
    pub fn add_estimate(&mut self, j: f32) {
        self.reservoir_j += j.max(0.0);
    }

    /// Advance by `dt`; returns the crunch level 0..1 (log of the plastic power).
    pub fn step(&mut self, cfg: &ImpactSynthConfig, dt: f32) -> f32 {
        if self.reservoir_j.is_nan() || self.reservoir_j <= 0.0 || dt <= 0.0 {
            self.reservoir_j = self.reservoir_j.max(0.0);
            return 0.0;
        }
        let tau = cfg.crunch_tau_s.max(0.01);
        let power = self.reservoir_j / tau;
        self.reservoir_j *= det_exp2(-dt / tau * LOG2_E);
        if self.reservoir_j < 1.0 {
            self.reservoir_j = 0.0;
        }
        let lo = cfg.crunch_floor_w.max(1.0);
        let hi = cfg.crunch_full_w.max(lo * 1.01);
        if power <= lo {
            0.0
        } else {
            clamp01(ln_ratio(power, lo) / ln_ratio(hi, lo))
        }
    }
}

/// Stick-slip rate of a scrape, Hz, from the sliding speed.
pub fn scrape_rate_hz(cfg: &ImpactSynthConfig, slide_mps: f32) -> f32 {
    cfg.scrape_rate_base_hz + cfg.scrape_rate_per_mps * slide_mps.max(0.0)
}

/// Contact pressure 0..1 of a scrape from the tick's normal impulse.
pub fn scrape_force(cfg: &ImpactSynthConfig, impulse_ns: f32) -> f32 {
    clamp01(impulse_ns.max(0.0) / cfg.scrape_force_full_ns.max(1.0)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(speed: f32) -> Hit {
        Hit {
            region: Some(BodyRegion::FrontBumper),
            material: MaterialKind::Plastic,
            speed_mps: speed,
            impulse_ns: 500.0,
            eff_mass_kg: 1400.0,
            mass_kg: 1400.0,
            panel_damage: 0.0,
            interior: false,
            seed: 7,
        }
    }

    #[test]
    fn det_math_matches_libm() {
        for &x in &[1e-3f32, 0.1, 0.5, 1.0, 1.7, 2.0, 3.3, 100.0, 4.0e5] {
            assert!((det_log2(x) - x.log2()).abs() < 2e-4, "log2 {x}");
        }
        for &y in &[-20.0f32, -3.7, -1.0, -0.2, 0.0, 0.3, 1.0, 5.5, 18.2] {
            let r = det_exp2(y) / y.exp2();
            assert!((r - 1.0).abs() < 1e-4, "exp2 {y}: {r}");
        }
        assert!((det_pow(0.4375, 0.2) - 0.4375f32.powf(0.2)).abs() < 1e-4);
    }

    #[test]
    fn strength_grows_with_speed_and_saturates() {
        let c = ImpactSynthConfig::default();
        let s20 = strength(&c, 20.0 / 3.6, 0.0, 1400.0);
        let s50 = strength(&c, 50.0 / 3.6, 0.0, 1400.0);
        let s90 = strength(&c, 90.0 / 3.6, 0.0, 1400.0);
        assert!(s20 < s50 && s50 < s90, "{s20} {s50} {s90}");
        assert!(s20 > 0.3 && s90 > 0.9 && s90 <= 1.0, "{s20} {s90}");
        assert_eq!(strength(&c, 0.0, 0.0, 1400.0), 0.0);
        assert!(strength(&c, 300.0, 1e9, 1e9) <= 1.0);
        // A big impulse at low closing speed (pushed) still excites.
        assert!(strength(&c, 0.5, 6000.0, 1400.0) > 0.7);
    }

    #[test]
    fn heavy_layers_only_for_hard_hits() {
        let c = ImpactSynthConfig::default();
        let soft = body_params(&c, &hit(2.0));
        let hard = body_params(&c, &hit(25.0));
        assert_eq!(soft["transient"], 0.0);
        assert_eq!(soft["duck"], 0.0);
        assert!(hard["transient"] > 0.8 && hard["thump"] > 0.8 && hard["duck"] > 0.4);
        assert!(hard["bright"] > soft["bright"]);
        assert!(hard["contact_ms"] < soft["contact_ms"]);
    }

    #[test]
    fn regions_differ_and_damage_deadens() {
        let c = ImpactSynthConfig::default();
        let at = |r, d| {
            body_params(
                &c,
                &Hit {
                    region: Some(r),
                    material: MaterialKind::Panel,
                    panel_damage: d,
                    ..hit(10.0)
                },
            )
        };
        let roof = at(BodyRegion::Roof, 0.0);
        let door = at(BodyRegion::LeftDoorF, 0.0);
        let bumper = body_params(&c, &hit(10.0));
        assert!(roof["f0"] < door["f0"] && door["f0"] < bumper["f0"]);
        assert!(roof["ring_s"] > door["ring_s"] && door["ring_s"] > bumper["ring_s"]);
        // A soft bumper tap is all plastic.
        assert_eq!(body_params(&c, &hit(3.0))["plastic"], 1.0);
        // A hard frontal reaches the steel behind the cover.
        let hard = body_params(&c, &hit(25.0));
        assert!(
            hard["plastic"] < 0.5 && hard["f0"] < bumper["f0"] && hard["ring_s"] > bumper["ring_s"]
        );
        let dented = at(BodyRegion::LeftDoorF, 0.8);
        assert!(dented["ring_s"] < door["ring_s"] * 0.6);
        assert!(dented["buzz"] > 0.5 && door["buzz"] == 0.0);
        assert!(dented["inharm"] > door["inharm"]);
    }

    #[test]
    fn heavier_cars_thump_lower() {
        let c = ImpactSynthConfig::default();
        let car = body_params(&c, &hit(20.0));
        let truck = body_params(
            &c,
            &Hit {
                mass_kg: 3200.0,
                ..hit(20.0)
            },
        );
        assert!(truck["thump_hz"] < car["thump_hz"]);
        assert!(truck["f0"] < car["f0"]);
    }

    #[test]
    fn crunch_follows_plastic_power() {
        let c = ImpactSynthConfig::default();
        let mut st = CrunchState::default();
        st.measure(100.0);
        assert_eq!(st.step(&c, 1.0 / 60.0), 0.0, "first measurement only bases");
        st.measure(100.0 + 5_000.0);
        let small = st.step(&c, 1.0 / 60.0);
        let mut big = CrunchState::default();
        big.measure(0.0);
        big.measure(60_000.0);
        let large = big.step(&c, 1.0 / 60.0);
        assert!(small > 0.0 && large > small, "{small} {large}");
        // It decays to silence within a few time constants.
        let mut lvl = 1.0;
        for _ in 0..60 {
            lvl = big.step(&c, 1.0 / 60.0);
        }
        assert_eq!(lvl, 0.0);
        // Repair (energy drops) does not crunch.
        let mut r = CrunchState::default();
        r.measure(50_000.0);
        r.measure(0.0);
        assert_eq!(r.step(&c, 1.0 / 60.0), 0.0);
    }

    #[test]
    fn glass_tail_by_kind() {
        let ws = glass_params(BodyRegion::Windshield, 1.0, false, 1);
        let rear = glass_params(BodyRegion::RearGlass, 1.0, false, 1);
        assert!(rear["tinkles"] > ws["tinkles"] && rear["tail_s"] > ws["tail_s"]);
        assert_eq!(ws["kind"], 1.0);
    }

    #[test]
    fn scrape_rate_and_force() {
        let c = ImpactSynthConfig::default();
        assert!(scrape_rate_hz(&c, 15.0) > scrape_rate_hz(&c, 2.0));
        assert!(scrape_force(&c, 600.0) == 1.0 && scrape_force(&c, 60.0) < 0.5);
    }
}
