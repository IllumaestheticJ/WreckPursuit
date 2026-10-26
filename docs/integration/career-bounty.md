# Career bounty integration notes

- **Workstream:** career bounty (what `PlayerAccount::bounty` does in the browser career)
- **Branch:** `claude/project-thread-21oo4x`
- **Owner request (2026-10-26):** "what is the current career function of bounty?" → it only set
  the bail at a capture. Owner: "implement all 3" of the offered options: the police know the
  bounty, the bounty can be paid off, and high-stakes races are gated on it and pay a bonus.
- **Contract version built against:** current `wp_contracts` (no contract edits; the payoff
  posts the existing `BountyCleared` reason with a cash delta, the race bonus an `Adjustment`).

## What the bounty does now

| | Where | Tuning |
|---|---|---|
| Grows on escapes and races won while wanted (unchanged) | `capture.rs`, `payouts.rs` | `stakes.toml [bounty.escape]`, `[bounty.race_won_while_wanted]` |
| Bail = 0.5 × bounty at a capture, then cleared (unchanged) | `capture.rs` | `consequences.toml bail_bounty_frac` |
| **Police know it.** Every city run starts with the career bounty × `career_carry` in the racer's police bounty, so reinforcements come sooner (`priority_max_reduction`) and up to `priority_extra_units` more cars come (× priority, rounded). 10 000 career bounty = max priority. A capture + "Repair & continue" resets it to the (cleared) account bounty. | `wp_police::PoliceSystem::carry_career_bounty`, `BountyRecord::carried`, `dispatch::wanted_units`; called from `wp_wasm::career::Career::car_session` and `Session::repair_and_continue` | `police/heat.toml [bounty] career_carry = 2.0`, `police/presence.toml [reinforcement] priority_extra_units = 2` |
| **Pay it off** in the garage (new Police tab): cost = 0.75 × bounty (min 100), one `BountyCleared` line. Dearer than bail, but no fine / impound / rep loss. | `wp_economy::Economy::{bounty_payoff_cost, pay_off_bounty}`; `career_pay_off_bounty()` | `stakes.toml [bounty.payoff]` |
| **Informants** (existed in the economy, never reachable in the career): the Police tab sells the three tips; each lowers the carried heat the next run starts with. Refused with no carried heat. | `Career::buy_informant`; `career_buy_informant(tier)` | `stakes.toml [informant.*]` |
| **High-stakes races** (illegal daytime meets; of the picker races since PR #82 that is Loop 9 day only, Loop 9 night and the Loop 9 to Ashpeak highway run are `street`) are invite only below 500 career bounty, and pay +10 % of the purse per 1 000 bounty (max +50 %), posted as an `Adjustment` "high-stakes bounty bonus". Career picker cards carry `bounty_needed` / `bounty_locked` / `bounty_bonus`; `Career::start_run` refuses a locked one ("Can't race: … invite only"). | `wp_economy::Economy::{is_high_stakes, high_stakes_bounty_needed, require_high_stakes_bounty, bounty_purse_bonus}`; `wp_wasm::race::RaceRun::pay_race`, `Career::race_cards` | `stakes.toml [bounty.high_stakes]` |

Damage-mode rule: nothing here branches on `DamageMode`.

## Known gaps
- Career runs with a carried bounty change police behaviour from tick 0 outside the replay log
  (like the restored lattice damage): such a replay does not verify.
- The in-chase HUD bounty now starts at the carried value (career bounty × `career_carry`), so
  it reads higher than the garage number.
- Bounty gained mid-run (an escape) is not fed back to the police until the next run.
- The quick-race picker outside a career shows no gate or bonus (no career bounty there).

## Changes outside my crates
- `wp_police`: `BountyRecord::carried` (serde default), `BountyConfig::career_carry`,
  `ReinforcementConfig::priority_extra_units` (both serde default), `carry_career_bounty`.
- `apps/ui-kit` garage Police tab; `apps/testbed-web` race picker lock / bonus, results line,
  wasm wiring.
