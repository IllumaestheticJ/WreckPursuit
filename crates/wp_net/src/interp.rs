//! Remote-vehicle state buffers: delta baselines and render interpolation.
//!
//! Remote vehicles render [`INTERP_DELAY_TICKS`] (100 ms) behind the estimated server tick,
//! interpolating between the two bracketing snapshots; past the newest snapshot they
//! extrapolate along velocity for at most [`MAX_EXTRAP_TICKS`] (50 ms) and then hold.

use std::collections::VecDeque;

use wp_math::{lerp3, nlerp_quat};

use crate::quant::{QVehicle, RemoteState};

/// Interpolation delay, physics ticks (100 ms).
pub const INTERP_DELAY_TICKS: f64 = 24.0;
/// Maximum extrapolation, physics ticks (50 ms).
pub const MAX_EXTRAP_TICKS: f64 = 12.0;
/// Received states kept per entity (baselines + interpolation).
pub const HISTORY_LEN: usize = 96;

/// Received states of one remote vehicle, ascending by tick.
#[derive(Debug, Clone, Default)]
pub struct StateBuffer {
    samples: VecDeque<(u64, QVehicle)>,
}

impl StateBuffer {
    /// Empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a state received for `tick` (out-of-order arrivals are placed correctly;
    /// duplicates are ignored).
    pub fn push(&mut self, tick: u64, q: QVehicle) {
        let idx = self.samples.partition_point(|(t, _)| *t < tick);
        if self.samples.get(idx).is_some_and(|(t, _)| *t == tick) {
            return;
        }
        self.samples.insert(idx, (tick, q));
        while self.samples.len() > HISTORY_LEN {
            self.samples.pop_front();
        }
    }

    /// State received for exactly `tick` (delta baseline lookup).
    pub fn at(&self, tick: u64) -> Option<&QVehicle> {
        self.samples
            .binary_search_by_key(&tick, |(t, _)| *t)
            .ok()
            .map(|i| &self.samples[i].1)
    }

    /// Newest `(tick, state)`.
    pub fn latest(&self) -> Option<(u64, &QVehicle)> {
        self.samples.back().map(|(t, q)| (*t, q))
    }

    /// State to render at fractional server tick `render_tick`.
    pub fn sample(&self, render_tick: f64) -> Option<RemoteState> {
        let (first_tick, first) = self.samples.front()?;
        if render_tick <= *first_tick as f64 {
            return Some(first.decode());
        }
        let (last_tick, last) = self.samples.back()?;
        if render_tick >= *last_tick as f64 {
            let mut s = last.decode();
            let dt =
                (render_tick - *last_tick as f64).min(MAX_EXTRAP_TICKS) / crate::TICK_HZ as f64;
            for i in 0..3 {
                s.pos[i] += s.vel[i] * dt as f32;
            }
            return Some(s);
        }
        let idx = self
            .samples
            .partition_point(|(t, _)| (*t as f64) <= render_tick);
        let (ta, a) = &self.samples[idx - 1];
        let (tb, b) = &self.samples[idx];
        let t = ((render_tick - *ta as f64) / (*tb - *ta) as f64) as f32;
        let (a, b) = (a.decode(), b.decode());
        let mut s = if t < 0.5 { a } else { b };
        s.pos = lerp3(a.pos, b.pos, t);
        s.rot = nlerp_quat(a.rot, b.rot, t);
        s.vel = lerp3(a.vel, b.vel, t);
        s.speed_mps = a.speed_mps + (b.speed_mps - a.speed_mps) * t;
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(y_cm: i32, vy: i16) -> QVehicle {
        QVehicle {
            pos: [0, y_cm, 0],
            rot: crate::quant::q_rot([0.0, 0.0, 0.0, 1.0]),
            vel: [0, vy, 0],
            ..Default::default()
        }
    }

    #[test]
    fn interpolates_extrapolates_and_holds() {
        let mut b = StateBuffer::new();
        b.push(100, q(0, 1000));
        b.push(108, q(80, 1000)); // positions are arbitrary; velocity is 10 m/s
        b.push(104, q(40, 1000)); // out of order
        b.push(104, q(9999, 0)); // duplicate ignored
        assert_eq!(b.at(104).unwrap().pos[1], 40);
        let mid = b.sample(102.0).unwrap();
        assert!((mid.pos[1] - 0.2).abs() < 1e-5);
        // 6 ticks past the newest: extrapolate 6/240 s at 10 m/s.
        let ex = b.sample(114.0).unwrap();
        assert!((ex.pos[1] - (0.8 + 0.25)).abs() < 1e-4);
        // Far past: capped at 12 ticks.
        let held = b.sample(300.0).unwrap();
        assert!((held.pos[1] - (0.8 + 0.5)).abs() < 1e-4);
        assert_eq!(b.sample(0.0).unwrap().pos[1], 0.0);
    }
}
