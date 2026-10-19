# Damage barrier scenarios

`wp_damage::barrier::BarrierScenario` files (not `simcli::Scenario`): one car, damage mode and
impact kind at several closing speeds, with expected ranges, required component failures /
survivors, total-loss verdicts and event ordering. Run by `cargo test -p wp_damage`
(`tests/barrier_scenarios.rs`, twice each, hashes must match) and by

```sh
cargo run --release -p wp_damage --bin damage-barrier -- crates/wp_damage/scenarios/*.ron
```

`simcli`'s own scenario test skips this folder. Ranges are deliberately loose (calibration
targets, not golden numbers); determinism is checked by hash.
Impact kinds: `Frontal` (full-width rigid barrier), `Offset` (40 % overlap, left), `Side`
(T-bone by a car of `other_mass_kg` centred on the front door), `Rear` (full-width rigid).
