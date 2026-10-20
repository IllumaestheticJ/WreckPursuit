# Engine decision report: Godot vs Unreal vs a Rust engine

_Written 2026-10-20 for the Checkpoint 3 engine decision (`docs/DESIGN_OVERVIEW.md` §2,
`docs/LEAD_HANDOFF.md` item 1). This is a recommendation, not a ruling. Nothing in the code
changes because of it._

## The short answer

**Recommendation: Godot 4, with our Rust core loaded in-process through godot-rust (GDExtension).**
Before committing, run a two-week spike (described at the end) that tests the one thing we are
least sure of: whether Godot can draw a dense night city with 24 deforming cars at 60 fps.

- **Pick Unreal 5 instead** if the spike fails on visuals and we are willing to pay for it in
  build complexity, slower iteration, and a team that has to know C++ and the Unreal editor.
- **Don't pick a Rust engine (Bevy, Fyrox) for the shipped game yet.** It is the most natural fit
  for our code and the least ready for an open-city game with artists working in it. It is still
  a good choice for internal tools and a better native testbed.

## What we are actually choosing

Most of the game is already built and will not move. Physics, tires, damage, traffic, police,
economy, races, weather and netcode all live in the Rust workspace. That code has no renderer, is
deterministic, and runs the same natively and in the browser. The headless server stays pure Rust
whatever we pick.

So the "engine" for the native version does a smaller job than usual:

1. **Drawing the world:** the city, lighting, night and weather, cars, debris, particles.
2. **Showing damage:** turning lattice node positions (`DamageSim::node_positions`) and the
   per-panel meshes (workstream R) into bent metal on screen, every frame, for up to 24 cars.
3. **Input and wheels:** keyboard, gamepad and steering wheels with force feedback. Browsers
   cannot do FFB, which is a main reason a native version exists at all.
4. **Sound, menus, HUD:** `wp_audio` already decides what plays; the engine plays it.
5. **The art pipeline:** where artists import, place and light things, and how we ship builds to
   PC (and maybe consoles).

We will **not** use the engine's physics, vehicle system or netcode. Unreal's Chaos Vehicles,
Godot's Jolt/VehicleBody3D and any built-in replication would all be bypassed, because our sim is
the authority and must stay deterministic. This matters: a lot of what makes each engine
attractive in general does not help us.

## The three options in plain terms

### Godot 4

A free, open-source engine (MIT licence, no royalties ever). Small download, opens in seconds,
simple editor. Its 3D has improved a lot over the 4.x releases but is still a step behind Unreal
for big, richly lit worlds.

**How our Rust code would connect:** godot-rust ("gdext") lets Rust code run as a native part of
Godot. Our crates would be compiled into a library that Godot loads; Godot nodes call `Sim::step`
and read back car states and lattice nodes directly. One language for the core, a thin layer of
Godot scenes and scripts on top. This is the closest any mainstream engine gets to "the core moves
as a library".

**Good for us:**
- Rust plugs in directly, with no hand-written C bridge.
- Scenes and resources are **text files** (`.tscn`, `.tres`) that diff and merge in git. This
  project is built by many parallel sessions working through text and pull requests; an engine
  whose project files can be read and reviewed suits that way of working.
- No licence cost or revenue share, and no risk of terms changing under us.
- Fast iteration: change a scene, press play.
- Deforming car meshes are straightforward: update vertex buffers each frame from the lattice,
  or drive a skeleton with one bone per lattice node.

**Bad for us:**
- **Big-city rendering is the main risk.** Open-world streaming, many lights at night, wet
  reflections and long draw distances are Godot's weakest area. We would probably have to build
  our own city chunk streaming and be disciplined about level-of-detail.
- Fewer ready-made high-end assets and plugins than Unreal.
- Console releases go through third-party porting companies, not Godot itself. Fine for PC
  first; a cost and a dependency later.
- godot-rust is community-maintained. It is used in shipped games, but it moves with Godot
  versions and we would be relying on its maintainers.

### Unreal Engine 5

The industry's high end. Best-in-class lighting (Lumen), streaming of huge worlds (World
Partition), mature tools for cinematics, materials and particles, and a big hiring pool. Free to
use, with a royalty on revenue above a threshold (currently 5% of gross over US$1M per product;
check the current EULA before deciding, as terms have changed before).

**How our Rust code would connect:** Unreal is C++. We would expose the Rust core through a plain
C interface (a new small crate built with `cbindgen` or similar), compile it as a static or
dynamic library, and wrap it in an Unreal C++ plugin. Every new thing the game needs from the sim
means touching three places: the Rust core, the C bridge, and the Unreal wrapper.

**Good for us:**
- Will look the best, soonest. A wet neon city at night with headlight pools and reflections is
  exactly what Unreal is good at.
- Open-world streaming is built in rather than built by us.
- Strong wheel/input support, profiling tools and console pipelines (with the platform holders'
  approval, same as any engine).
- Many artists and technical artists already know it.

**Bad for us:**
- **Heaviest integration.** Two languages, two build systems, a C bridge that must be kept in
  sync with the contract, and Unreal's own long compile times.
- **Its flagship features fight deformation.** Nanite (the high-detail geometry system) is aimed at
  static meshes; constantly deforming cars need a different, ordinary mesh path, so the cars
  themselves won't get the full "Unreal look" for free.
- Project files are mostly binary (`.uasset`, `.umap`). They cannot be meaningfully diffed or
  reviewed in a pull request and need file locking for teams. That is a poor match for how this
  repo is developed today.
- Heavy hardware for development, large downloads, slow editor startup.
- Revenue share if the game succeeds.

### A Rust engine (Bevy, or Fyrox)

Write the client in the same language as everything else. **Bevy** is the popular choice: a
modern, open-source (MIT/Apache) engine built around an entity-component-system. **Fyrox** is a
smaller Rust engine that does ship a full editor.

**How our Rust code would connect:** it just would. Same workspace, same build, same types. No
bridge at all. Bevy also runs in the browser via WASM, so in principle one client could replace
both the Three.js testbed and the native version.

**Good for us:**
- Zero integration cost and one language for everyone.
- Determinism rules, the contract and the tests carry straight over.
- Could unify browser and native into one codebase.
- No licence or royalty concerns.

**Bad for us:**
- **No stable visual editor in Bevy** as of writing. Building and lighting a city, placing props
  and tuning materials would happen in code or in tools we make ourselves. Fyrox has an editor,
  but a much smaller community and ecosystem.
- **Frequent breaking releases.** Bevy changes its API every few months; a multi-year game would
  spend real time keeping up.
- Rendering is capable but less proven for large, dense, night-lit worlds than either Godot or
  Unreal, and we would be early adopters for a game of this scope.
- No established console path.
- Hardest for future artists and designers to join; easiest only for programmers.

## Side-by-side

Ratings are relative to this project's needs, not general quality. ●●● best of the three.

| What matters | Godot 4 + godot-rust | Unreal 5 + C bridge | Bevy / Rust engine |
|---|---|---|---|
| Effort to connect our Rust core | ●●○ direct, via gdext | ●○○ C bridge + C++ plugin | ●●● none |
| Ongoing cost of keeping the bridge in sync | ●●○ | ●○○ | ●●● |
| Night city looks (lighting, wet roads, density) | ●●○ | ●●● | ●○○ |
| Large-world streaming | ●○○ build our own | ●●● built in | ●○○ build our own |
| Drawing 24 deforming cars | ●●○ | ●●○ (not Nanite) | ●●○ |
| Wheel / FFB input | ●●○ plugin or our SDL2 code | ●●● | ●●○ our SDL2 code (already exists) |
| Iteration speed day to day | ●●● | ●○○ | ●●○ |
| Fits text/PR-driven, many-session workflow | ●●● text scenes | ●○○ binary assets | ●●● all code |
| Tools for artists and designers | ●●○ | ●●● | ●○○ |
| Stability of the engine API | ●●○ | ●●● | ●○○ |
| Console path | ●○○ third-party porters | ●●● | ○○○ |
| Cost | ●●● free, MIT | ●●○ royalty above threshold | ●●● free |
| Could also replace the browser testbed | ●○○ (web export with a Rust extension is experimental) | ○○○ | ●●○ |

## Why Godot comes out ahead

1. **It keeps the architecture honest at the lowest cost.** The design already says the core
   moves as a library and only rendering and input are rewritten. Godot with godot-rust is the
   cheapest way to do exactly that while still getting a real editor.
2. **It suits how we build.** Many parallel sessions, everything reviewed as text in pull
   requests. Godot's text scenes keep that working; Unreal's binary assets would split the project
   into "code people can review" and "content nobody can".
3. **Our hardest problems are already solved outside the engine.** Unreal's biggest advantages
   (physics, vehicles, replication) are ones we deliberately don't use. What's left is mostly
   rendering, and that is a gap we can measure in a spike rather than guess at.
4. **It is the decision easiest to reverse.** If Godot hits a wall, the Rust core is untouched and
   the Godot layer is thin. Moving off Unreal after building a C bridge, C++ plugins and binary
   content would cost far more.

The honest weakness: **if the game's appeal depends on looking like a current-generation AAA
night city, Unreal wins on visuals and Godot may never fully catch up.** The spike exists to
find out whether "good enough" is good enough.

## What would change the recommendation

- **Visuals fail the spike** (we can't hit 60 fps with acceptable lighting in a dense block at
  night on mid-range hardware) → Unreal.
- **Consoles become a launch requirement** with a fixed date → lean Unreal.
- **We hire a team that already knows Unreal** → Unreal's costs shrink a lot.
- **We decide the browser version is the real product** and native is a bonus → Bevy becomes
  attractive, because one client could serve both.
- **Bevy ships a stable editor and slows its breaking changes** before we start content
  production → revisit Bevy.

## Things that stay the same whichever we pick

- All simulation stays in Rust and remains the authority. Engines only render and collect input.
- The headless server, simcli, scenarios, goldens and determinism checks are unaffected.
- The browser testbed (Three.js + `wp_wasm`) stays as the shareable test client.
- A new native-facing surface will be needed (a `wp_native` / gdext crate, or a C interface crate
  for Unreal), sitting next to `wp_wasm` and using the same `wp_sim` session. Its shape is a
  contract question for the integration lead and goes through `docs/CONTRACT_CHANGELOG.md`
  when we get there.
- Wheel force feedback: we already have SDL2 haptic code in `apps/testbed-native`; any of the
  three can reuse that approach if the engine's own support falls short.

## Proposed spike (two weeks, before Checkpoint 3)

Goal: answer "can Godot carry this game?" with numbers, not opinions.

1. **Bridge:** load `wp_sim` in Godot through godot-rust; drive one car with a wheel and FFB.
   Measure the per-frame cost of the bridge itself.
2. **Damage on screen:** 24 cars with per-panel meshes deforming from lattice nodes, a pile-up
   scenario replayed from simcli. Target: draw cost within the browser budget's native equivalent.
3. **Night city block:** one dense downtown block from the procedural city, streetlights,
   headlights, wet road reflections, 400 traffic agents. Target: 60 fps at 1080p on a mid-range
   GPU (define the reference machine before starting).
4. **Workflow check:** have two sessions edit the same Godot scene in parallel branches and
   merge. Note what hurts.

If (1), (2) and (4) pass and (3) is close, commit to Godot. If (3) is far off, repeat step 3 only
in Unreal before deciding; the bridge and damage steps there would cost more and are not the
uncertain part.
