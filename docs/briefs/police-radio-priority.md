# Brief: police radio — priority tiers, heat scaling, and chaos on impact

**Playtest row:** 144 (owner, 2026-10-22).
**Repo:** `IllumaestheticJ/WreckPursuit`. Base your branch on `lead/round12` (latest head) and
push to your own branch. **Do not push to `lead/round12` or `main`.**
**Read first:** `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`,
`docs/integration/police-radio-v2.md` (the current radio design; this brief builds on it).

## What exists today (police radio v2)
- `crates/wp_police/src/chatter.rs`: makes the lines from police events (`PitAttempt`,
  `UnitDisabled`, `SpikeHit`, …), contacts and state diffs. Each line starts with a call sign.
- `crates/wp_police/src/radio.rs` + `data/police/radio.toml`: pacing. One transmission at a
  time. Gaps are per heat level (`global_gap_s`, `unit_gap_s`, `min_gap_s`, stale times).
  The estimated line length is `words / 2.6 + 0.7 s`.
- `crates/wp_audio/src/scanner.rs`: the radio sound path (squelch, static).
- `apps/testbed-web/src/audio/scanner.ts`: voices, through browser text-to-speech
  (`speechSynthesis`) with a radio filter.
- `apps/testbed-web/src/radio_strip.ts`: the on-screen strip, showing the last 3 lines.
- The settings have a voice toggle.

## What the owner wants
"Filtering between important and unimportant information."
1. **Low heat (L0–L1, no active pursuit):** officers investigating is fine ("suspicious vehicle",
   "checking the area", "last seen near …"). Constant announcements of units deploying are not.
   At most one summary line ("units responding") when heat first rises, then silence about
   deployments until something actionable happens.
2. **As heat rises, driver-useful information takes priority:** roadblocks deploying, spikes
   deploying, the helicopter deploying.
3. **Next in priority:** heat levels increasing, and tougher departments (more aggressive cars)
   taking over.
4. **Then impacts and wrecks:** the chaos and violence should come across over the radio.

## Priority tiers (high → low)
| Tier | Content | Behaviour |
|---|---|---|
| 1 Actionable | Roadblock deploying (where); spike strip deploying (where); helicopter deploying, arriving or losing you; a unit ahead of you or intercepting | Always gets through and pre-empts or cuts any lower line. A short distinct radio-tone cue before the voice. Strongest strip highlight. Announced when the tool is **planned or deploying**, not when the player is already on it. Includes direction and distance ("roadblock ahead on Lantern Row, 400 m"). |
| 2 Escalation | Heat level up or down; a tougher department taking over ("Highway Patrol taking over pursuit", "heavy units authorised", "rams authorised") | Pre-empts tiers 3–5. Highlighted. |
| 3 Impacts / wrecks | Rams, PITs, units hit or disabled, officers wrecked, civilian crashes caused by the chase | **Chaotic and violent**: shouted lines ("I'm hit! I'm hit!", "Unit 4 is down, unit 4 is down!", "He rammed me, I've lost my front end!", "Officer down, need EMS!"). The line is picked by impact severity (sim energy and damage) and by who was hit. Louder and more distorted, with a static burst and shorter gaps. A big wreck may stack two quick lines (the victim, then another unit reacting) if nothing in tier 1–2 is waiting. TTS: faster rate and higher pitch for these. |
| 4 Investigation | Low-heat search lines | Only when the channel is free. |
| 5 Routine | Acknowledgements, "copy that", plate and description repeats, generic status, routine "unit en route / on patrol / available" | Only when the channel is free and nothing important happened in the last few seconds. Dropped first when busy. Thins out as heat rises. **No routine deploy announcements at L0–L1.** At L2+, deployment lines only when actionable (tier 1). |

- The priority rises with heat: at L2+, tier 1 dominates.
- **Strip:** tiers 1–2 highlighted (colour or bold plus an icon) and kept on screen longer; tier 3
  in an urgent style; tiers 4–5 dimmed.
- **Setting:** "Radio: Important only / All" (default All). "Important only" plays tiers 1–3.
  Wire it into the existing voice toggle and menu (testbed and ui-kit).

## Rules
- Determinism: the radio decisions in `wp_police` stay deterministic. Use no wall-clock time, and
  use the seeded RNG for line variety.
- `wp_contracts` is owned by the integration lead. If you need a contract field (e.g. a priority
  on `PoliceEvent::Radio`), propose it in `docs/integration/police-radio-v2.md` under "Contract
  change requests", or carry the tier inside `wp_police` and the bridge.
- No licensed or real names. Street names come from the game's own road names on `World`.
- Every public item gets a doc comment. Update the crate READMEs.

## Tests / acceptance
- Unit tests for the tier order:
  - tier 1 cuts a routine line mid-air;
  - routine lines are dropped when busy;
  - no deploy lines at L0–L1 except the one summary.
- At L3, a roadblock or spike deployment line is spoken within ~1 s of the deploy decision,
  even if a routine line is playing.
- Impact lines are picked by severity: a light tap is never "officer down"; a wreck is never a
  calm line.
- A bench on `radio_city_chase.ron` (and one race with police), before and after:
  - lines per minute per tier at L0 / L1 / L2 / L3+;
  - deploy lines per minute at L0 / L1 / L2+;
  - delay from deploy decision to spoken line at L1 / L2 / L3.
- 5 example transcripts from chases: at least one big wreck, one department takeover, and one
  roadblock and spike sequence.
- Gate before pushing:
  - `cargo fmt`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test -p wp_police -p wp_audio`;
  - `cargo build --target wasm32-unknown-unknown -p wp_wasm`;
  - `cd apps/testbed-web && npx tsc --noEmit -p .`;
  - the e2e radio/siren tests.
- Re-bless goldens only if police-in-scene goldens move, with the reason in the commit.
  Radio is presentation, so ideally none move.

## Also list (don't build)
What a voice upgrade would take, replacing browser text-to-speech with recorded or generated
voice lines: several distinct officers plus dispatch, shouted variants for tier 3, and the radio
processing chain.

## Report back
Branch and hash, the bench tables, the example transcripts, and any contract change request.
