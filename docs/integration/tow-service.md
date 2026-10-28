# Tow service (career)

Owner request, 2026-10-28: "add an option for us to tow broken cars to repair shops".

- **Workstream:** lead helper, branch `lead/tow-service`
- **Crates / folders touched:** `wp_economy` (`tow.rs`, ledger reason, profile car spots, session
  flag, `prices.toml [tow]`), `wp_wasm` (`tow.rs`, career, run, garage_visit shop positions,
  debug hook), `apps/ui-kit` (access rules, `garage/Tow.tsx`, garage screen, pause menu),
  `apps/testbed-web` (in-run picker, prompt, key, wreck panel button, e2e `tow.mjs`).
- **Contract version built against:** 0.3.x (no contract edits; one request below).

## What existed before

- A flat per-class `towing` charge (`prices.toml [class.*] towing`, Beater 120 … Exotic 450),
  posted as the `Towing` penalty (may create debt) when a run ended with an **uncaptured total
  loss** (`EconomySession::finish`, `PlayerRun::settle`). No destination: the wreck was "towed
  home".
- In the drive-to career screen (`access.ts`), a car that could not be driven had its repairs
  open **anywhere** ("the shop's tow truck brings it in"), priced at a shop of the home city. The
  `Access.tow` / `repairShop` fields existed for that rule.
- Nothing remembered where a car physically was between runs; runs always start at home.

## What it does now

**In a run** (career runs only):
- "Call a tow truck" from the pause menu (`#gm-tow`), the new rebindable **tow** action
  (Backspace / d-pad left), the wreck panel ("Call a tow truck" next to "Garage"), and a prompt
  at the bottom of the screen whenever the car cannot be driven on: **wrecked** (total loss),
  **engine dead**, **wheel off**, **flipped** (on its side/roof, still, 2 s) or **stuck**
  (throttle held, not moving, 5 s). Flipped/stuck use the sim tick only (`wp_wasm::tow::TowWatch`).
- The pop-up lists destinations with distance and price. Picking one charges the tow on the
  run's account, ends the run (one night, as "back to the garage"), and opens the garage at the
  destination: the player is "at" that place in the drive-to sense, so its repairs/workshop open.
  The car arrives with its damage (lattice untouched).

**In the garage screen**: a car that cannot be driven and is not at a repair shop (a wreck left
on the road, a dead car at home) gets a **"Tow to …"** panel in the Cars tab. Every other action
stays drive-to. A car's work now follows **where the car sits** when it cannot be driven
(`CarView.spot`): at the shop it was towed to, everything of that shop is open (even if the
player opened the screen from the menu); on the road, nothing but the tow; at home, the workshop
but no repairs (repairs are shop work).

**Destinations** (`wp_wasm::tow::destinations`): the nearest repair shop (first, marked
"nearest"), every other repair shop, and every unlocked home garage. Not offered: the place the
car already sits at, or a home garage the car is standing in (within 25 m).

**Price** (`data/economy/prices.toml [tow]`, `wp_economy::tow`): `callout + per_km × road km`,
road km = straight-line distance × `route_factor`, rounded to 100 m. Data: callout 150, per_km
100, route_factor 1.3. Resulting prices: next-door shop ≈ $200, across Port Vesper $270–$320,
Port Vesper → Harnell ≈ $630, Port Vesper → Ashpeak City ≈ $750, Harnell → Ashpeak City (the
longest) ≈ $890 — against 4 500 starting cash (the longest tow is about a fifth of it) and roughly 900–3 000 for
an early race win (`crates/wp_economy/README.md`, "Prices against the car roster"). Positions are
fixed points of the combined world (`WORLD_SHOPS[].pos`, checked against the generated world in
`spawn_tests::world_shops_match_the_generated_world`; home anchors), so the garage screen can
price without a world.

**Can't pay**: the nearest shop is **on account**: it tows anyway and the shortfall becomes debt
(the ledger's existing penalty rule: negative cash = debt, which blocks voluntary spends until
paid). Other destinations need the cash. Not stuck: in debt the economy-floor beater is free, and
the wreck can be scrapped at the shop.

**Ledger**: one line per tow, `EventReason::Tow { to, metres }` (a penalty like `Towing`); the
garage shows it as "Tow: Meridian Motorworks → 0.5 km".

**Police**: a car in the pound gets no tow ("release it from the Impound tab"). Wanted or chased
(live: pursued or wanted level > None; garage: carried heat at L1+) → refused "Not with the
police on you." unless the car is a total loss. A totaled car keeps the capture rules: during a
run the tow waits for the settlement (`PlayerRun`'s 0.5 s grace); a capture impounds it (no tow),
an uncaptured wreck may be towed. Police outcomes are unchanged.

**Career runs no longer auto-charge the flat towing** for an uncaptured wreck
(`SessionConfig::auto_tow = false` for career runs, `PlayerRun::for_career_car`): the wreck stays
on the road for the tow service instead (no double charge). The demo (non-career) runs and the
`wp_economy` career simulation keep the flat charge.

## Exposes

- wasm: `career_tow_need()`, `career_tow_offer_json(uid)`, `career_tow(uid, kind, id)`,
  `career_place_json()`, `debug_total_loss(entity)` (e2e: 60 km/h frontal dents + write-off; not
  in replays).
- `CarView.spot` (`{kind: Garage | Home | Road, id, name}`, null in the pound).
- `wp_economy`: `Economy::{tow_metres, tow_price, tow_car}`, `TowJob`, `TowPricing`,
  `EventReason::Tow`, `CarSpot`, `CareerProfile::car_spots` (serde default: old saves load; no
  version bump), `SessionConfig::auto_tow` (serde default true).
- ui-kit: `CareerApi.towOffer?/tow?`, `garage/Tow.tsx` (`TowList`, `TowPickerView`,
  `TowPromptView`), `GameMenuHost.tow?/canTow?`.

## Contract change requests

| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| T1 | `wp_contracts::EconomyReason::Towing` gains `{ to: String, metres: u32 }` (or a new `Tow` variant with its own kind code) | `EventReason::Tow` maps to the field-less contract `Towing` (same kind code 35), so the net feed loses the destination and distance | 07 net, 08 economy, UI feed | proposed |

## Tests

- `cargo test -p wp_economy tow::` — price formula, early-career tuning, the ledger line, debt
  fallback (only the tow on account), pound refusal.
- `cargo test -p wp_wasm --lib tow` — destinations (nearest first and on account, prices,
  exclusions), flipped/stuck watch, damage needs; career: a wreck left on the road is towed to
  the nearest shop (charged, ledger line, car and player there, damage intact, saved, repaired
  there), every destination kind (another shop, a home garage, a locked garage refused), broke →
  debt fallback, wanted refusal unless totaled + pound refusal, a tow called in a run ends it,
  a chased player is refused.
- `npx vitest run src/garage` in `apps/ui-kit` — access rules for cars that cannot be driven
  (road / shop / home / pound / drivable), tow labels.
- `node e2e/tow.mjs` in `apps/testbed-web` (needs `npm run wasm`; `SHOTS=dir`): wreck → prompt →
  tow key → nearest shop → garage at the shop → Repair all → ledger line; pause-menu item; a
  wreck left on the road → "Tow to …". Screenshots: `docs/media/tow/`.

## Known gaps

- Distances are straight lines × 1.3, not routed over the road network (the garage screen has no
  world loaded).
- A repaired car at a shop still starts its next run from home (runs always spawn at home).
- There is no tow-truck animation: the run ends and the garage opens at the destination.
- The tow message in the garage uses the screen's message line (shown in the error colour, like
  the existing "Safe at … for the night").
