# Versions (owner-facing)

Scheme (owner request 2026-10-20): `0.ROUND.PREVIEW`. Previews of round N are `0.N.1`, `0.N.2`, …;
the main testbed link and the public demo get `0.N.0` when round N passes the full gate. The
version shows as a badge in the page corner and in every refresh report.

| Version | Link | What's new |
|---|---|---|
| 0.11.0 | main testbed | Greybox demo loop: capture/escape → fine, impound, repair; Port Vesper v2; signals; minimap pending |
| 0.12.1 | preview | Recorded sounds, weather + rainy night, garage/career, minimap, differentials, hood crumple v1, cop sideswipe chase, AI go-around |
| 0.12.2 | preview | Sidewalk fall-through fix, races, understeer retune, LOD pop-in fix |
| 0.12.3 | preview | Tougher Arcade cars, resale fix + insurance note, missing car sounds, tunnels, quit-while-wanted = capture, gamepad + rebinding (F9), junkyard beater, speed pass, regenerated city |
| 0.12.4 | preview | Junction holes + stop-line bumps fixed |
| 0.12.5 | preview | Parked cars drawn (the real merge) |
| 0.12.6 | preview | Low-poly dark art style, traffic yields to sirens, hood crumple v2, horn hold, reset in place, "Impound" |
| 0.12.7 | preview | (police merge incomplete — superseded by 0.12.8) |
| 0.12.8 | preview | Police: spawn near calls, pass-by chases + turn-around, fewer stuck cops; siren pull-over fix; race fixes |
| 0.12.9 | preview | Parked-car frame-rate fix (culled chunks, −70 % triangles) |
| 0.12.10 | preview | Pause menu + settings (Esc), minimap overlap fixed, crash effects (sparks, smoke, glass, skids, debris), instant crash replay, sidewalks level with buildings + flush lots, debug/release determinism fix; touch controls merged but broken on phones (being fixed) |
| 0.12.11 | preview | Touch controls fixed with the new menu (phones: auto gears + TCS by default, pause by touch) |
| 0.12.12 | preview | Wreckfest-style countersteer (slider; gyro removed), handbrake turns, one-click Rookie/Street/Sim presets, modelled engine sound, modelled impacts/horns, police round 13 (backup, tactics, push-through, building feelers, tunnel search), parked cars unlit, spike strips drawn |
| 0.12.13 | preview | Police round 13 (pass-by chases, longer sight, search reinforcements, head-on blocks, not stuck in jams, safe spawns + self-reset), police radio chatter (spoken, radio strip), siren v2, night traffic curve + fewer parked cars + jam fixes, Arcade damage −20–30 %, stock shift times + shift mods, faster countersteer, browser perf passes, beater fixes |
| 0.12.14 | preview | AI racer pace (real car limits, junction gaps, traffic passing), siren traffic test re-scoped, round-13 test fixes |
| 0.12.15 | preview | Physics cost back down (sim-perf), wider roads + residential districts, police turn-round loop fix, police roadblocks + spike strips (physical), reference-style controls settings + parity suite, vehicle drift lab (assists off by default) |
| 0.12.16 | preview | Brake dive cut (anti-dive geometry + damping on 5 cars), racers on the minimap, AI creeps past parked police, junction permit fix (civilian crash), siren pull-over to the kerb |
| 0.12.17 | preview | Race grids inside the paved lanes, police give way to racers and look before turning (Hollins hairpin), goldens re-blessed; all race tests green |
| 0.12.18 | preview | TCS + ABS Off / Half / Full (presets Rookie F/F, Street H/F, Sim Off/Half), TCS Off = raw throttle, handbrake pivot aid (owner-approved, keyboard/gamepad), auto clutch no longer drags below idle when rolling back |
| 0.12.19 | preview | RWD cars no longer spin off the line on cambered roads (spin side-force floor), auto clutch fully engaged once moving (no constant-rpm slip in tall gears) |
| 0.12.20 | preview | Ground-probe tilt fix: tyre loads no longer chatter in corners (steadier grip everywhere) |
| 0.12.21 | preview | AI racers: fleeing cars pull over to the kerb (no in-lane deadlocks), low-speed junction line fix, hill starts, AI never triggers the handbrake pivot aid; pace ranges re-measured |
| 0.12.22 | preview | Police: parked cops U-turn forward to give chase, no turning across oncoming racers, deadlock breaking (Loop 9 / Hollins), reverse-hold braking fix; sirens play real recordings |
| 0.12.23 | preview | Hollins race: busted racers pull off instead of freezing in the hairpin, no stopping in tight bends; asphalt tyre roar instead of hiss |
| 0.12.24 | preview | AI racers pass cars that are out of the race and never stop to yield in a hairpin (Hollins seed 6 finishes); solid terrain matches drawn terrain (no invisible hillside); engine sound rework (recorded layers at their real firing frequency) |
| 0.12.25 | preview | C is the clutch (camera cycle on F3); rev on the grid during the countdown; AI: fleeing racers get round cars waiting at reds, grid cars line up properly |
| 0.12.25 (race build) | preview ([link](https://claude.ai/artifact/R3vr7NVghCGvdvN4TDzZCA)) | Built in parallel from the same round12 commit as the 0.12.25 above (merged in the 2026-10-21 branch triage): race gates much wider (min 26 m) with a taller arch; going too far off course puts the car back on the route instead of a DQ (playtest row 127, filed as row 120 on its branch) |
| 0.12.26 | preview | Parked cars dent, shed glass and parts when hit; traffic uses turn signals (turns, lane changes, pulling over) and hazards; player indicators on - / = |
| 0.12.27 | preview | Eight new cars: Stoat 16V and Marten 2.0 (90s hot hatches), Bulwark 400 and Brigand 400 (70s muscle sedans), Arashi T4 and Talus 2.0T (AWD rally saloons), Satchel 2.0 and Tumbril 2.0 (vans); drawn terrain follows the ground behind lots |
| 0.12.28 | preview | New careers start with $150 000 for testing (?cash=market for the real $4 500); game data embedded compressed (wasm 10.7 MB) |
| 0.12.29 | preview | Parked cars sit on the ground (hillside driveways around Hollins were up to 0.76 m in the terrain), knocked cars no longer sink; new cars' meshes regenerated |
| 0.12.30 | preview | ONE VERSION: the owner's handling (reference step-14 balance, springs/bars ×1.5, step-22 tyres) on every car; over-rotation catch, transition + settle steering, held-key fix on the player car; auto gearbox holds gear while sliding; hidden tune panel (?tune=1) with reset-all, copy-tune and rear ×0.50 / ×0.55 presets |
| 0.12.31 | preview | Photo mode (F7: free camera, filters, save PNG), realistic tyre squeal, mod shop with parts drawn on the car, police helicopter (bridges block its view, radio), race gates wider + off-course reset, audio pauses with the game; tune panel moved to F10 |
| 0.12.32 | preview | Main's new maps (divided highways, ramps, lane markings, new grid), P8 pro police + stat bars; police: noticed→pursuit, U-turn route, Hollins hairpin capture, stand-offs clear in 3 s; AI racers use the whole road, weave, gap planner, cut empty junctions; front brake bias +0.05 (brake stability); race payout matches the advertised purse; parked cars clear of fences / Bluff Road ground fix |
| 0.12.33 | preview | Render budget back under limits: road markings tiled + culled (one draw per tile), 640 m city tiles on phones, buried grid wall curtains dropped (chase 425k → 363k triangles, phone 323 → 280 draw calls) |
| 0.12.34 | preview | Police: arresting unit busy 30 s (no chain arrests), pinned capture needs a chasing unit, head-on waiting unit creeps aside, backup waits ahead facing the racer, faster chase junctions, stuck units route round traffic; AI racers pass civilians stopped at Lantern Row lights, junction gap check, never stand at a red on the opposing side |
| 0.12.35 | preview | AI racers never give up in a race (owner rule): no yielding or pulling over at any wanted level; a race ends only on a real capture, wreck or DNF |
| 0.12.36 | preview | Police audio: police engines procedural (no 2.3x sped-up recordings, no layer flipping), siren junction cue calmer (3 s minimum pattern hold); spikes: recorded blow-out per tyre, flat-tyre flap loop, rim grind, scanner line |
| 0.12.37 | preview | Police keep-up: pro driver no longer crawls at 4 m/s through every right-angle corner apex (pace vs racer 0.68 → 0.78) |
| 0.12.38 | preview | Detailed car models for every car (styles, liveries, decals, light rigs), 3 new cars (Fulmine V10, Sentinel Warden, Sentinel Vigil), mixed-model traffic and police, car LOD (hero / mid / fleet); races: going off route is never a DQ or reset, checkpoints in order |
| 0.12.39 | preview | Big engines (V8 / inline-6, 13 cars incl. police) on the generated engine instead of 4-cylinder recordings sped up 1.8–3.4x (owner: generated sound now); 4-cylinders keep recordings |
| 0.12.40 | preview | Spikes: a flat corner visibly drops (60–85 mm at full deflation; suspension uses the deflated radius), fully flat wheels run on the rim (low grip, pull to the flat side) |
| 0.12.41 | preview ([link](https://claude.ai/artifact/R3vr7NVghCGvdvN4TDzZCA)) | Traffic no longer panics and slams the brakes when the player (or a racer) passes in the next lane or comes the other way; AI racers fleeing the police use every lane (they sat on the green line on Loop 9) |
| 0.12.42 | preview | One combined build on the main preview link: 0.12.41's calm traffic + fleeing racers on every lane, plus rim grind on the real rim-contact flag and the lane-split rule (skilled racers thread side-by-side traffic when the gap fits) |
| 0.12.43 | preview | Turn signals built into every car's head / tail clusters (per style; flush, break with the car end; the cars' own lenses blink instead of corner boxes); D's traffic jam fixes (row 85) on main's maps |
| 0.12.44 | preview | Keyboard countersteer default 0 %, new-side catch 0.9 → 0.8 (fixes the rampart double transition), flick sliders lab-only |
| 0.12.45 | preview | Player keyboard steering = the key (no transition / catch / limit / held-key cut / smart lock; plain speed curve only); hands-off countersteer default 30 %, capped at 30 % of lock, no-key only (matches the reference clips); reference cars parity_rwd_e / parity_rwd_f |
| 0.12.46 | preview | (next) |
| public demo | demo link | Snapshot of 0.12.3 with a start menu (refreshed only from a green main) |
