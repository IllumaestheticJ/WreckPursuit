//! # wp_math
//!
//! Deterministic math helpers shared by every simulation crate:
//! - [`FixedStep`]: accumulator turning variable frame time into fixed physics ticks.
//! - [`Rng`]: seeded xoshiro256** generator. The **only** source of randomness in sim code.
//! - [`StateHasher`]: stable FNV-1a 64 hasher over quantised values for determinism checks.
//! - Interpolation helpers and polynomial [`sin_cos`] (bit-identical on native and WASM).
//!
//! Owned by the integration lead. Workstreams propose additions in their integration doc.

#![warn(missing_docs)]

mod fixed_step;
mod hash;
mod interp;
mod rng;
mod trig;

pub use fixed_step::FixedStep;
pub use hash::{quantize, StateHasher};
pub use interp::{clamp01, lerp, lerp3, nlerp_quat, smoothstep};
pub use rng::Rng;
pub use trig::{sin_cos, wrap_angle};
