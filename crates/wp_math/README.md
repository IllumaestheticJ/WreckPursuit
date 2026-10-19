# wp_math

**Owned by workstream 12 (integration lead); additions proposed by any workstream.**

Deterministic helpers: `FixedStep` accumulator, seeded `Rng` (xoshiro256**, SplitMix64 seeding — the only randomness allowed in sim code), `StateHasher` (FNV-1a 64 over quantised values), `lerp`/`nlerp_quat`/`smoothstep`, and polynomial `sin_cos`/`wrap_angle` that are bit-identical on native and WASM (use them instead of `f32::sin`/`cos` in sim code).

## Known gaps
- No `atan2`/`sqrt` replacements yet (`sqrt` is IEEE-exact so it is fine; `atan2` will be needed by 02).

## Perf
Not measured; all functions are a handful of flops.

## Tests

```sh
cargo test -p wp_math
```
