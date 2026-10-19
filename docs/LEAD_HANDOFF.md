# Lead Coordinator Handoff — WreckPursuit

**To:** the Claude Code session that will act as integration lead from now on.
**From:** the planning session that wrote `docs/DESIGN_OVERVIEW.md`, `CLAUDE.md` and `prompts/*.md`.
**Date:** 2026-10-18 (evening, America/Los_Angeles).

You are now the single point of coordination for this repo. Your standing instructions are `prompts/12_integration_lead.md`; this document gives you the state of the world, the people/machines involved, and the judgement calls that were made so you don't re-litigate them.

---

## 1. Who you are working with
- **The owner** is one person running many Claude Code sessions on separate machines. They are not a professional game developer or Rust engineer; they are a product owner with strong opinions about *feel* (Wreckfest handling, punishing damage, slow calculated police) and limited time per day. Talk to them in plain language, lead with what is playable and what you need from them, keep reports to ~10 lines, and never hand them a wall of Rust.
- **Workstream sessions** are other Claude Code instances. They cannot see each other. The only channels between them are: `main`, `docs/CONTRACT_CHANGELOG.md`, and `docs/integration/<name>.md`. Anything you want a workstream to know, you write into its integration doc under a `## Lead feedback (YYYY-MM-DD)` heading — the owner pastes the "resume" prompt and the session reads it.

## 2. State of the repo right now
- `main` = `9882862`. Prompt 01 is complete: `wp_contracts` v0.1.0 (loaders, schema generation, validation tests), example data (`debug_sedan` FWD, `debug_coupe` RWD, street tire, 5 materials, box lattices, arcade/sim mode tables), all stub crates, `apps/testbed-web` Vite+Three.js shell, `apps/simcli`, `apps/server` echo, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, `docs/STATUS.md`, `docs/integration/_TEMPLATE.md`.
- Workstreams started ~18:40 on 2026-10-18 (machines A–I = prompts 02–10). Branches already visible: `ws/vehicle`, `ws/testbed`, `ws/cars`. Others will appear as sessions finish.
- **Quirk:** some sessions also push to auto-named `claude/<adjective>-<name>-<id>` branches (e.g. `claude/charming-curie-3vttop` ≈ `ws/testbed`, `claude/magical-goldberg-k0vjje` ≈ `ws/cars`). Treat `ws/*` as canonical; if a `claude/*` branch is *ahead* of its `ws/*` twin, merge from the one that is ahead and tell the workstream to push to `ws/*` only. `claude/youthful-mayer-73fdu7` is a stale pre-01 branch — ignore it; ask the owner to delete it.
- Branch protection on `main` is **not** set. Ask the owner to enable it (only you merge). Until then, watch for direct pushes to `main` from workstreams and rebase them into proper branches.
- Prompts 13–20 (audio, weather, race/events/crews, UI kit, opponent AI, meta services, design docs, tuning dashboard) exist in `prompts/` and are **not started**. They are optional and independent. If the owner asks what to run next, recommend in this order: 19 (design docs — costs nothing, unblocks everyone's questions), 16 (UI kit), 15 (race rules), 13 (audio), 17, 14, 20, 18.

## 3. Ownership table (keep this current in `docs/STATUS.md`)

| # | Workstream | Branch | Owns | Status |
|---|---|---|---|---|
| 02 | Vehicle dynamics | `ws/vehicle` | `wp_vehicle`, `data/tires/` | running |
| 03 | Damage model | `ws/damage` | `wp_damage`, `data/lattices/` (generator), `data/materials/`, `data/modes/` | running |
| 04 | Browser testbed | `ws/testbed` | `apps/testbed-web`, `wp_wasm`, `apps/testbed-native` | pushed, unreviewed |
| 05 | City & traffic | `ws/world` | `wp_world`, `data/city/`, `tools/city-import` | running |
| 06 | Police | `ws/police` | `wp_police`, `data/police/` | running |
| 07 | Netcode | `ws/net` | `wp_net`, `apps/server`, `apps/testbed-web/src/net/` | running |
| 08 | Economy & mods | `ws/economy` | `wp_economy`, `data/mods/`, `data/economy/` | running |
| 09 | Car roster | `ws/cars` | `data/cars/`, `data/lattices/` (car files), `tools/specsheet` | pushed, unreviewed |
| 10 | Harness & CI | `ws/harness` | `apps/simcli`, `tools/bench`, `.github/workflows`, `wp_sim` test hooks | running |
| 12 | **Lead (you)** | `main` | `wp_contracts`, `schemas/`, `wp_sim` composition, all `docs/` top level | — |

Known overlap hazards to watch at merge time: 04 and 07 both touch `apps/testbed-web` (07 is confined to `src/net/`); 03 and 09 both touch `data/lattices/` (03 owns the generator and debug lattices, 09 owns the 13 car files); 05 and 06 both need the AI driving controller (05 builds it, 06 reuses — if 06 built its own, pick one and tell the other); 02 and 04 both may touch `wp_wasm` (prompt 02 was told to defer to 04 — check).

## 4. Decisions already made (don't reopen without the owner)
1. Rust workspace, Rapier3D (`enhanced-determinism`), custom vehicle model, custom node/beam lattice. Three.js/WASM for phase 1. Engine (Godot vs Unreal) decided at Checkpoint 3, not before.
2. Rigid chassis + deformable lattice (Wreckfest), **not** full soft body (BeamNG).
3. One damage code path, two TOML tables. Simulation calibrated to crash-test crush; Arcade is multipliers of Simulation.
4. 240 Hz fixed step, deterministic; multiplayer replicates contact impulses, never node positions.
5. Police: slow, calculated, mortal. Day/night is data in `data/police/*.toml`. No NFS-style cops. Per-unit risk budget.
6. Analog car names only; roster names in `data/cars/` are placeholders the owner may rename.
7. Art: prototype in "stylized low-poly, dark"; final style chosen at Checkpoint 3.
8. `HandlingEffects` is the entire damage→handling interface. Extend it rather than adding side channels.

## 5. Your first session (in order)
1. `git fetch --all`, read `docs/STATUS.md`, `docs/CONTRACT_CHANGELOG.md`, and every `docs/integration/*.md` that exists.
2. Review `ws/testbed` and `ws/cars` (first to land). For each: merge into a scratch branch off `main`, `cargo test --workspace`, `cargo build --target wasm32-unknown-unknown -p wp_wasm`, `just web` smoke. Record findings in their integration docs.
3. Collect contract change requests across all integration docs. Batch them into **one** `wp_contracts` bump (v0.2.0) rather than dribbling changes — every bump costs every workstream a rebase. Expected requests based on the prompts: `PriceTable`/`RepairEstimate` promotion (03/08), `HandlingEffects.tire_puncture` (06), `PoliceEvent`/`EconomyEvent`/`RaceEvent` payloads (06/08/15), `ContactImpulse.material_kind` (13), `WorldQuery::weather_at` (14), LOS query on `WorldQuery` (06).
4. Start composing `wp_sim` with whatever has merged: inputs → world clock → vehicles inside Rapier step → contacts → damage → `HandlingEffects` back → events drained. Feature-gate systems so partial builds run. Keep `just web` playable on `main` after every merge; revert first, investigate second.
5. Write `docs/STATUS.md` (per-workstream row + Checkpoint 1 checklist) and give the owner the 10-line report.

## 6. Checkpoint definitions (what "done" means)
- **Checkpoint 1 — Debug:** FWD + RWD debug cars drivable in the browser with a deforming lattice; barrier tests pass in both modes; replay round-trips; determinism suite green in CI.
- **Checkpoint 2 — Greybox:** procedural city with signal-obeying traffic; police patrol/pursue with day/night tables; heat/escape/capture loop; repair billing; 4-player server test.
- **Checkpoint 3 — Low-poly playable:** 13 cars validated; mods installable via garage; captain seat functional; 32-bot soak green; low-poly style applied; external playtest build; engine + art decisions written up in `docs/decisions/`.

## 7. Things the owner will ask you, and the answers
- *"What can I play right now?"* → whatever `just web` on `main` does today. Say it in one sentence.
- *"What should I run on a free machine?"* → the next unstarted prompt from §2's order, or a "resume" of a workstream that has Lead feedback waiting.
- *"Is X done?"* → quote `docs/STATUS.md`; if it isn't in there, it isn't done.
- *"Can we add feature Y?"* → put it in `docs/design/FEATURE_BACKLOG.md` (prompt 19 creates it; create it yourself if not yet) with a one-line cost estimate and which crate it lands in. Do not start it mid-sprint.
- *"It doesn't feel like Wreckfest."* → that is a tuning session, not a code change: point them at the testbed debug layers, replays, and `tools/tuning-dash` (prompt 20) once it exists, and open a tuning note in `docs/integration/vehicle.md` or `damage.md`.

## 8. Non-negotiables you enforce at every merge
Determinism (no wall clock, no unordered iteration, no threads in the sim step, f32 hot paths) · no native-only deps reachable from `wp_wasm` · no licensed car names anywhere · one damage code path · `main` always builds and `just web` always runs · every merged workstream has a current `docs/integration/<name>.md`.

Hand this document back to the owner updated if the lead role ever moves again.
