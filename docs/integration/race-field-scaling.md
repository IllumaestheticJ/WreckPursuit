# Race fields scaled to the player (2026-10-27)

Owner request: "make the opponents more aggressive across the board, have the cars scale with
both reputation and the car the user is bringing (if im bringing an exotic, so should they)
and have the reputation points scale accordingly as well". Stacked on the per-opponent risk
levels of PR #129 (`claude/project-thread-1xrl5d`).

## Aggression

- Picker races (all offered races are Loop 9): `wp_race::catalog::sharpened` now gives
  aggression +0.4 (at least 0.85, was +0.3 / 0.75) and risk +`SHARPENED_RISK` (0.1, at most
  0.9; risk was untouched before). Mara moves to Bold, Vic to Reckless.
- Roaming street racers: `wp_wasm::street::LEVELS` aggression 0.4/0.55/0.7/0.8 →
  0.6/0.72/0.84/0.92 and risk 0.35/0.5/0.65/0.8 → 0.5/0.6/0.72/0.85. Each name's
  `TEMPERAMENT` still spreads them, so most are Bold or Reckless now.

## Cars (`wp_race::field`)

- Target class index = player's car class index (mods included) + the rep tier's offset,
  never below the tier's floor (`TIER_TARGETS`: Unknown −10, Street 0, Known +10 / floor 400,
  Respected +25 / 480, Feared +40 / 560, Legend +60 / 650). Real class indices (2026-10-27):
  hatchbacks 170-330, sport coupes 430-570, exotics 620-999 (Morrigan, Halberd, Nyx capped at
  999); the pool is sparse above 680, so an exotic field mixes 620-860 cars.
- Regulars aim ±`SPREAD` (30) around it by starting form rating (strongest highest). Everyone
  whose own car is within `KEEP_WITHIN` (40) of their aim keeps it; the rest take the nearest
  unused `FIELD_CARS` car, strongest first (street cars only; no police, trucks, debug cars or
  the Ironside 440).
- `RaceDef::ai` is now a `Vec`, so a scaled def is an ordinary `RaceDef`
  (`field::with_cars`).
- wp_wasm: `race::scaled_field` (picker card, `race_start_on`, `Career::start_run` with the
  career car as modded and the account's rep) puts the cars on the scenario; the session
  reads them back with `race::field_of_scenario`, so results, book and replays agree.
  `race_scenario_in` itself is unchanged: the generated `apps/simcli/scenarios/race/pick_*.ron`
  files and goldens keep the catalog field.

## Rep

- `field::rep_scale(field classes, player class)`: (1 + (field mean − 450)/350) × (1 +
  (field mean − player)/250 when the field out-guns the player, at most 1.5), kept in
  0.75..2.5. Loop 9 examples: Vireo at 0 rep 0.75×, Kestrel at 0 rep ~0.9×, Fulmine at 0 rep
  ~1.7×, Kestrel at Legend rep ~2.4×. Only rep earned is scaled (a DNF's rep loss is not).
- `RaceRun::pay_race` awards the difference (`career:rep:race field strength`) and reports it
  as `RacePayout::field_rep`; the picker card shows `rep_scale` ("rep ×1.6").

## Known gaps

- The learned driving table (`data/ai/racer_training.json`) was trained per car per course;
  a regular in a swapped car uses rows matched by capability or drives untrained.
- Roaming street racers' cars are still fixed per level at spawn (their odds and rep already
  scale with the car gap, PR #81).
