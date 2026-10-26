# Battering-ram mods integration notes

Owner ask (2026-10-26): "significantly increase the effectiveness of ramming/reinforcement
upgrades for mods - give them stronger boosts as well as penalties - we want to make the cars
feel like a battering ram".

- **Branch:** `claude/project-thread-x5eg2c` (built on PR #101, half-space ground)
- **Crates / folders touched:** `wp_economy` (mods), `wp_sim` (new `ram.rs`), `apps/simcli`
  (scenario mods), `wp_wasm` (career car), `data/mods/protection.json`

## Exposes
- `wp_economy::ModEffect::Ram { zone, mass_scale, dealt_scale, taken_scale }` in mod data:
  `{"Ram": {"zone": "Front", "mass_scale": 2.6, "dealt_scale": 2.2, "taken_scale": 0.45}}`.
  Zones `Front | Rear | Side`. Validated: mass and dealt 1..=4, taken 0.25..=1, and the mod
  must add mass.
- `wp_economy::{RamProfile, RamFace, RamZone}`; `AppliedMods::ram`. Effects on one face
  multiply, clamped (mass ≤ 4, dealt ≤ 4, taken ≥ 0.25).
- `wp_sim::Sim::set_ram_profile(id, profile)`. Set by `simcli` for scenario entities with
  `mods` and by the career's `car_session` for the player's car.

## How it works (`crates/wp_sim/src/ram.rs`)
Each tick, after the physics step and before any damage scaling, for every car-on-car contact
pair where one car has a non-stock profile:
- **Effective mass.** Each car's face is the impulse-weighted mix of the faces its contact
  normals point into (`RamZone::of_normal`, ±60° cones). With `k_a`, `k_b` the faces' mass
  scales and `g = μ(k_a·m_a, k_b·m_b) / μ(m_a, m_b)`, each car gets a correcting impulse so it
  changes velocity as a car of mass `k·m` would under `g·J`. The solver's relative velocity
  change is kept. Only while the pair closes at ≥ 1 m/s (`RAM_MIN_CLOSING_MPS`), so leaning
  on a car gives no free thrust.
- **Damage.** The contacts the damage model sees are scaled: mine by `g/k_me · taken_me ·
  dealt_other`, the other car's by `g/k_other · taken_other · dealt_me`.

Identical in Arcade and Simulation (owner rule). Stock cars are skipped, so goldens without
modded entities are unchanged.

## Numbers (two Tansy 13s, 15 m/s, `prot_bull_bar_ram` on the striker vs stock)
| Crash | Striker Δv | Struck Δv | Striker crush | Struck crush |
|---|---|---|---|---|
| Arcade rear-end | 8.30 → 4.73 m/s | 7.55 → 10.13 m/s | 2206 → 156 J | 10057 → 40268 J |
| Arcade T-bone | 9.03 → 5.18 | 5.81 → 9.79 | 2121 → 153 J | 23000 → 74538 J |
| Sim rear-end | 8.30 → 4.73 | 7.55 → 10.13 | 10199 → 2206 J | 10057 → 40268 J |
| Sim T-bone | 8.21 → 5.13 | 5.03 → 9.81 | 10148 → 2121 J | 23000 → 74538 J |

Tests: `wp_sim` `ram::tests`, `wp_economy/tests/ram.rs`.

## Changes outside my crates
`wp_sim/src/lib.rs` (slot field, setter, one call in `step`), `apps/simcli/src/{data,run}.rs`,
`wp_wasm/src/career.rs` (`car_session`).

## Known gaps
- Props, parked cars and debris get no ram effect (their contacts carry no entity id).
- AI racers and police cars spawn stock, so only the player's modded car (and scenario
  entities with `mods`) ram. A police push bar only matters for a player cop with mods.
- No "reinforced frame" mod exists yet; the roll cages carry a mild all-round ram face instead.
- The garage UI shows the trade-off text but no ram numbers.
