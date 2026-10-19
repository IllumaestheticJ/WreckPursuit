# PROMPT 12 — Integration lead (run daily on the lead machine; owns `wp_contracts`, `wp_sim`, `main`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, `docs/STATUS.md`, and every file in `docs/integration/`. You are the only session allowed to edit `wp_contracts`, `schemas/`, and the game-logic composition in `wp_sim`. Branch work happens on `main` via merge commits.

## Daily loop
1. `git fetch --all`; list `ws/*` branches with new commits since last integration.
2. For each branch: rebase or merge into a scratch integration branch, run `cargo test --workspace`, the WASM build, `simcli` determinism suite and golden replays. Triage failures: contract drift vs. real bugs. Fix trivial drift yourself; for real bugs write a precise note into that workstream's `docs/integration/<name>.md` under "Lead feedback" (file, test, expected vs actual) and skip the branch this round.
3. Process **contract change requests** from all integration docs: accept, reject or modify; apply to `wp_contracts`, bump the version, write `docs/CONTRACT_CHANGELOG.md` entries (what changed, why, who needs to update), regenerate schemas, update example data.
4. Compose `wp_sim`: the fixed-step loop that, each physics tick, (a) applies inputs, (b) steps `wp_world` clock/traffic LOD, (c) steps all `VehicleSim`s inside Rapier's step, (d) collects contact manifolds → `ContactImpulse`s → `wp_damage::apply_contacts`, (e) pushes `HandlingEffects` back to vehicles, (f) steps `wp_police`, (g) drains events to `wp_economy` and the net layer, (h) hashes state when asked. Keep system registration feature-gated so partial builds run.
5. Merge green branches to `main`, push, update `docs/STATUS.md` (per-workstream: merged commit, what works end-to-end now, what's blocked and on whom, perf vs budget) and `docs/PERF.md`.
6. Produce a 10-line human status report (plain text) at the end: what's playable today, what changed in contracts, decisions needed from the owner.

## Standing responsibilities
- Protect determinism: reject anything with wall-clock time, unordered iteration, threads in the sim step, or new f64 hot paths.
- Protect the browser: reject native-only deps reaching `wp_wasm`.
- Protect the design pillars (see `06` header and `00_README_OVERVIEW.md` §1): no invincible cars, no NFS-style cops, one damage code path with two tables, analog car names only.
- Keep `just web` playable on `main` at all times; if a merge breaks it, revert first, investigate second.
- Maintain the **milestone checklist** in `docs/STATUS.md`:
  - Checkpoint 1 (Debug): drivable FWD + RWD debug cars in browser with deforming lattice, barrier tests pass in both modes, replay works, determinism suite green.
  - Checkpoint 2 (Greybox): procedural city with traffic obeying signals; police patrol/pursue with day/night tables; heat/escape/capture loop; repair billing; 4-player server test.
  - Checkpoint 3 (Low-poly playable): 13 cars validated, mod tree installed via garage, captain seat functional, 32-bot soak green, style B applied, external playtest build.
- When the owner asks for a decision (engine choice, art style), prepare a one-page `docs/decisions/NNN-<topic>.md` with options, costs, and a recommendation.
