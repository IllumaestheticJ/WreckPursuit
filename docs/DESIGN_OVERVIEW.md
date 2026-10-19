# WreckPursuit — Design Analysis, Tech Plan & Parallel Work Split

Working title: **WreckPursuit** (gritty street-racing + physics-driven damage + asymmetric police multiplayer).

This folder contains:

| File | Purpose | Run on |
|---|---|---|
| `00_README_OVERVIEW.md` | This document — analysis, decisions, references, missing features, art checkpoints | Read first |
| `CLAUDE.md` | Drop into the repo root. Shared conventions every Claude Code session obeys | Every machine |
| `01_architecture_contracts.md` | **Run FIRST, alone.** Repo skeleton + data schemas + crate interfaces everything else builds against | Machine A (≈ half a day) |
| `02_vehicle_dynamics.md` | Wreckfest-style sim-cade vehicle physics core | Machine A |
| `03_damage_model.md` | Semi-soft-body deformation + mechanical damage + arcade/sim tables | Machine B |
| `04_browser_testbed.md` | Three.js/WASM testbed, debug viz, telemetry, replay | Machine C |
| `05_city_traffic.md` | Road network, signals, speed limits, traffic AI, density by time/distance | Machine D |
| `06_police_pursuit_ai.md` | Heat/bounty, dispatch, day/night tables, pursuit tactics, roadblocks/spikes/helo | Machine E |
| `07_multiplayer_netcode.md` | Authoritative server, prediction, police roles (captain/chase/AI), interest mgmt | Machine F |
| `08_economy_progression_mods.md` | Money/rep, repair bills, impound, UG2-style mod tree, durability-vs-speed | Machine G |
| `09_car_roster_data.md` | 10 unlicensed analog cars with real-world-derived spec sheets + damage lattices | Machine H (no engine work; data only) |
| `10_test_harness_ci.md` | Deterministic headless sim, physics regression tests, benchmarks, CI | Machine I |
| `11_art_direction_pipeline.md` | Art style options, asset pipeline, greybox → final checkpoints (later phase) | Later |
| `12_integration_lead.md` | The prompt for whoever merges everything each day | Lead machine |

**Order of operations:** `01` must finish and be pushed before `02`–`10` start (they all import the `wp_contracts` crate). After that, everything runs in parallel. `12` runs continuously.

---

## 1. What you are actually building (restated as engineering)

1. **Rigid-body chassis + deformable shell.** This is exactly what Wreckfest does and it is *not* BeamNG. The car that *drives* is one rigid body with 4 raycast/sweep wheels and a proper tire + drivetrain model. The car that *crumples* is a lattice of mass-nodes and plastic beams skinned to the visual mesh. Contact impulses from the rigid body are distributed into the lattice, the lattice deforms permanently, and the *deformation feeds back* into the driving model (bent toe/camber, steering pull, lost wheels, crushed radiator, power loss). Keeping these two systems separate is what makes it tunable, networkable and runnable in a browser.
2. **One damage code path, two parameter sets.** Arcade and Simulation are the same solver with different tables: energy→damage multipliers, yield thresholds, component failure curves, repair-cost multipliers. Wreckfest's "Normal / Realistic" works this way. Never fork the code.
3. **Deterministic fixed-step simulation** (240 Hz physics, 60 Hz netcode snapshots, render interpolated). Determinism is what lets you (a) replay crashes for tuning, (b) run regression tests headless, (c) replicate deformation in multiplayer cheaply by sending impulse events instead of mesh data.
4. **Engine-agnostic core.** The physics, damage, traffic, police and economy logic live in a Rust workspace with no renderer dependency. It compiles to native (headless server, tests) and WASM (browser testbed). When you later move to Godot/Unreal/Unity, the core moves as a library; only the renderer and input layer are rewritten.

## 2. Technology decisions

| Layer | Choice | Why | Alternative if the team prefers |
|---|---|---|---|
| Language / core | **Rust** workspace | Compiles to native + WASM from one codebase, deterministic math is practical, good for authoritative server, strong test tooling | C++ core with Emscripten (same architecture, more build pain) |
| Rigid-body / collision | **Rapier3D** (`enhanced-determinism` feature, f32) | Rust-native, WASM-ready, deterministic across platforms, exposes contact manifolds we need to drive the lattice | Jolt via FFI (faster, more mature vehicle demos, but FFI + WASM is heavier) |
| Vehicle model | **Custom** (not Rapier's built-in car) | Wreckfest feel needs our own tire model, drivetrain, assists, FFB | — |
| Deformation | **Custom node/beam lattice** per car (~150–400 nodes) with plastic beams | Semi-soft body like Wreckfest; cheap enough for 24+ cars + traffic | — |
| Phase-1 renderer | **Three.js + TypeScript + Vite** loading the WASM core | Zero-install testable in a browser, trivial to share builds, good debug drawing | Bevy (Rust-native windowed testbed) — fine, but slower to iterate on UI |
| Server | **Rust + tokio**, WebSocket for prototype → WebTransport/UDP later | Same crates as client; headless | — |
| Data formats | JSON (car defs, lattices, mod tree), TOML (mode tables), binary road-network | Human-diffable; schema-validated in `wp_contracts` | — |
| Later engine | Godot 4 (Jolt) or Unreal 5 | Decide at the "Checkpoint 3" art milestone; core ports via C ABI / GDExtension | — |

Wheel support (FFB) is **native-only** — browsers cannot drive force feedback. Keyboard/gamepad in browser; wheel testing happens through the native testbed stub (see `04`).

## 3. Data & references the team must feed in

**Vehicle dynamics (per analog car — see `09`):** curb mass, weight distribution, wheelbase, track widths, CG height (estimate from class), engine torque curve (10–15 points), redline, gear ratios + final drive, drivetrain layout (FWD/RWD/AWD + diff types), tire size and compound, Cd × frontal area, spring rates / ARB / damping (class-typical), brake bias, steering ratio and lock. Public spec sheets and owner-community dyno data for the real counterparts are enough; we rename everything.

**Tires:** Pacejka '94/MF5.2 coefficient sets are widely published in academic and sim-community sources for street, sport and race compounds. We use a simplified Magic Formula (B, C, D, E per axis + load sensitivity + combined-slip friction ellipse). Feed 3 base compounds; the tuner derives the rest.

**Materials for damage:** yield strength, plastic modulus, failure strain, density for: mild steel body panels, HSS/UHSS structural members (A/B pillars, rails, crossmembers), aluminum (hoods, some fenders), polypropylene/TPO bumper covers, polycarbonate/laminated glass, cast components (knuckles, hubs). Publicly available engineering tables suffice.

**Crash calibration:** NHTSA / IIHS public crash-test reports give crush depth vs impact speed for frontal/side/offset tests. Use them to calibrate the Simulation table ("a 35 mph frontal into a barrier = total loss"). Arcade is a tuned multiple of Simulation, not a separate dataset.

**City:** an OpenStreetMap extract of a real mid-size US downtown + surrounding neighborhoods for road topology, lane counts, speed limits, signalized intersections (ODbL requires attribution — fine for a prototype; the shipped city will be fictional). Signal timings: standard NEMA phase plans (fixed-time for prototype).

**Police behaviour:** public pursuit-policy documents from large US departments (pursuit initiation criteria, PIT speed ceilings, termination rules, air-support roles) are the reference for "slow, calculated" AI and for the captain's toolset.

**Feel references (subjective, for tuning sessions):** Wreckfest 1 & 2 (handling + damage baseline), BeamNG (what *not* to do: full soft body is too heavy and too floaty for this), NFS Underground 2 (mod/garage structure), Driver: San Francisco / Midnight Club LA (city traffic density feel), Mafia / GTA IV (police that feel dangerous).

## 4. Features you are probably missing (proposed)

**Must-have for the core loop**
- **Replay + photo mode** — also your #1 physics-debugging tool. Build it in `04` from day one.
- **Garage / tuning UI with a test lot** — Wreckfest's "reset to garage" loop; mods need somewhere to be felt.
- **Insurance / repair-vs-scrap decision** — the economy needs a floor so new players aren't bricked after one crash. Cheap "beater" tier cars are the floor.
- **Impound & retrieval** — getting caught should not delete the car by default (Arcade); Simulation can.
- **Heat decay & hideouts** — safehouses that end a pursuit if you break line-of-sight long enough; without this, chases have no win condition for the racer.
- **Race types**: sprint, circuit (closed day events), outrun (1v1 open-world), checkpoint cannonball, delivery/smuggle (cargo weight affects handling), and *illegal daytime* events as the rare high-heat variant you described.
- **Spectator / helicopter camera** for the police captain — the captain's whole gameplay is a map + camera + dispatch radial.
- **Difficulty/assist matrix** separate from Arcade/Sim damage: ABS, TCS, stability, steering help each toggleable; keyboard gets filtered input by default.

**Strongly recommended**
- **Weather** (rain, wet roads, fog) — grip multiplier + visibility + police response changes. Cheap to add once tire model exists.
- **Cop career progression** — rank unlocks spikes → roadblocks → helicopter → captain eligibility. Gives the police side its own loop.
- **Crews / factions** with shared rep and territory heat.
- **Radio scanner audio** generated from the dispatch system's actual events (free content, huge atmosphere).
- **Dynamic crash scenes** — a wreck persists for N minutes with traffic slowdown and a patrol car; the city remembers.
- **Telemetry overlay + input display** (shipped, like sims) — serious players will want it; you need it anyway.
- **Anti-cheat baseline** — server-authoritative sim already covers most; add input-plausibility checks.
- **Livery editor** (UG2 vinyls) — later, but design the mesh UVs for it in `11`.
- **Accessibility**: remappable inputs, colorblind-safe HUD, rumble.

## 5. Art style options & evolution checkpoints

| Option | Look | Production cost | Fit |
|---|---|---|---|
| **A. Neon-noir realism** | Wet asphalt, sodium + LED mixed lighting, grounded PBR cars, heavy particulate/fog | High | Best match for "gritty street racing"; needs a real engine (Checkpoint 4) |
| **B. Stylized low-poly, dark palette** | Flat-shaded, strong silhouettes, chunky damage reads clearly, post-process grain | Low | Fastest to ship; hides placeholder meshes; deformation looks *good* on low-poly |
| **C. 2000s PS2/early-HD throwback** | Baked lighting look, bloom-heavy, VHS/DV grain, period UI | Medium | Nostalgia hook for the UG2 audience |
| **D. Graphic-novel / halftone** | Cel edges, high contrast, limited palette per time-of-day | Medium | Distinctive, strong marketing identity, forgiving of asset quality |

Recommendation: ship the prototype in **B**, decide between **A** and **D** at Checkpoint 3.

**Checkpoints**
1. **Debug** — boxes, wireframe lattices, force arrows, text telemetry. Physics only. (Prompts 01–04, 10)
2. **Greybox city** — untextured blocks, flat road meshes from the road graph, traffic cubes, police cubes with light bars. Full loop playable. (05–08)
3. **Low-poly playable** — style B applied, 10 cars, HUD, garage, multiplayer alpha. First external test. Choose final style + engine here.
4. **Target style in target engine** — core ported, final style, audio, livery editor.

## 6. How to run the prompts in parallel

1. Create an empty GitHub repo. Put `CLAUDE.md` at the root. Commit.
2. On **one** machine, run `01_architecture_contracts.md`. It produces the Cargo workspace, `wp_contracts`, the schemas, the stub crates, and `docs/INTEGRATION.md`. Push to `main`.
3. Every other machine: clone, `git checkout -b ws/<name>`, paste its prompt. Each prompt is self-contained and tells the agent which crate it owns, which crates it may *read* but not edit, and what it must deliver (code, tests, a crate `README.md`, and a `docs/integration/<name>.md`).
4. The lead machine runs `12_integration_lead.md` once or twice a day: merges branches, runs the full test suite and the headless sim, fixes contract drift, and re-publishes `wp_contracts` if a schema had to change (schema changes are announced in `docs/CONTRACT_CHANGELOG.md`; downstream agents are told to read it at session start).

Scope honesty: this is a multi-year commercial scope. The prompts are cut so that **a driveable car with convincing damage in a browser** exists within the first sprint, and everything after that is additive. Physics and damage tuning will be iterative and human-in-the-loop; the testbed and replay system exist so those sessions are productive.
