//! Road styles of Port Vesper v3: class, lanes, speed, parking, grade cap and banking per
//! kind of road (US defaults: residential 25 mph, collector 30–35, arterial 35–45,
//! highway 55–65).

use crate::builder::{mph, RoadClass};

/// How a road is built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Functional class.
    pub class: RoadClass,
    /// Lanes forward (along the road's direction).
    pub fwd: u8,
    /// Lanes backward (0 = one-way).
    pub bwd: u8,
    /// Speed limit, m/s.
    pub speed: f32,
    /// Kerbside parking both sides.
    pub parking: bool,
    /// Kerbside bus lane each direction (needs ≥ 2 lanes).
    pub bus: bool,
    /// Grade cap.
    pub grade: f32,
    /// Largest superelevation (cross slope) in curves.
    pub bank_max: f32,
}

impl Style {
    /// Same style with another speed limit.
    pub const fn at(self, mph_: f32) -> Self {
        Self {
            speed: mph_ * 0.447_04,
            ..self
        }
    }

    /// Same style with `n` lanes each way.
    pub const fn lanes(self, n: u8) -> Self {
        Self {
            fwd: n,
            bwd: if self.bwd == 0 { 0 } else { n },
            ..self
        }
    }

    /// One-way version with `n` lanes.
    pub const fn one_way(self, n: u8) -> Self {
        Self {
            fwd: n,
            bwd: 0,
            ..self
        }
    }

    /// Without parking.
    pub const fn no_parking(self) -> Self {
        Self {
            parking: false,
            ..self
        }
    }

    /// With kerbside parking.
    pub const fn with_parking(self) -> Self {
        Self {
            parking: true,
            ..self
        }
    }

    /// Two-way.
    pub fn two_way(&self) -> bool {
        self.bwd > 0
    }

    /// Half paved width (widest side; one-way roads are centred), m.
    pub fn half_width(&self) -> f32 {
        let w = match self.class {
            RoadClass::Highway => 3.7,
            RoadClass::Alley => 3.0,
            _ => 3.5,
        };
        let p = if self.parking { 2.5 } else { 0.0 };
        if self.bwd == 0 {
            self.fwd as f32 * w * 0.5 + p
        } else {
            self.fwd.max(self.bwd) as f32 * w + p
        }
    }

    /// Rank for clipping and de-duplication decisions (higher keeps).
    pub fn rank(&self) -> u8 {
        self.class.rank() * 4 + self.fwd.max(self.bwd).min(3)
    }
}

const fn s(
    class: RoadClass,
    fwd: u8,
    bwd: u8,
    mph_: f32,
    parking: bool,
    grade: f32,
    bank: f32,
) -> Style {
    Style {
        class,
        fwd,
        bwd,
        speed: mph_ * 0.447_04,
        parking,
        bus: false,
        grade,
        bank_max: bank,
    }
}

/// Highway carriageway centreline style (both directions; split into carriageways later).
pub const HIGHWAY: Style = s(RoadClass::Highway, 3, 3, 55.0, false, 0.055, 0.06);
/// Interchange ramp (one-way, one lane, 45 mph).
pub const RAMP: Style = s(RoadClass::Highway, 1, 0, 45.0, false, 0.07, 0.06);
/// Loop ramp (one-way, one lane, 25 mph advisory).
pub const LOOP_RAMP: Style = s(RoadClass::Highway, 1, 0, 25.0, false, 0.07, 0.06);
/// Major arterial 2+2, 40 mph.
pub const ARTERIAL: Style = s(RoadClass::Arterial, 2, 2, 40.0, false, 0.08, 0.02);
/// Downtown avenue (collector 1+1 with parking, 30 mph): signals at every junction.
pub const AVENUE: Style = s(RoadClass::Collector, 1, 1, 30.0, true, 0.09, 0.0);
/// Collector 1+1, 30 mph.
pub const COLLECTOR: Style = s(RoadClass::Collector, 1, 1, 30.0, false, 0.10, 0.0);
/// Local residential street 1+1, 25 mph.
pub const LOCAL: Style = s(RoadClass::Local, 1, 1, 25.0, false, 0.11, 0.0);
/// Local street without parking (suburbs, service).
pub const STREET: Style = s(RoadClass::Local, 1, 1, 25.0, false, 0.11, 0.0);
/// Industrial / port service road 1+1, 25 mph.
pub const SERVICE: Style = s(RoadClass::Local, 1, 1, 25.0, false, 0.08, 0.0);
/// Port road 2+2 collector, 35 mph.
pub const PORT: Style = s(RoadClass::Collector, 2, 2, 35.0, false, 0.06, 0.0);
/// Mid-block alley 1+1, 15 mph.
pub const ALLEY: Style = s(RoadClass::Alley, 1, 1, 15.0, false, 0.11, 0.0);
/// Hill road (Hollins Heights) 1+1, 25 mph, 12 % cap.
pub const HILL: Style = s(RoadClass::Local, 1, 1, 25.0, false, 0.12, 0.03);
/// Scenic two-lane (Bluff Road, Skyline Drive) 1+1 collector.
pub const SCENIC: Style = s(RoadClass::Collector, 1, 1, 35.0, false, 0.10, 0.04);

/// Speed limit of a mph value (re-export for layouts).
pub fn speed(m: f32) -> f32 {
    mph(m)
}
