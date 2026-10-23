# Spike strips always manned — integration notes

- **Workstream:** spike strip with no police car (playtest bug), PR #54 (`ws/spikes`)
- **Ported onto:** `lead/round12` by the PR-ports helper (`lead/pr-ports`), from 281187e4
- **Crates / folders:** `wp_police` (dispatch, tools), plus a small hook in `wp_sim::police_link`
- **Contract version built against:** 0.3.1 (no contract change)

## What changed (row 165, 2026-10-23 — supersedes the unmanned-strip fallback)
Owner row 165 ("unaccompanied spikes"): both must hold — (a) a spike strip never exists
without its police unit beside it, (b) no police car ever appears in a racer's view (rows 131 /
158).

- **Dynamic strips** (AI / human captain): a request creates a `dispatch::SpikeRun`; nothing is
  on the road yet (`spike_strips_pending()`, `tool_cars_waiting(tool)` show it).
  - Site search: the first rule-valid site along the racer's predicted route (150–500 m,
    ≥ 120 m approach sight) whose car post passes `spot_clear` (row 75) and is hidden from the
    racer (graph sight lines, now and along its road ahead); else the first rule-valid site.
  - Next think (`maintain_spike_runs`, which has the world): post hidden (`spawn_in_view` false)
    → the car appears there and the strip goes down with it. Post in view → a car spawns out of
    view (`spike_drive_in`: lane spot 40–900 m from the post by road, hidden, clear, not on the
    racer's far carriageway, route keeps 60 m from the racer and arrives 4 s before it) and
    drives in with an ambush-style route; the strip is laid when it is within 8 m of its post
    and stopped. No spot yet → retried each think.
  - Given up, no strip (`stats.spikes_given_up`): racer within `SPIKE_LATE_M` (40 m) of the
    strip's line or past it, pursuit over, or the strip's lifetime gone; a car driving in goes
    out of service.
  - The radio's "spikes N metres ahead" line is still said at the decision
    (`spikes_decided` / `spikes_announced`; the later `SpikesPlaced` is not announced again).
- **Scripted race-profile strips** (`races.toml [[x.spikes]]`): each gets its own unit parked on
  the shoulder on the strip's side at race setup, through the row 158 view check (as stakeouts);
  a strip whose post a racer can already see is not placed.
- **Lifting:** every tick (`lift_unmanned_spikes`, after the units move) a strip whose unit is
  wrecked, reassigned (e.g. a row 84 search), gone, or more than `SPIKE_UNIT_LEASH_M` (15 m)
  from its post is lifted; also on `assign` / `despawn` of its unit.
- Removed: the unmanned-strip path (spike `PendingToolCar`, `SPIKE_PASSED_M`).
- Kept: spikes from wanted level 4 (row 156); `place_spikes_at` (simcli scenarios / tests)
  spawns its unit directly; the `wp_sim` withdraw hook for a spike unit without a car.

### Spike hits
| Measure | main (strip down at once, unmanned if needed) | row 165 |
|---|---|---|
| `police_scenario tool_placement` (5 runs) | 9 | 0 |
| `police_scenario radio_city_chase` (5 runs) | 17 | 0 |
| road-keeping racer, AI captain, 12 seeds (`ai_spikes_hit_a_racer_that_keeps_its_road`) | — | 10 / 12 seeds hit |
| `grid_highway_sprint` scripted strip (`race_picker`) | 2 | 2 |

Why the scenario numbers drop: with the full view check (camera + road ahead) a post
150–500 m ahead is practically always in view, so every dynamic strip is a drive-in, laid
~10 s after the decision; these scripted racers turn at random at every junction, so by then
they have left the predicted road (`tool_placement` 2 runs: 9 requested, 7 drive-ins,
2 given up; `radio_city_chase`: 11, 3, 8). On `main` their hits came from strips lying there
from the decision on — what row 165 forbids. A racer that keeps its road is hit in 10 of 12
seeds.

## Exposes
- `PoliceSystem::withdraw_unbodied_spike_unit(unit) -> bool`
- `wp_police::dispatch::SPIKE_UNIT_LEASH_M`, `SPIKE_ARRIVE_M`, `SPIKE_LATE_M`
- `PoliceSystem::spike_strips_pending()`; `tool_cars_waiting(tool)` also reports a strip's car post while it is on its way
- `PoliceStats::spikes_given_up`, `spike_drive_ins`
- `SpikeStrip::post: Option<[f32; 2]>` (serde default)
- `wp_police::tools::plan_site_where`

## Changes outside my crates
- `crates/wp_sim/src/police_link.rs` (`reconcile_police_units`): after the body spawn loop, call
  `withdraw_unbodied_spike_unit` for units still without a car.
- Row 165: `crates/wp_race/tests/race_picker.rs` checks every strip is manned after every tick.

## Tests
- `tests/spikes_manned.rs`: `tool_placement` / `radio_city_chase` (2 runs each): after every
  step every strip has its unit (on it, not wrecked, ≤ 15 m from its post); no tool car spawned
  in a racer's sight (pop-in audit).
- `wp_race/tests/race_picker.rs`: the same strip check after every tick of every picker race.
- `tests/system.rs`: `spike_strip_never_lies_without_its_unit`,
  `spike_strip_lifted_when_its_unit_leaves_the_post`,
  `unbodied_spike_unit_is_withdrawn_with_its_strip`; the captain-tools test (strip pending
  until laid with its car, then hit); `ai_spikes_hit_a_racer_that_keeps_its_road`.
- `race.rs` `spikes_cover_part_of_the_course_only`: the scripted strip has its unit.
- `tests/divided_tools.rs`: reads strips on their way too.

## Goldens
- Row 165: `jam_pursuit_grid` re-blessed — the AI captain's first spike decision is at tick
  24408; on `main` the strip went down at once, now it waits for its car, so the hash moves at
  the next checkpoint (tick 26400). No other golden moved (`golden_spike_strip` uses
  `place_spikes_at`, unchanged).
- Earlier (PR #54 port): `golden_spike_strip` re-blessed when its strip got its unit (tick 24).

## Follow-ups tried (row 165)
- **Re-plan** (cancel the drive-in and pick a new site when the racer leaves the predicted road
  before the strip is down): not kept. In the random-walk scenarios no strip went down at all
  any more (the racer keeps leaving the road, runs thrash and are given up), hits stayed 0, and
  the road-keeping measure was unchanged (10 / 12).
- `police_scenario tool_placement` / `radio_city_chase`: the reason for ~0 spike hits is written
  in the scenario files; no spike-hit expectation there.

## Gate (row 165, d589145c + bless 03b2b2b1)
- fmt, clippy `-D warnings`, simcli goldens (only `jam_pursuit_grid` moved; blessed), wasm32
  build, `npm run wasm` + tsc, e2e `police_tools` and `helicopter`: pass.
- `cargo test --workspace`: one failure, `wp_opponent` `tests/race.rs`
  `live_police_chase_ends_in_escape_or_capture_without_stuck_cars` ("busted EntityId(1) still
  moving 6.0 m/s"; passes on `main`). Spikes now reach this chase (4 drive-in strips); after
  its capture at ~82 s racer 1's AI keeps "rolling on at 6 m/s until at the kerb and clear of
  any junction" (`wp_opponent::field`, pull-over) and never meets that condition before the run
  ends, likely with spike-flattened tyres. No police car pushes it. Opponent-workstream code;
  not changed here.

## Known gaps
- A drive-in spike car can be seen driving to its post (it never appears in view).
- A run whose racer leaves the predicted road keeps going until too late; re-planning is not
  done.
- Roadblocks have the same shape (barriers can outlive their units); not changed here.
