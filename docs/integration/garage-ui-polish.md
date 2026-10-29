# Garage UI polish integration notes

- **Workstream / prompt:** lead — garage / workshop polish (owner requests 2026-10-29)
- **Branch:** `lead/garage-ui-polish`
- **Crates / folders owned:** `apps/ui-kit/src/garage/{Workshop.tsx,upgrades.ts}`,
  `apps/testbed-web/src/audio/garage_sfx.ts`, `apps/testbed-web/src/workshop/sfx.ts`
- **Contract version built against:** unchanged (no Rust changes)

## What changed

1. **What you have is greyed out.** `upgrades.ts::ownedState(screen, entry, mod, prefer)` says
   whether the player already has what an upgrade row sells. Each `UpgradeRow` (part card,
   catalog) uses it. Rows the player has get `data-owned` and an "on the car" / "owned" tag
   with a tooltip:
   - `fitted`: the grade is on the car where the row would fit it (the card's slot for a single
     part, every slot of the kind for a set). The row is dimmed (`wp-dim`), and **Buy** and
     **Fit for me** are disabled.
   - `installed`: a bolt-on is on the car. The row is dimmed, and **Remove** stays enabled.
   - `inventory`: a part of that grade is in the inventory (single parts only). **Buy** is
     disabled because a second copy is pointless. The mechanic button becomes **Fit mine for
     me** (labour only, fitting that inventory part). Fitting it by hand from the inventory
     strip works as before. The row is not dimmed, because the part is not on the car yet.
   - Stock parts (grade null) are never "owned": a new stock part replaces a worn one.
   `Shop.target` now prefers a slot that does not already carry the grade, so for a single part
   with two slots, the empty-of-it one is the one that gets fitted.
2. **The catalog stays open.** "Fit for me", "Fit both/all for me", Buy and Install no longer
   close the catalog (the `after`/`done` hook is gone). The catalog shows the last action's
   line at its top (`ws-cat-notice`, e.g. "Mechanic fitted Aluminium Radiator: …"; errors in
   `ws-cat-error`). Opening the catalog clears the old line.
3. **Garage sounds** (`audio/garage_sfx.ts`): procedural Web Audio only (no files, no new
   licensed audio). They cover buy and sell (register clack and bell), the mechanic (impact
   wrench), bolt-on and part on/off (ratchet and clunk), repair (hammer taps and wrench) and a
   respray (spray hiss). The workshop also gets a low room-tone loop while it is open.
   - Routing: the mixer's **Ui** bus, scaled by the menu's **Effects** volume (the loop also by
     **Ambience**). Master volume and mute apply through the master gain.
   - Garage screens pause the game, which used to suspend the AudioContext. Now
     `AudioSystem.setPaused(p, garageScreen)` keeps the context running behind the career
     garage and the workshop. It shuts the other buses with a new per-bus `gate` node in
     `AudioMixer` (after the trim, so `busOut` and offline taps are unchanged), and the
     director waits. Every other pause still suspends the context.
   - Hooks: `workshop/sfx.ts::withSounds` wraps the `WorkshopApi` handed to the overlay. Each
     wasm call that returned (did not throw) plays its sound. The paint shop's `respray` is
     wrapped from outside, without touching paint logic. The career garage's repair (not
     scrap), tyres and install/uninstall, plus the in-world garage visit's repair, play
     through `withSound` in `main.ts`.
   - Like the rest of the audio, nothing plays before the first click or key press. e2e reads
     `__wpAudio.garageStats()`.

## Tests

- `apps/ui-kit/src/garage/upgrades.test.ts`: `ownedState` (fitted, sets, the slot the row is
  for, slots inside a removed assembly, inventory, stock, bolt-ons).
- `apps/testbed-web/e2e/garage_polish.mjs` (new) covers:
  - the catalog stays open after Fit for me, Buy and Install;
  - fitted rows are greyed and their buttons disabled;
  - Fit mine for me charges labour only;
  - a bolt-on's Remove still works;
  - garage sounds are requested and played, and the room tone starts and stops.
  Screenshots: `docs/media/garage-polish/{before,after}/`.
- `e2e/mechanic.mjs`, `e2e/workshop_buy.mjs`: the catalog stays open after Fit for me and the
  fitted row is greyed.

## Known gaps

- `e2e/workshop_buy.mjs` step 3 fails on `origin/main` too, before these changes. The catalog
  line for `brakes_kit_2` now has `reason: "needs Performance Pads fitted first"`, so the plan
  is blocked and "Fit both for me" is disabled. The plan should not treat a missing
  prerequisite it fits itself as a block.
- `apps/ui-kit/src/data/data.test.ts` "protection adds mass" fails on `origin/main` (data
  drift, unrelated).
