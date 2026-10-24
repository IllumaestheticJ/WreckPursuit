# glamx 0.3.1 — WreckPursuit patch

Unmodified glamx 0.3.1 from crates.io except for one change, wired in through
`[patch.crates-io]` in the workspace `Cargo.toml`:

- `src/eigen3.rs` (`SymmetricEigen3::eigenvalues`, the 3x3 symmetric eigen solver): `acos` and
  `cos` go through the `libm` crate (`libm::Libm::<$Real>`) instead of the inherent f32/f64
  methods; `Cargo.toml` gains the `libm` dependency.

Why: the inherent methods call the platform math library — glibc natively, Rust's libm port on
wasm32 — and the two can disagree in the last bit. parry calls this solver for mass properties
derived from convex hulls and trimeshes (`MassProperties::with_inertia_matrix`) and to seed
convex-hull construction (`convex_hull3::initial_mesh`), so native and wasm32 runs of the same
scenario diverged (`tools/wasm-determinism/compare.sh`: `jam/pursuit_jam_grid`,
`jam/pursuit_jam_vesper`; traced to a parked car's dented-hull inertia and to a building hull's
face normals). glamx's own `libm` feature does not reroute inherent method calls.

`src/svd2.rs` and `src/pose2.rs` still use inherent trig; they are 2D only and not on the 3D
path. Drop this patch once upstream glamx routes its transcendental calls through libm.
