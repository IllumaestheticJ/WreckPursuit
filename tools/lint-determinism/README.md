# tools/lint-determinism

**Owned by workstream 10.** Line-based lint for the simulation crates (`wp_contracts`,
`wp_math`, `wp_vehicle`, `wp_damage`, `wp_world`, `wp_police`, `wp_economy`, `wp_sim`).
Errors: `f64`, libm transcendentals (`.sin()`, `.atan2()`, `.powf()`, `.exp()`, …, also
`f32::sin` forms), `HashMap`/`HashSet`, wall clock (`std::time`, `Instant`, `SystemTime`),
threads (`std::thread`, `rayon`), unseeded RNG (`rand::`, `thread_rng`, `getrandom`).
Warning: `std::f32::consts` (exact, but check nothing libm-based hides nearby).

Skips comments, string literals, `src/bin/`, and everything after a file's first
`#[cfg(test)]`. Suppress one line with `// det-allow(<rule>): <reason>`, a whole file with an
entry in `allow.txt` (`<path> <rule> <reason>`; stale entries fail).

```sh
cargo run -p lint-determinism          # report; exit 1 on errors
cargo test -p lint-determinism         # unit tests + the workspace audit (also in cargo test --workspace)
```

## Known gaps

Textual, so it cannot see through macros, function pointers or re-exports (`use f32::sin as s`).
It is a tripwire; the native-vs-wasm hash comparison is the real check.
