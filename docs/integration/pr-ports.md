# PR ports onto round12 — integration notes

- **Workstream:** PR-ports helper for the integration lead (three open PRs built on `main`)
- **Branch:** `lead/pr-ports` (from `lead/round12` abea6fe8; round12 5080fa80 merged in before
  the gate). Never pushed to `lead/round12` or `main`.
- **Method:** cherry-pick only the listed commits and adapt them; no branch merges from `main`.
- **Contract version built against:** 0.4.1 (no contract change).

## PR #53 — testbed-web audio: stop the whole mix dropping out in busy scenes (`ws/audio-dropouts`)

Took **e0f9f643** (cherry-pick -x).

- `voices.ts` applied cleanly: listener and panner positions are plain per-block `.value`
  writes, the strip low-pass is a plain value unless it jumps more than half an octave (20 ms
  glide), gain and pitch keep their glides.
- Conflict `audio/index.ts`: round12's `new AudioMixer(ctx, lib.config.master)` (master-stage
  settings) kept, with the PR's `latencyHint: "balanced"`.
- Conflict `docs/integration/audio.md`: both sections kept (round12's siren / radio sections,
  then the PR's "Owner report (2026-10-23)" section).
- Checked round12's newer audio for the same pattern: engine groups (recorded packs /
  procedural V8) go through the patched `applyParams`; sirens v2, the scanner and the radio
  tiers do not automate panner positions. `siren_lab.ts` (developer tool) still does; left.
- The Air-1 rotor added for PR #32 uses plain panner writes from the start.

## PR #54 — wp_police: spike strips only exist while their police unit is beside them (`ws/spikes`)

Took **281187e4** [bless], adapted; **skipped 31cf6bb1** as asked (Extreme-mode repair
property; needs main #48). #48 has since reached round12 through main-sync, and round12's
`wp_economy` `full_wreck_in_simulation_costs_at_least_60_percent` passes as it is, so nothing
is missing without it.

E (police workstream) was messaged before starting; `ws/police` had no commits beyond
round12 at the time of the gate, so there was nothing of E's to merge.

Details in `docs/integration/spikes.md`. Conflict resolutions / round12 adaptations:

- **Row 131 / 158 (no police car appears in a racer's view) vs. "a strip only exists with its
  car".** round12 laid the strip at once and held the car back until its spot was hidden — the
  reported bug. Holding the strip back with the car kills spikes (police scenarios: spike hits
  10 -> 0 and 16 -> 0). First port: spike cars appeared with the strip even in view (hits 8 / 14).
  **Final (lead, 2026-10-23):** the strip goes down at once; on the next think its car appears
  beside it if the spot is hidden (counted in the pop-in audit), else no car and the strip stays
  **unmanned**, lifted once its racer is 25 m past it, the pursuit ends or it expires. Hits:
  9 / 17. Roadblock cars still wait as before.
- **Row 75 prop clearance:** the spike car's shoulder spot must pass `spot_clear` (parked cars,
  cones, vehicles); `tools::plan_site_where` takes the first rule-valid site that does.
- **Row 75 upright reset / shoves:** the strip is lifted when its unit is more than
  `SPIKE_UNIT_LEASH_M` (15 m) from where it parked (`SpikeStrip::post`), checked at think rate.
- **Row 84 search units:** a spike unit pulled into a search (or any reassignment) leaves
  `SpikeAt`, which lifts its strip at once (hook in `assign`, and in `despawn`).
- **Row 156 (spikes at heat level 4):** unchanged; `place_spikes_at` bypasses rules as before.
- **Race profiles / `SpikeStrip::scripted`:** **exempt** (E agrees). Scripted strips have no unit, and no
  profile has a stakeout beside its strip (only `grid_highway_sprint` has a strip, at 0.85;
  its stakeouts are at 0.3 / 0.55). A scripted strip that is ever given a unit follows the rule.
- **wp_sim:** `withdraw_unbodied_spike_unit` hook in `reconcile_police_units` (as in the PR).
- **Golden:** `golden_spike_strip` re-blessed twice (on the old base, then on the merged round12
  whose replay header moved to 0.4.1): the strip now has its unit, the hash moves from tick 24
  exactly as on `main`; the scenario's expectations pass; no other golden moved.

## PR #32 — Helicopter as Air-1 (`claude/bold-ride-zezrp2`)

Took **ab57236b, f98943fb, 96203483** — client side only, re-implemented on round12's
architecture (the PR's files did not apply: round12's maps are canvas renderers fed by
`map_dynamic()`, round12 already has the helicopter model, spotlight and call signs).

- **Not taken (round12 already has it or supersedes it):** the `wp_police` part (call sign
  "Air-1" renumbering, follow call-outs every 30 s, `helicopter.callout_s`, `helo_callouts`):
  round12 has "Air <unit id>" call signs and `helo_reports` (eyes on / lost visual / reacquired
  on **tier 1**, position reports on tier 4), i.e. the pilot lines already go through the
  radio priority tiers and the "Radio: All / Important only" setting. Kept round12's
  "Air <id>" naming everywhere ("AIR 6" on the map), not "Air-1". Not taken either: the
  testbed helicopter model / spotlight (`heli_view.ts`), `police_helicopters(player)` wasm
  signature, the run.mjs stub step (replaced by a real-helicopter check in `helicopter.mjs`).
  The helicopter deploys at heat 5 (row 156) — untouched; the e2e uses the debug launch hook.
- **Taken:**
  - rotor + pilot-radio audio (`audio/heli.ts`): pilot lines get a rotor-chopped cockpit bed
    through a harder radio EQ instead of the static, a mic-key chirp (not on tier-1 lines,
    which already get the two-tone priority alert), a lower / quicker voice; the in-world rotor
    plays at the nearest helicopter (rows of `police_helicopters()`, which main.ts already
    fetches: no extra wasm call). Panner positions are plain writes (PR #53).
  - **+12 % loudness (`HELI_LOUDNESS = 1.12`) kept** on the pilot radio and rotor:
    `audio_mix.mjs` passes (its scenes have no helicopter).
  - minimap / full-map helicopter icon (ui-kit `map/draw.ts`): top-down silhouette in a new
    `--heli` yellow, call sign upright, dashed tether + pulsing ring to whoever it follows
    (the player's own position when it is on the player). `MinimapStats.tethers`.
  - chase-HUD radar (`ChaseHud`): the SVG minimap with the helicopter icon / tether, shown only
    while the testbed's corner minimap layer is off (`LiveFrame.radar.hidden`,
    `HudState.radarHidden`); the "AIR <id> on you" note sits in the chase status line (clear of
    the radio strip) and shows either way. `map/data.ts` `chaseRadar()` builds it from the map
    frame the testbed already decodes (`ui/map.ts`).
  - pilot badge: `ScannerEntry.air` (reducer flags "Air <n>…" unit lines), AIR badge in the
    HUD alerts, chase HUD and scanner feed.
  - tests: ui-kit `chaseRadar` and reducer tests; `e2e/helicopter.mjs` air-unit check.

## Gate (on the merged head; env CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0)

| Step | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --no-fail-fast` | pass (188 test binaries) |
| `simcli goldens` (golden set) | pass after the `golden_spike_strip` bless (only golden that moved) |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm` | ok |
| `npm run wasm && npx tsc --noEmit -p .` (testbed-web) | ok |
| ui-kit `tsc --noEmit`, `vitest run` | tsc ok; 113 / 114 — `MockProvider.test.ts` "toy captain follows wp_police rules" fails the same on round12 without these ports (it expects spikes at L3; row 156 moved them to L4) |
| e2e `helicopter.mjs` | pass (incl. the new air-unit check) |
| e2e `police_tools.mjs` | pass (golden_spike_strip live = simcli) |
| e2e `radio.mjs` | pass |
| e2e `audio.mjs`, `audio_mix.mjs`, `audio_engine.mjs`, `audio_roster.mjs`, `audio_samples.mjs` | pass |
| e2e `audio_impacts.mjs` | 3 FAIL, **pre-existing**: same 3 with round12's own `src/audio` (A/B run): "20 km/h ducks less than 90 km/h", "interior: more body, less air", "90 km/h crash broke the windshield and its tail is heard" |

## Lead follow-ups (2026-10-23), done
1. Spike cars no longer appear in view: unmanned strip fallback (above).
2. Chase radar only while the corner minimap is off; "on you" note moved into the status line.
3. The spike car's spawn is counted in the row 131 pop-in audit (`record_spawn`).
4. ui-kit `MockProvider.test.ts` updated: spikes refused at L3 ("Unlocks at wanted level 4"),
   allowed once the recorded chase reaches L4, then cooling down.
5. Merged `origin/lead/round12` b0269195 (Ashpeak); `ws/police` 4ca05ced (row 156) was already
   in it.

`audio_impacts.mjs` keeps its 3 pre-existing failures (left as the lead asked).
