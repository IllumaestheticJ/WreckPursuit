//! The lattice solver: extended position-based dynamics (XPBD) with force-limited beams.
//!
//! Why XPBD (see README): it is unconditionally stable for the very stiff steel beams we need
//! (k ≈ 10⁸ N/m on ~1 kg nodes) at a few sub-steps, its cost is linear in beams with a fixed
//! iteration count (predictable in WASM), and plasticity falls out naturally: a beam's
//! Lagrange multiplier *is* its force, so clamping it at the yield force makes the beam
//! perfectly plastic, and the motion it fails to resist becomes permanent set.

use wp_contracts::{MaterialKind, ModeTable, SimEvent};

use crate::model::DamageModel;
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
        let damp = (1.0 - tune.node_damping_per_s * h).clamp(0.0, 1.0);
        let n = self.pos.len();

        for _ in 0..substeps {
            for imp in &mut self.impactors {
                imp.travel_prev = imp.travel;
                imp.travel += imp.vel * h;
                imp.age += h;
            }
            for i in 0..n {
                self.prev[i] = self.pos[i];
                if self.inv_mass[i] > 0.0 {
                    self.pos[i] = vec3::madd(self.pos[i], self.vel[i], h);
                }
            }
            for b in &mut self.beams {
                b.lambda = 0.0;
                b.at_limit = 0;
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
                self.post_beam(bi, flow, mode.beam_break_scale, h, out);
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
        let max_v2 = self
            .vel
            .iter()
            .map(|v| vec3::dot(*v, *v))
            .fold(0.0, f32::max);
        if self.impactors.is_empty() && max_v2 < tune.sleep_velocity_mps * tune.sleep_velocity_mps {
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
        let b = &mut self.beams[bi];
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
        let nrm = vec3::scale(d, 1.0 / len);
        let c = len - b.rest;
        // Compliance α = 1/k with k = EA / rest; α̃ = α / h².
        let alpha = b.rest / (b.ea * h2);
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
    /// ledger and detachable hinge impulse for beam `bi`.
    fn post_beam(
        &mut self,
        bi: usize,
        flow: f32,
        break_scale: f32,
        h: f32,
        out: &mut Vec<SimEvent>,
    ) {
        let tune_min = self.tuning.min_length_ratio;
        let buckle_strain = self.tuning.buckle_strain;
        let plastic_crush = self.tuning.plastic_crush_fail_strain;
        let b = &self.beams[bi];
        if b.broken {
            return;
        }
        let (ia, ib) = (b.a as usize, b.b as usize);
        if self.inv_mass[ia] + self.inv_mass[ib] <= 0.0 {
            return;
        }
        let len = vec3::length(vec3::sub(self.pos[ib], self.pos[ia]));
        let mut work = 0.0;
        let mut brk = false;
        {
            let b = &mut self.beams[bi];
            // Parts tear off under hard tension, not by being pushed into the car; residual
            // stress below a quarter of yield never accumulates (the integrator also leaks).
            if let Some(g) = b.group {
                let tension = -b.lambda / (h * h);
                let over = tension - 0.25 * b.lim_t;
                if over > 0.0 {
                    self.groups[g].impulse_ns += over * h;
                }
            }
            match b.at_limit {
                // Brittle beams (glass, cast) are force-limited but take no permanent set;
                // they break on strain below.
                1 => {
                    if !b.is_brittle() {
                        let excess = len - b.rest - b.lim_t * b.rest / b.ea;
                        if excess > 0.0 {
                            let dr = flow * excess;
                            b.rest += dr;
                            b.plastic_strain += dr / b.rest0;
                            work = b.lim_t * dr;
                        }
                    }
                }
                -1 => {
                    if !b.is_brittle() {
                        let excess = b.rest - len - b.lim_c * b.rest / b.ea;
                        if excess > 0.0 {
                            let new_rest = (b.rest - flow * excess).max(tune_min * b.rest0);
                            let dr = b.rest - new_rest;
                            if dr > 0.0 {
                                work = b.lim_c * dr;
                                b.rest = new_rest;
                                b.plastic_strain += dr / b.rest0;
                                b.comp_plastic_strain += dr / b.rest0;
                                if !b.buckled
                                    && b.comp_plastic_strain > buckle_strain
                                    && matches!(
                                        b.kind,
                                        MaterialKind::Structural | MaterialKind::Panel
                                    )
                                {
                                    b.buckled = true;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            let strain0 = (len - b.rest0) / b.rest0;
            let fail = b.fail_strain * break_scale;
            let broke = match b.kind {
                MaterialKind::Glass | MaterialKind::Cast => strain0.abs() > fail,
                // Polymer also cracks when crushed flat.
                MaterialKind::Plastic => strain0 > fail || -strain0 > plastic_crush * break_scale,
                _ => strain0 > fail,
            };
            if b.breakable && broke {
                brk = true;
            }
            if work > 0.0 {
                b.energy_j += work;
                let tuning = &self.tuning;
                b.update_limits(tuning);
            }
        }
        if work > 0.0 {
            let regions = self.beams[bi].regions;
            self.region_plastic_j[regions[0] as usize] += 0.5 * work;
            self.region_plastic_j[regions[1] as usize] += 0.5 * work;
        }
        if brk && self.beams[bi].breakable {
            self.break_beam(bi, out);
        }
    }
}
