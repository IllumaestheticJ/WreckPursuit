# Playtest log (owner feedback)

## 2026-10-19 — first city chase on the published link
| # | Feedback | Owner of the fix | Status |
|---|---|---|---|
| 1 | No city buildings on the published link | lead (artifact can't serve .glb → base64 fallback in `scene.ts`) | fixed, link v5 |
| 2 | No visible car damage | lead: lattice view on by default; real denting panels = R (prompt 21) | interim fix on link |
| 3 | Cars feel really slow | helper `lead/playtest-feel`: browser sim perf (sim fell behind real time at ~35–50 ms/frame in city chase) + car speed/feel | dispatched |
| 4 | Transmission is sloppy | helper `lead/playtest-feel` (shift logic) | dispatched |
| 5 | Use real open-source sounds | J (audio): CC0 recorded packs | dispatched |
| 6 | Dispatcher and cops are way too talkative | helper `lead/playtest-radio`: `wp_police::radio` director (critical lines always; tactical/routine behind 8 s / 20 s global and 30 s per-unit gaps; duplicates merged; patrol chatter only when wanted or within 200 m; `data/police/radio.toml`) + HUD feed max 3 lines fading after 6 s. 5-min night city chase (`wp_police` scenario `radio_city_chase`, 4 seeds): **in pursuit 30.8 → 5.6 lines/min, not wanted 20.5 → 3.4/min**; calm patrol (`radio_calm_patrol`) 1.0 → 0.1/min. Goldens unchanged (radio not hashed) | fixed on `lead/playtest-radio` |
| 7 | Camera zooms way too far out; need more angles | helper `lead/playtest-camera` | dispatched |

### Row 6 gate notes (`lead/playtest-radio`)
- fmt, `clippy --workspace --all-targets -D warnings`, `lint-determinism`, wasm32 build, ui-kit + testbed `tsc`, ui-kit vitest: pass.
- `cargo test --workspace`: everything passes except `simcli goldens_match`. Eight police-city goldens
  (city_chase, night_pursuit_escape, opponent ×4, race_city_sprint, traffic_soak) diverge at tick 24.
  This comes from the required merge of `origin/ws/police` (round 6 obstacle awareness): `origin/main` passes;
  the merge commit *before* any radio change fails with the same hashes as after it, so the radio change
  hashes the same. Nothing re-blessed here; E or the lead blesses with the police merge.
- `web-e2e`: everything passes except "city chase … speeding violations in the feed" (heat stays 0). It fails
  the same way on the merge commit without the radio change.
