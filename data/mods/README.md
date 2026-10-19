# data/mods — mod catalog

Owned by workstream 08 (economy). One pack per file: `{ "id": "<file stem>", "mods": [ ModDef, ... ] }`,
loaded by `wp_economy::Economy` (`ModCatalog::from_packs` validates everything). The list of packs
compiled into the browser build is `wp_economy::data::MOD_PACKS`; a test fails if it differs
from this folder.

`ModDef` fields: `id` (snake_case, unique), `name` (no trademarks), `slot` (UI grouping), `line`
(mods in one line replace each other), `tier` 1–5, `price`, optional `requires` (satisfied by the
same line at an equal or higher tier), `class_whitelist`, `modes`, `effects`, `tradeoff`
(required: what the mod costs you besides money).

Effects (externally tagged JSON):
- `{"Vehicle": {"param": "engine.torque_scale", "op": "Mul", "value": 1.05}}` — parameter paths
  and allowed ops are listed on `wp_economy::mods::VehicleParam`.
- `{"Durability": {"target": {"Region": "FrontBumper"} | {"Component": "Radiator"} | "CabinCage",
  "scale": 1.8, "mass_kg": 14.0, "material": "hss"}}` — strengthening (`scale > 1` or a cage) must
  add mass; weakening (`scale < 1`) must not.
- `{"Cosmetic": {"kind": "BodyKitFront", "value": "lip_a"}}` — body kits become detachable regions.
- `{"Tool": {"kind": "RadarDetector", "tier": 1}}`.

No licensed brand or product names anywhere (CLAUDE.md rule 9): generic part names only.
