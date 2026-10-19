//! The lattice solver: extended position-based dynamics (XPBD) with force-limited beams.
//!
//! Why XPBD (see README): it is unconditionally stable for the very stiff steel beams we need
//! (k ≈ 10⁸ N/m on ~1 kg nodes) at a few sub-steps, its cost is linear in beams with a fixed
//! iteration count (predictable in WASM), and plasticity falls out naturally: a beam's
//! Lagrange multiplier *is* its force, so clamping it at the yield force makes the beam
//! perfectly plastic, and the motion it fails to resist becomes permanent set.

use wp_contracts::{MaterialKind, ModeTable, SimEvent};

use crate::model::{DamageModel, NO_GROUP};
use crate::vec3;

impl DamageModel {
    /// Advance the lattice by `t` seconds (one solver tick).
    pub(crate) fn solve(&mut self, t: f32, mode: &ModeTable, out: &mut Vec<SimEvent>) {
        if t <= 0.0 {
            return;
        }
        let tune = self.tuning.clone();
        let substeps = tune.substeps.max(1);
        let h = t / substeps as f32;
        let h2 = h * h;
        let flow = (tune.plastic_flow_rate * mode.plasticity_scale).clamp(0.0, 1.0);
        let n = self.pos.len();
        self.tick_start.clone_from(&self.pos);
        if mode.beam_break_scale != self.break_scale {
            self.break_scale = mode.beam_break_scale;
            for (c, h) in self.beams.iter().zip(self.hot.iter_mut()) {
                c.update_break_lengths(h, mode.beam_break_scale, &tune);
            }
        }

        for _ in 0..substeps {
            // Ringing after the impacts are spent is invisible; damp it hard so the lattice
            // goes back to sleep quickly.
            let damping = if self.impactors.is_empty() {
                tune.settle_damping_per_s
            } else {
                tune.node_damping_per_s
            };
            let damp = (1.0 - damping * h).clamp(0.0, 1.0);
            for imp in &mut self.impactors {
                imp.travel_prev = imp.travel;
                imp.travel += imp.vel * h;
                imp.age += h;
            }
            for i in 0..n {
                self.prev[i] = self.pos[i];
                if self.inv_mass[i] > 0.0 {
                    if self.has_ext_force {
                        self.vel[i] =
                            vec3::madd(self.vel[i], self.ext_force[i], self.inv_mass[i] * h);
                    }
                    self.pos[i] = vec3::madd(self.pos[i], self.vel[i], h);
                }
            }
            for _ in 0..tune.iterations.max(1) {
                for bi in 0..self.beams.len() {
                    self.project_beam(bi, h2);
                }
                for k in 0..self.impactors.len() {
                    self.project_impactor(k, h);
                }
            }
            for bi in 0..self.beams.len() {
                self.post_beam(bi, flow, h, out);
            }
            for i in 0..n {
                if self.inv_mass[i] > 0.0 {
                    self.pos[i] = vec3::clamp_box(self.pos[i], self.bounds_lo, self.bounds_hi);
                    self.vel[i] = vec3::scale(vec3::sub(self.pos[i], self.prev[i]), damp / h);
                }
            }
            for imp in &mut self.impactors {
                imp.vel = ((imp.travel - imp.travel_prev) / h).max(0.0);
            }
            self.stick_to_faces();
            let leak = (1.0 - h / tune.hinge_impulse_decay_s).clamp(0.0, 1.0);
            for g in &mut self.groups {
                g.impulse_ns *= leak;
            }
        }

        let max_age = tune.impactor_max_age_s;
        self.impactors
            .retain(|imp| imp.vel > 0.05 && imp.age < max_age);
        // Sleep on net motion over the tick, not per-sub-step velocity: stiff XPBD beams
        // jitter by ~1 µm per sub-step without going anywhere.
        let mut max_d2: f32 = 0.0;
        for i in 0..n {
            let d = vec3::sub(self.pos[i], self.tick_start[i]);
            max_d2 = max_d2.max(vec3::dot(d, d));
        }
        let still = tune.sleep_velocity_mps * t;
        if self.impactors.is_empty() && !self.has_ext_force && max_d2 < still * still {
            self.quiet_ticks += 1;
            if self.quiet_ticks >= tune.sleep_ticks {
                self.awake = false;
                for v in &mut self.vel {
                    *v = [0.0; 3];
                }
            }
        } else {
            self.quiet_ticks = 0;
        }
    }

    /// Inelastic contact: a node touching an impactor face cannot run ahead of it along the
    /// normal (without this, light panel nodes keep flying into the car after the face stops,
    /// since nodes do not collide with each other).
    fn stick_to_faces(&mut self) {
        for imp in &self.impactors {
            let n = imp.normal;
            for &(i, g) in &imp.nodes {
                let i = i as usize;
                if self.inv_mass[i] <= 0.0 {
                    continue;
                }
                let face = imp.face0 + imp.travel * g;
                if vec3::dot(self.pos[i], n) - face > 0.01 {
                    continue;
                }
                let vn = vec3::dot(self.vel[i], n);
                let cap = imp.vel * g;
                if vn > cap {
                    self.vel[i] = vec3::madd(self.vel[i], n, cap - vn);
                }
            }
        }
    }

    /// One XPBD projection of beam `bi` with its force clamped to the current yield limits.
    #[inline]
    fn project_beam(&mut self, bi: usize, h2: f32) {
        let b = &mut self.hot[bi];
        if b.broken {
            return;
        }
        let (ia, ib) = (b.a as usize, b.b as usize);
        let (wa, wb) = (self.inv_mass[ia], self.inv_mass[ib]);
        let w = wa + wb;
        if w <= 0.0 {
            return;
        }
        let d = vec3::sub(self.pos[ib], self.pos[ia]);
        let len = vec3::length(d);
        if len < 1e-6 {
            return;
        }
        b.len = len;
        let nrm = vec3::scale(d, 1.0 / len);
        let c = len - b.rest;
        // Compliance α = 1/k with k = EA / rest; α̃ = α / h².
        let alpha = b.rest * b.inv_ea / h2;
        let dl = (-c - alpha * b.lambda) / (w + alpha);
        // λ < 0 = tension, λ > 0 = compression; force = |λ| / h².
        let want = b.lambda + dl;
        let lo = -b.lim_t * h2;
        let hi = b.lim_c * h2;
        let got = want.clamp(lo, hi);
        b.at_limit = if want < lo {
            1
        } else if want > hi {
            -1
        } else {
            0
        };
        let dl = got - b.lambda;
        b.lambda = got;
        self.pos[ia] = vec3::madd(self.pos[ia], nrm, -wa * dl);
        self.pos[ib] = vec3::madd(self.pos[ib], nrm, wb * dl);
    }

    /// After a sub-step's iterations: plastic flow, hardening/buckling, breaking, energy
    /// ledger and detachable hinge impulse for beam `bi`; resets its multiplier.
    fn post_beam(&mut self, bi: usize, flow: f32, h: f32, out: &mut Vec<SimEvent>) {
        let hb = self.hot[bi];
        self.hot[bi].lambda = 0.0;
        self.hot[bi].at_limit = 0;
        if hb.broken {
            return;
        }
        // Fast path: elastic, not an attachment, inside its break lengths.
        if hb.at_limit == 0
            && hb.group == NO_GROUP
            && hb.len < hb.break_long
            && hb.len > hb.break_short
        {
            return;
        }
        let (ia, ib) = (hb.a as usize, hb.b as usize);
        if self.inv_mass[ia] + self.inv_mass[ib] <= 0.0 {
            return;
        }
        // Parts tear off under hard tension, not by being pushed into the car; residual
        // stress below a quarter of yield never accumulates (the integrator also leaks).
        if hb.group != NO_GROUP {
            let tension = -hb.lambda / (h * h);
            let over = tension - 0.25 * hb.lim_t;
            if over > 0.0 {
                self.groups[hb.group as usize].impulse_ns += over * h;
            }
        }
        // Exact length for yielding beams; the length seen by the last projection (off by at
        // most one correction) is good enough for everyone else's break test.
        let len = if hb.at_limit != 0 {
            vec3::length(vec3::sub(self.pos[ib], self.pos[ia]))
        } else {
            hb.len
        };
        let tune_min = self.tuning.min_length_ratio;
        let buckle_strain = self.tuning.buckle_strain;
        let cold = &mut self.beams[bi];
        let hot = &mut self.hot[bi];
        let mut work = 0.0;
        // Brittle beams (glass, cast) are force-limited but take no permanent set; they break
        // on strain below.
        match hb.at_limit {
            1 if !cold.is_brittle() => {
                let excess = len - hot.rest - hot.lim_t * hot.rest * hot.inv_ea;
                if excess > 0.0 {
                    let dr = flow * excess;
                    hot.rest += dr;
                    cold.plastic_strain += dr / cold.rest0;
                    work = hot.lim_t * dr;
                }
            }
            -1 if !cold.is_brittle() => {
                let excess = hot.rest - len - hot.lim_c * hot.rest * hot.inv_ea;
                if excess > 0.0 {
                    let new_rest = (hot.rest - flow * excess).max(tune_min * cold.rest0);
                    let dr = hot.rest - new_rest;
                    if dr > 0.0 {
                        work = hot.lim_c * dr;
                        hot.rest = new_rest;
                        cold.plastic_strain += dr / cold.rest0;
                        cold.comp_plastic_strain += dr / cold.rest0;
                        if !cold.buckled
                            && cold.comp_plastic_strain > buckle_strain
                            && matches!(cold.kind, MaterialKind::Structural | MaterialKind::Panel)
                        {
                            cold.buckled = true;
                        }
                    }
                }
            }
            _ => {}
        }
        let brk = len >= hot.break_long || len <= hot.break_short;
        if work > 0.0 {
            cold.energy_j += work;
            cold.update_limits(hot, &self.tuning);
            let regions = cold.regions;
            self.region_plastic_j[regions[0] as usize] += 0.5 * work;
            self.region_plastic_j[regions[1] as usize] += 0.5 * work;
        }
        if brk {
            self.break_beam(bi, out);
        }
    }
}
