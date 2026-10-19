# PROMPT 16 — Game UI kit: HUD, garage/mod tree, scanner feed, captain map, menus (`apps/ui-kit`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/DESIGN_OVERVIEW.md` §5. You own a new package `apps/ui-kit` (TypeScript, framework: Svelte or Preact — pick one, justify, keep it tiny) consumed later by `apps/testbed-web` (prompt 04 owns that; do not edit it — provide a mount API and a Storybook-style demo page). Branch `ws/ui`.

## Goal
Every screen the prototype needs, built against mock data shaped like the real contracts (`schemas/*.json` → generate TS types with `json-schema-to-typescript`), so the moment the sim exposes the data the UI is done. Style: option B "stylized low-poly, dark" from the design overview — scanner-green monospace accents, grime, high-contrast night readability. Load the `frontend-design` guidance if available in your session.

## Screens
HUD (speed/rpm/gear, damage silhouette by `BodyRegion` energy, component list, heat/wanted meter, minimap stub, radar/scanner alerts, race position/checkpoints/gap, assist badges), pause/settings (full input remapping incl. gamepad/wheel binding flow, assist matrix, damage mode selector, accessibility: colourblind palettes, UI scale, rumble), garage (car list, condition, repair/scrap/drive-damaged decision with itemised bill, insurance), mod tree (slots, tiers, requirements, performance-vs-durability deltas with before/after numbers, class index), market (buy/sell), race lobby/meet screen, results + payout breakdown, impound screen, crew/territory map, police side (rank, unlocks, orders feed), **captain map** (2D canvas: roads, units, incidents, heat map, command radial, unit assignment drag), scanner feed, replay controls. All data via a single `GameModel` observable interface with a mock provider that replays `events.jsonl` from `apps/simcli` output.

## Deliver
Package with demo page (`npm run demo`), mount API docs, mock provider, type generation script, `docs/integration/ui.md` (data each screen needs and which crate owns it; gaps become contract requests).
