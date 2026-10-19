# Lead automation — how a round runs without the owner pasting prompts

Set up 2026-10-19 at the owner's request ("option 1 + 3"). The lead session runs on a schedule;
each run is one **round**. The owner only plays the published testbed and makes design calls.

## Why fresh sessions
A Claude session treats messages from another session as unverified and asks its owner first.
A session's **first prompt** is its assignment, so the lead starts a new cloud session per
workstream per round with the task as that first prompt. Context lives in the repo (integration
docs, Lead feedback, changelog), so nothing is lost. Old sessions are left alone (the owner may
archive them).

## Registry

| Machine | Prompt | Branch | Integration doc |
|---|---|---|---|
| A | 02 vehicle | `ws/vehicle` | `vehicle.md` |
| B | 03 damage | `ws/damage` | `damage.md` |
| C | 04 testbed | `ws/testbed` | `testbed.md` |
| D | 05 world | `ws/world` | `world.md` |
| E | 06 police | `ws/police` | `police.md` |
| F | 07 net | `ws/net` | `net.md` |
| G | 08 economy | `ws/economy` | `economy.md` |
| H | 09 cars | `ws/cars` | `cars.md` |
| I | 10 harness | `ws/harness` | `harness.md` |
| J | 13 audio | `ws/audio` | `audio.md` |
| K | 14 weather | `ws/weather` | `weather.md` |
| L | 15 race | `ws/race` | `race.md` |
| M | 16 UI kit | `ws/ui` | `ui.md` |
| N | 17 opponent AI | `ws/opponent` | `opponent.md` |
| O | 18 meta | `ws/meta` | `meta.md` |
| P | 19 design docs | `ws/design-docs` | `design-docs.md` |
| Q | 20 tuning dash | `ws/tuning-dash` | `tuning-dash.md` |
| R | 21 car models & panels | `ws/panels` | `panels.md` |
| J2 | 13 audio (recorded samples) | `ws/audio-samples` | `audio.md` |
| S | 11 art direction (low-poly dark) | `ws/art` | `art.md` |

Lead helper sessions (title `WP Lead helper · <task>`) work on `lead/*` branches (e.g.
`lead/contract-v0.3.0`, `lead/test-speed`); the lead merges them like `ws/*` branches.

Session titles: `WP <letter> · <name> · round <n>`, so the lead can find this round's sessions
with `list_sessions`.

## One round (what each scheduled lead run does)

1. **Collect.** `git fetch --all`; list `ws/*` branches ahead of `main`. Read every integration
   doc changed since the last round (contract requests, questions to the lead).
2. **Skip busy workstreams.** A workstream is busy if a session titled for it is still
   working (`list_sessions` / `get_session`: status `RUNNING`, or a push to its branch in the
   last 20 minutes). Busy branches are merged only if they are green; no new session starts.
3. **Merge.** Merge each ahead branch into the lead branch, fix drift, and run the full gate:
   fmt, clippy `-D warnings`, `cargo test --workspace`, the wasm build plus no native deps,
   the determinism lint, all scenarios, goldens, native vs wasm, `just web-e2e`, and `tsc`.
   Check exit codes, not grep output. Re-bless goldens only for understood changes, in a
   `[bless]` commit. A branch that breaks the gate is not merged: it gets precise Lead feedback.
4. **Push** `main` and the lead branch. Republish the testbed artifact (same file path / link).
5. **Lead work.** Contract bumps (batched), `wp_sim` composition, cross-workstream fixes.
6. **Write feedback.** For every workstream with something to do, add a dated
   `## Lead feedback (round N)` section to its integration doc: done items, failures (file,
   test, expected vs actual), next tasks. Push.
7. **Dispatch.** For each non-busy workstream with feedback items, `create_session` (this repo,
   `outcome_branch` = its `ws/*` branch) with the first prompt:
   > You are machine X (WreckPursuit workstream NN — name). git pull origin main; check out and
   > merge main into ws/<name> (create it from main if missing). Read CLAUDE.md,
   > docs/INTEGRATION.md, docs/CONTRACT_CHANGELOG.md, docs/STATUS.md, prompts/NN_*.md and
   > docs/integration/<doc>.md — do every item in the latest "Lead feedback" section. Stay in
   > your own crates/folders; anything needed elsewhere goes into your integration doc as a
   > request. Run fmt, clippy -D warnings, cargo test --workspace, the wasm build, and your
   > scenarios; update your integration doc; commit often; push ws/<name>.
8. **Report** to the owner in ≤ 10 plain lines, only when something changed: what is new on the
   link, what needs a decision. A quiet round stays quiet.

## Owner switches
- Pause: tell the lead "pause automation" (it disables the scheduled task).
- Keep a workstream local: say so; the lead stops dispatching it and lists its prompt instead.
