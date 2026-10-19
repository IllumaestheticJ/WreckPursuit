# tools/specsheet — car roster tooling (workstream 09)

Plain Python 3 (standard library only, 3.9+). Nothing here is reachable from the game or
`wp_wasm`; it produces and checks data.

| File | What it does |
|---|---|
| `roster_inputs.py` | **The source of truth** for the roster: per-car engineering inputs (mass, geometry, torque curve, gearing, tire sizes, ride frequencies, roll targets, caliper sizes, turning circle…), prose (design note, how it breaks, source types) and lattice options. |
| `build_roster.py` | Derives `data/cars/<id>.json`, `data/cars/<id>.spec.md` and `data/lattices/<id>.json` from the inputs. Lattices = `gen-box-lattice` output + documented hand adjustments (`adjust_lattice`). Deterministic. |
| `estimate.py` | Point-mass performance estimator (0-100, top speed, 100-0, skidpad, rough class index). Not the game's vehicle model. |
| `specsheet.py` | Renders `data/cars/ROSTER.md` (roster table) and `data/cars/VALIDATION.md` from the car JSON. |
| `nameplate_check.py` | Fails if a real maker/model/badge/trademarked tech name appears in roster files. Denylist stored as hashes. |
| `smoke_sim.py` | Spawns every roster car in `wp_sim` via `simcli` in both damage modes, twice, and checks the state hashes match. |

## Typical loop

```sh
cargo build --release -p wp_damage --bin gen-box-lattice
cargo build --release -p simcli
# edit tools/specsheet/roster_inputs.py, then:
python3 tools/specsheet/build_roster.py          # cars, spec sheets, lattices
python3 tools/specsheet/specsheet.py             # ROSTER.md + VALIDATION.md
python3 tools/specsheet/nameplate_check.py       # naming policy
python3 tools/specsheet/smoke_sim.py             # loads + determinism through the sim
cargo test -p wp_contracts                       # schema + loader validation of all data
```

Never hand-edit the generated files; the next build overwrites them.

## Derivations (summary — each spec sheet shows the numbers)

- `mass_kg` = curb + 75 kg driver.
- Inertia: box formula `m·(a²+b²)/12` per axis × mass-concentration factor `k` (0.75 mid-engine
  … 0.97 long-overhang/body-on-frame).
- CdA = Cd × `area_k`·W·H (+ police lightbar 0.05 m²).
- Tire radius from the ISO size code; loaded = 0.97 × unloaded (0.96 truck/SUV).
- Springs `k = m_s·(2πf)²`; dampers `c = 2ζ√(k·m_s)`, rebound 1.5 × bump.
- Anti-roll bars: total roll stiffness for a target roll gradient (°/g) with a front share;
  springs contribute `k·t²/2`, bars `k_arb·t²` (the bar rate in `arb_n_per_m`).
- Brakes: disc `T = 2·μ_pad·p·A_piston(one side)·r_eff`, drum `T = BF·p·A_wc·r`; line pressure
  100 bar modern, 65 bar 70s; `bias_front = f + 0.9·h/L` clamped 0.55-0.78.
- Steering lock from kerb-to-kerb turning circle (mean road-wheel angle `atan(L/R_rear)`).
- Engine braking = 10-12 % of peak torque; clutch capacity 1.35-1.6 × peak torque.

## Tire compounds

Each car names an *intended* compound (`street`, `sport`, `police_pursuit`, `all_terrain`,
`vintage_bias`). `build_roster.py` uses it when `data/tires/<compound>.json` exists, else
`street`. The estimator then scales street peak friction by `estimate.COMPOUND_SCALE`
(sport 1.10, semi-slick 1.22, police pursuit 1.04, all-terrain 0.90, vintage bias-ply 0.85 —
rule-of-thumb ratios of peak lateral g between tire categories on the same car).

## Nameplate check

`nameplate_check.py` hashes every word, adjacent word pair and de-hyphenated run in the
roster files and compares with two hashed lists: STRICT (banned everywhere) and NAME
(ordinary words that are also nameplates — banned only in ids, display names, `analog_of`
and Markdown headings). To add a word: `python3 tools/specsheet/nameplate_check.py --hash
"word"` and paste the hash into the right set. Sound-alikes cannot be caught by hashing —
review new names by hand.

## Tests

The data itself is tested by `cargo test -p wp_contracts` (schema + loader validation) and
`cargo test -p simcli`. The scripts are checked by running them: each exits non-zero on
failure.

## Known gaps

- The estimator is a point-mass model: no wheelspin dynamics, no torque converter, no
  combined slip, no transient handling. Use it to catch data mistakes, not to tune feel.
- Lattices are post-processed 120-node box grids (124 with a push bar) until prompt 03's
  `gen-lattice` lands; see `docs/integration/cars.md` for what it should learn.

## Perf

Building all 13 cars (including 13 generator runs) takes ≈ 1 s; `specsheet.py` ≈ 1 s.
Roster lattices: 120-124 nodes and 704-714 beams per car (input for prompt 03's
0.25 ms/car lattice budget).
