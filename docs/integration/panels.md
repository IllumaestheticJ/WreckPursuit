# Panels (car models, destructible panels, hinges) integration notes

Copy to `docs/integration/<name>.md` and keep it current. The integration lead reads every one
of these daily.

- **Workstream / prompt:** 21 — car models & panels (machine R)
- **Branch:** `ws/panels`
- **Crates / folders owned:** `crates/wp_damage/src/panels.rs`, `data/panels/`, `tools/assets/`, `apps/testbed-web/src/render/`
- **Contract version built against:** 0.2.0 (v0.3.0 in progress)

## Exposes
Public API other crates/apps may call (type/function signatures, file formats, WASM functions).

## Needs
What this workstream needs from others (crate, item, by when) and current workaround.

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|

## Changes outside my crates
Minimal edits made to other crates to compile (file, reason). Should normally be empty.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|

## Known gaps
What is stubbed, approximated or broken.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)

## Lead feedback (round 6b, 2026-10-19)
New workstream. Do everything in `prompts/21_car_models_panels.md`.
