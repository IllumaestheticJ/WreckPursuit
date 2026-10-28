# data/mods — mod catalog

Owned by workstream 08 (economy). One pack per file: `{ "id": "<file stem>", "mods": [ ModDef, ... ] }`,
loaded by `wp_economy::Economy` (`ModCatalog::from_packs` validates everything). The list of packs
compiled into the browser build is `wp_economy::data::MOD_PACKS`; a test fails if it differs
from this folder.

`ModDef` fields: `id` (snake_case, unique), `name` (no trademarks), `slot` (UI grouping), `line`
(mods in one line replace each other), `tier` 1–5, `price`, optional `requires` (satisfied by the
same line at an equal or higher tier), optional `supersedes` (mods of other lines whose job this
one already does: installing it takes them off at resale value and they cannot go on while it is
fitted; both coilovers supersede `susp_dampers_1`), `class_whitelist`, `modes`, `effects`, `tradeoff`
(required: what the mod costs you besides money), optional `visuals`.

Effects (externally tagged JSON):
- `{"Vehicle": {"param": "engine.torque_scale", "op": "Mul", "value": 1.05}}` — parameter paths
  and allowed ops are listed on `wp_economy::mods::VehicleParam`.
- `{"Durability": {"target": {"Region": "FrontBumper"} | {"Component": "Radiator"} | "CabinCage",
  "scale": 1.8, "mass_kg": 14.0, "material": "hss"}}` — strengthening (`scale > 1` or a cage) must
  add mass; weakening (`scale < 1`) must not.
- `{"Cosmetic": {"kind": "BodyKitFront", "value": "lip_a"}}` — body kits become detachable regions.
- `{"Tool": {"kind": "RadarDetector", "tier": 1}}`.
- `{"Ram": {"zone": "Front" | "Rear" | "Side", "mass_scale": 2.6, "dealt_scale": 2.2,
  "taken_scale": 0.45}}` — battering ram on one face of the car in car-on-car hits: effective
  mass multiplier (1..=4), damage dealt (1..=4), damage taken (0.25..=1). The mod must add mass.
  Faces stack by multiplying. Applied by `wp_sim` (`ram.rs`, `docs/integration/ram-mods.md`).

`visuals` (optional): parts the mod bolts on that you can see, beyond what its effects already say
(`Cosmetic` effects and `wheels.tire_id` are drawn without it):
`"visuals": [{"part": "RearWing", "style": "gt"}]`. Kinds and their styles are
`wp_economy::visuals::PartKind` / `PartKind::styles` (validated at load). Each kind is mounted on
body regions (`PartKind::regions`), so the drawn part dents and detaches with that panel. A mod in
a visible slot (Wheels, Tires, Aero, Body*, Hood, Roof, Paint, Vinyls) must draw something
(`visuals::tests::every_visible_slot_mod_draws_something`).

No licensed brand or product names anywhere (CLAUDE.md rule 9): generic part names only.
