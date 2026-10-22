# data/cosmetics — what mods look like on the car

`visuals.json` maps every mod id in `data/mods/*.json` to the pieces the testbed draws on the
car (`apps/testbed-web/src/render/cosmetics/`), or to `{"none": "<why it cannot be seen>"}` for
parts that live inside the engine, gearbox or dash. Render only: the sim never reads this file.

```json
{ "format": 1, "mods": { "prot_bull_bar_tube": [{"piece": "bull_bar", "style": "tube"}], ... } }
```

`crates/wp_economy/tests/visuals.rs` checks that every mod has an entry, every entry is a mod,
and every mod with a `Cosmetic` effect has pieces. Adding a mod means adding a line here.

Pieces place themselves from the car's panel mesh (bumper, hood, trunk, roof, door, lamp and
wheel positions), so one piece fits every model. Bolt-ons ride with their body region's lattice
and fall off with the panel they are on.

| Piece | Options | Drawn by |
|---|---|---|
| `splitter` | `style`: `lowdrag` | body.ts |
| `canards`, `lip` (`style`: `aggressive`), `flares` (`axle`), `skirts` (`style`: `wide`), `rear_valance`, `diffuser` | | body.ts |
| `wing` | `style`: `street`, `ducktail`, `gt`, `drift`, `swan` | body.ts |
| `hood_vents`, `hood_scoop`, `roof_scoop`, `roof_rack`, `roof_led_bar` | | body.ts |
| `lights` | `style`: `smoked`, `xenon`, `amber`, `halo` | body.ts + vehicle_view.ts lamp colours |
| `fog_lights` (`style`: `round`, `led_pods`), `underglow` (`colour`), `mud_flaps` | | body.ts |
| `exhaust_tip` | `style`: `sport`, `race`, `cannon`, `dual`, `quad`, `burnt` | body.ts |
| `bull_bar` | `style`: `tube`, `guard`, `ram`, `push` | body.ts |
| `spotlight`, `steel_bumper` (`end`), `skid_plate`, `rad_guard`, `armor_plates`, `door_bars`, `antenna` (`style`: `whip`, `stub`), `plates` | | body.ts |
| `rims` | `style`: `steel`, `black_steel`, `mesh`, `gold_mesh`, `forged_5spoke`, `turbofan`, `deep_dish`, `split_spoke` | wheels.ts |
| `tyres` | `style`: `sport`, `slick`, `drag`, `all_terrain`, `wide` | wheels.ts |
| `sidewall` | `style`: `whitewall`, `letters`, `red_line` | wheels.ts |
| `hub_guards`, `brakes` (`level`), `springs` (`colour`), `dampers`, `sway_bar` (`axle`) | | wheels.ts |
| `seats`, `interior` (`colour`), `strip` (`level`), `cage` (`level`), `shifter` (`style`), `hydro_handbrake`, `dash_box`, `nitrous` (`bottles`), `fuel_cell`, `window_net` | | cabin.ts |
| `radiator` (`style`), `intercooler`, `oil_cooler`, `intake` (`style`), `turbo` (`size`), `blower` (`size`), `engine_dress`, `fuel_rail` | | cabin.ts |
| `paint` (`finish`: `gloss`, `metallic`, `matte`, `pearl`, `satin`, `chrome`), `vinyl` (`design`), `carbon` (`panel`), `tint` (`style`), `glass` (`style`: `poly`, `shatterproof`) | | cabin.ts → look.ts materials |

Vinyl designs (`vinyl` pieces, `render/cosmetics/vinyls.ts`): `stripes`, `flames`, `tribal`,
`sponsor_generic`, `side_slash`, `pinstripe`, `race_number`, `checker`, `lightning`, `splatter`,
`hazard`, `shark_mouth`, `retro_bands`, `camo_urban`, `camo_digital`, `sunset_fade`, `circuit`,
`skull_trail`. They are drawn in car metres into the livery atlas, so each fits every model.
Words on vinyls are made up; no real brands or logos (CLAUDE.md rule 9).
