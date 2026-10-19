# data/events — race rules, stakes, scheduler, crews, progression

Owned by workstream 15 (`wp_race`). All files are TOML, SI units, validated by `wp_race` on
load (`RaceData::parse`); the crate embeds them at compile time and its tests fail on invalid
data. Design write-up: `crates/wp_race/README.md`.

| File | Loaded as | What |
|---|---|---|
| `rules.toml` | `RulesTable` | per-kind grid, racer limits, time limit, finish window; shared tolerances (clean-race impulse, gate tolerance, corridor margin/grace, teleport speed, countdown); drag / outrun / delivery settings |
| `stakes.toml` | `StakeTable` | purse, entry fee, heat and police presence keyed by race kind, `DayPhase`, class bracket and legality; `risk_curve` makes purse monotone in risk |
| `schedule.toml` | `ScheduleRules` | sanctioned day events, night meets, rare illegal daytime races, bracket weights, venue gap, meet heat |
| `progression.toml` | `ProgressionTable` | class brackets (must match `data/economy/market.toml`), licence tiers by clean races |
| `crews.toml` | `CrewConfig`, `TerritoryConfig` | crew size, bank cut, rep share; territory points, control threshold, weekly decay, heat-decay bonus |

Keys: race kinds use `RaceKind::key()` (`sprint`, `circuit`, `outrun`, `checkpoint`,
`cannonball`, `delivery`, `drag`, `time_attack`); legality uses `sanctioned`, `street`,
`illegal_daytime`; day phases use `Dawn`, `Day`, `Dusk`, `Night`; brackets `D`…`X`.

These files have no JSON Schema in `schemas/` yet (they are not contract types; see request R1
in `docs/integration/race.md` for the types proposed for the contract).
