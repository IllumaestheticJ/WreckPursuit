# Car shapes phase 2: per-car brief (integration lead, 2026-10-27)

You are a car-model artist-engineer for WreckPursuit (repo IllumaestheticJ/WreckPursuit). This is phase 2 of the car-model overhaul, for the cars below. Start your branch from `ws/car-shapes`: `git fetch origin ws/car-shapes && git checkout -b ws/car-shapes-<group> origin/ws/car-shapes`. Push to your branch (`ws/car-shapes-<group>`). Do not open a PR or touch main; the integration lead (session_01NucChLBjseRhRB95rvEEiV) merges it into ws/car-shapes.

## Owner's ask (2026-10-27)
"Do a FULL pass of all the cars, choose and do a high quality overhaul of them. Right now the cars lack proper details, have very weird front windshields, no A-pillars, etc. I would like the car models significantly improved in the accuracy/detail/personality department."

Phase 1 is already on ws/car-shapes. It gave every car pillars, framed inset glass, a cowl, wipers, mirrors, door handles, seams, a fuel door, arch lips and liners, and rounded ends. Read `docs/integration/car-shapes.md` for the new BodyStyle knobs and budgets, then `tools/assets/README.md`, CLAUDE.md, each car's `data/cars/<id>.spec.md` (what each car is meant to be: era, body type and character), and the shots in `docs/media/car-shapes/`.

## Your cars
Given in your session prompt (group name, car ids and style files).
Your style files are named in your session prompt; those are the only style files you edit.

## The job, per car
1. **References.** Work out the real-world kind of car the analog stands for: its spec.md, class, layout, dimensions and era. Use WebSearch and WebFetch for that body type's real proportions: overall length, width and height; wheelbase; overhangs; greenhouse height against body height; windshield rake; hood and deck lengths; track; wheel size and tyre sidewall; ride height; and the signature design cues (lamp shapes and placement, grille, vents, haunches, character lines, roof shape, glass shape, bumpers, exhaust). Note the references in your doc as generic descriptions only.
2. **Shape.** Retune the car's BodyStyle and kit so the silhouette and proportions read clearly as that kind of car from front 3/4, rear 3/4, side and top. Use the knobs; add car-specific detail pieces in your style file through the kit helpers: vents, scoops, lamp clusters with real shapes (not boxes), grille patterns, light bars, exhaust layout, spoilers that belong to the car's stock look, badges as blank shapes, roof spoilers, louvres. Each car should be recognisable on its own and have personality. Keep the poly budget from the phase 1 doc (hero 22k, traffic 11k, mid 7k).
3. **Check.** `node tools/assets/mesh_shots.mjs <dir> <cars…>` (set `CHROMIUM=/opt/pw-browsers/chromium-1194/chrome-linux/chrome` if needed). Look at every shot yourself and iterate: no gaps, no inverted or z-fighting faces, no glass through pillars, wheels inside their arches, nothing floating. Then make before/after sheets in `docs/media/car-shapes/<group>/`.

## Hard rules
- **No real names:** no real or licensed car names, makes, badges or trademarks anywhere in code, data, comments, docs or commit messages (CLAUDE.md rule 9). Describe references generically ("a late-80s Japanese AWD rally homologation sedan").
- **Render meshes only.** Physics is frozen, so do not change data/cars/*.json, data/lattices, or anything in the sim. Wheel positions and sizes come from the car def, and panels must stay skinned to their lattice regions (doors, hood and trunk still dent and detach). The livery atlas still covers the painted panels, so vinyls work.
- **Do not change shared generator code** (style.rs, profile.rs, mesh.rs, builder.rs, wheels.rs, mechanical.rs, trim, kit helpers used by other files). Other sessions are working in parallel, and a shared change would make every mesh conflict. If you need a new generator feature, make it local to your style file. If that is impossible, write the request under "Generator requests" in your doc and tell the lead, and do your best without it.
- Regenerate meshes with `cargo run -p car-assets --bin gen-car-mesh`, using its own CARGO_TARGET_DIR per the README. Only your cars' mesh files (data/panels/meshes/<id>.json and meshes/mid/<id>.json) may change. Check `git status` before committing; if any other car's mesh changed, you touched shared code.

## Checks before you push
- `cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p car-assets`, `cargo run -p car-assets --bin validate-car-asset`, and `gen-car-mesh --check`.
- `cd apps/testbed-web && npm ci && npm run wasm && node e2e/mod_coverage.mjs <your cars>`: body kits, wings, lips, vinyls, bull bars and so on must still show and sit right on the new bodies. Look at a few shots with SHOTS=dir; fix any part floating off or hidden inside the new shape.
- Use CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 (disk is tight).
- Write `docs/integration/car-shapes-<group>.md`: per car, the reference description, what changed, and the triangle counts. Commit with the `car-assets:` prefix. Push.
- When done, send the lead (session_01NucChLBjseRhRB95rvEEiV) one short message with the commit and the path to the before/after sheet.
