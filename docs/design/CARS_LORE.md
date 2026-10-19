# Cars lore and cosmetic themes

**Owner:** workstream 19 (design docs). **Consumers:** 08 economy (cosmetic catalogue, market
blurbs), 16 UI (garage and market text), 11 art (livery and body-kit direction), 13 audio (engine
character), 15 race (who shows up where).

Setting: Port Vesper (`docs/design/CITY.md`). District, route (R01–R15) and garage names below
come from that document.

## 1. Rules

- **Analog names only.** Lore never names a real manufacturer, model, trim, tuner, parts brand,
  racing series, sponsor or badge, and never uses a sound-alike (CLAUDE.md rule 9,
  `data/cars/README.md`). Era and origin are fine ("a 90s Japanese grand tourer"); a nameplate is
  not. The maker houses in §3 are invented for this game.
- **Lore follows the spec.** Layout, class, era, price and `durability_class` come from
  `data/cars/*.json` and the spec sheets. If a spec changes, the lore changes; it never asks the
  physics to match a story. How a car breaks in the text matches its spec sheet's "How this car
  breaks" section.
- **Grounded.** No supercars that fly and no cars with a legend that makes them invincible. Every
  car here has a weak spot and a repair bill.
- Prices are `base_price` (new, before condition). The used market (`market.toml`) is cheaper.

## 2. The roster

### Junkyard Beater — `junkyard_beater` · Beater · FWD (proposed) · 3 000 base / 1 500 floor price
*No car file yet* (the economy's floor car; `market.toml` references it, workstream 09 owes the
file). Some years ago it was a family sedan. Now it is one colour on the doors, another on the
hood, and a third where the primer shows through. Every beater in Port Vesper comes from the same
place: the scrap dealers behind the Westline Yards, who will sell you anything with four wheels
and a title for whatever you have in your pocket. It is slow, it pulls left, and the heater is
stuck on. It is also the reason nobody's career ever really ends: when the pound auctions your car
and the bank account says zero, the beater is still there. Plenty of the scene's names did their
first Mercer Meet in one. Light hits fold it like paper, but nobody cares because nothing on it
is worth fixing.
- **Scene role:** the floor. Debt-grinding, Mercer Meets, first night.
- **Best routes:** R10, R07.

### Vireo XS — `vireo_xs` · Compact · FWD · 7 500
A late-90s hatchback from Tsurugi with a small four that comes alive above the cam crossover and
screams until you shift. Delivery drivers in Dunmore drove them into the ground, so there are
hundreds on the used market, and a whole generation learned to race in one: the Mercer kids keep
them above 6 000 rpm through traffic on Founders Parkway and get past cars that should be faster.
It has to keep its speed up, because it can't get it back on a straight. Spares are cheap at Sal's
Paint & Panel. The weak spot is the nose: the radiator sits right behind a plastic bumper, a
40 km/h tap cooks it, and in Simulation one committed frontal folds the A-pillars. Drive it clean
or drive it home on a tow hook.
- **Scene role:** first real car; the street-racer's school.
- **Best routes:** R10, R02, R13.

### Kestrel TS — `kestrel_ts` · Sport · FWD (AWD via mod) · 22 000
A mid-2000s Tsurugi sport sedan that looks like the car your accountant drives, until the turbo
comes on at 2 500 rpm and the steering wheel tugs in your hands. The Port Vesper police auction
moves a few every year (former unmarked cars, still with the grommet holes in the parcel shelf),
and that is how most of them reach Roundhouse Customs, where the all-wheel-drive conversion makes
it the rally-bred car it was always meant to be. Four doors, a dull colour and dull plates make it
the classic sleeper for daytime runs through Lantern Row, where looking like a commuter is half the
escape. The intercooler sits first in line, so a medium nose hit leaves it limping on overheat,
and four doors are four weak spots in a T-bone.
- **Scene role:** sleeper, budget AWD project.
- **Best routes:** R02, R14, R11.

### Shiro Turbo — `shiro_turbo` · Sport · RWD · 28 000
An early-2000s Tsurugi coupe with a turbo four, a 1.5-way diff and the most talkative chassis in the
city. The drift crowd took it up in the Westline Yards: the gravel aprons behind the roundhouse
are where the Shiro owners slide in circles under the yard lights until a patrol from Westline
Precinct moves them on. It is light and balanced and not very fast, and it will hold an angle all
the way through Five Points if you are brave and the gates are up. The shell is thin: long doors
and thin sills mean side hits reach the cabin quickly, and a rear-ender bends the axle geometry
into a permanent crab walk. The front end bolts off, which the owners find convenient.
- **Scene role:** drift scene, technical routes, crew show car.
- **Best routes:** R05, R13, R09.

### Hakone GT — `hakone_gt` · Sport · RWD · 48 000
A 90s grand tourer from Tsurugi's flagship line: a straight-six with sequential twin turbos that
feels ordinary until 4 000 rpm and then pulls to the limiter in every gear. On the Loop at 2 a.m.
nothing gets away like it, and the Harbor Skyway is its home. Older owners from Bayview Bluffs keep
them in heated garages and bring them out once a month; younger ones bought theirs from those
older owners' estates. It is a highway escape car, heavy and calm and hard to unsettle, less happy
in Lantern Row's 120 m blocks. Light hits glance off, but the long front overhang puts the
intercooler and both radiators in the way, the big glass hatch shatters early, and every part on
it comes off a boat at Gantry Garage at a price.
- **Scene role:** highway king, the escape car people buy when they can.
- **Best routes:** R01, R12, R06.

### Rampart 5.0 — `rampart_50` · Muscle · RWD · 42 000
A 2010s Ostrander coupe with a 5.0 litre V8, independent rear suspension and more straight-line
pace per dollar than anything else in town. The Canal Street quarter mile is a Rampart parade on
Friday nights, with blown-out exhausts echoing off the mill walls in Canal Flats. Keeping traction
in second gear is the whole skill: once it hooks up, only the Lucciola outruns it. The heavy nose
pushes into corners and snaps loose if you stab the throttle to fix it. Modern crash structure
means the cabin stays whole even in Simulation at moderate speed, but the front clip goes with any
nose hit: the cooling pack and low splitter are scrap after one mistake.
- **Scene role:** drag strip, straight-line bully, crew muscle.
- **Best routes:** R07, R08, R12.

### Ironside 440 — `ironside_440` · Muscle · RWD · 38 000
An early-70s Ostrander big-block coupe: seven litres, a lazy three-speed automatic, chrome bumpers
and brakes that can't lock the tires on dry tarmac. Grandfathers in Dunmore drove them to the
docks; their grandsons drag them out of garages in Southmoor and find out what "plan your stops
early" means on the Kettle Street descent. It wallows, pitches and steers like a boat, and it will
pull away from anything at a light. The thick steel panels dent all night without falling off, so
light hits cost almost nothing. A hard frontal is another matter: there is no crumple zone, the
pillars are mild steel, and in Simulation it is the most dangerous car in the city to crash.
Rear-enders tear the leaf-spring hangers out of the floor, and the fuel tank sits right behind the
axle.
- **Scene role:** heritage muscle, shoving matches, the car that scares everyone including its driver.
- **Best routes:** R07, R08, R03.

### Sperber 32 — `sperber_32` · Sport · RWD · 36 000
A 2000s Falkenrath sport sedan with a naturally aspirated six that wants 8 000 rpm and a
near-50:50 chassis that steers on the throttle. Uptown lawyers bought them new; Mercer bought them
third-hand. On Hollins Drive nothing in its price bracket is faster, and its four doors and sober
paint make it the getaway car that doesn't look like one. It punishes lazy gear choices and rewards
drivers who know the route. The shell is solid and handles a frontal well, but the long nose holds
a big radiator and the sump hangs low, so kerb strikes and nose hits reach the engine sooner than
owners expect. The heavy steel doors hold up in side hits.
- **Scene role:** touge and technical-route specialist; clean racer's car.
- **Best routes:** R04, R05, R13.

### Lindwurm 4T — `lindwurm_4t` · Sport · AWD (65/35, clutch-pack) · 34 000
A 2010s Falkenrath hot hatch with a turbo four, a dual-clutch gearbox and all-wheel drive that
makes it the quickest thing off a Lantern Row light and the most forgiving fast car in Port
Vesper. It's what you drive when it's raining, when you don't trust yourself, or when the race
starts on Old Harbor cobblestones. The scene calls it boring, and it wins anyway. It understeers
when pushed and runs out of breath on the Loop where the rear-drive cars keep pulling. Its
ultra-high-strength cabin is one of the safest in the city, and the plastic outside the cell is
cheap to fix. The rear coupling and prop shaft under the floor die in heavy kerb or sill strikes,
leaving a front-drive car in an all-wheel-drive race.
- **Scene role:** all-weather weapon, cannonball favourite.
- **Best routes:** R11, R14, R02.

### Lucciola 350 — `lucciola_350` · Exotic · RWD (mid-engine) · 120 000
A 90s Benvenuti mid-engine coupe with a flat-plane V8 that screams to 8 500 rpm. It is the poster
on every Mercer bedroom wall, and maybe four of them run in the city. One belongs to a Bayview
Bluffs collector who has never taken it past the end of Bluff Road; the rest belong to people who
do. It is the fastest thing through a corner in Port Vesper and it rotates the instant you lift.
The economics are a glass cannon's: a win in it pays four times a Compact purse, and a crash costs
more than most cars are worth. Everything behind the cabin is sacrificial: the aluminium rear
subframe carries engine, gearbox and both rear corners, so any rear hit kills drive. The side fuel
tanks make T-bones dangerous, aluminium panels tear instead of denting, and the parts come from
Italy, slowly.
- **Scene role:** endgame trophy; pink-slip bait.
- **Best routes:** R06, R04, R01.

### Hauler 1500 — `hauler_1500` · Truck · RWD · 32 000
A 2010s Ridgeback half-ton crew-cab pickup with a pushrod V8, a full-length frame and steel
bumpers. In the Vesper Container Terminal it is everyone's work truck; in the scene it is the car
that goes through the roadblock first. Captains learn its silhouette quickly. It is slow to turn,
slow to stop and top-heavy enough to lift a wheel in a hard swerve, but it wins every shoving
match and accelerates harder than it should. Crew runs on the Cross-Town Cannonball often put one
at the front to clear the way. The frame rails take the hits, so bumpers, grille and bed get
mangled while the cab stays straight. The side-saddle fuel tank is exposed in T-bones, and a
rollover goes straight into the cab.
- **Scene role:** blocker, crew escort, Delivery specialist.
- **Best routes:** R15, R11, R09.

### Sentinel Cruiser — `sentinel_cruiser` · PoliceCruiser · RWD · 30 000
The everyday patrol car of the Port Vesper Police Department, built by Sentinel Fleet Systems on a
full-size rear-drive sedan with a V6, pursuit tires, big brakes and a push bar bolted through to
the front rails. Every precinct runs a fleet of them, and most of the city's stops start with one
in the mirror. It cannot outrun a sports car on a straight and is not supposed to: its job is
shadowing, boxing in, the PIT below the speed ceiling and holding the perimeter (see
`POLICE_DOCTRINE.md`). The push bar takes nose-to-tail shunts and makes the nose a weapon; once a
hard enough hit rips it off, the front is as soft as any sedan's. The long rear overhang crumples
in rear-enders. Retired Cruisers end up at the police auction, and some come back as beaters.
- **Scene role (police):** patrol, perimeter, box-in.
- **Typical ground:** every district; Central and South Precincts run the most.

### Sentinel Wolfhound — `sentinel_wolfhound` · PoliceInterceptor · AWD (55/45) · 45 000
The pursuit specialist: twin-turbo V6, all-wheel drive, the same reinforced front as the
Cruiser on a modern cell. Highway Patrol Post 9 runs them on the Loop and Central Precinct keeps a
pair downtown. A Wolfhound's driver does not try to win a top-speed race. They stay on your bumper
out of every junction and never let the gap open until the helicopter has you. They are heavy
and tall, so a real sports car leaves them in fast corners, and AWD hardware under the floor means
a sill or tunnel strike can cut drive to one axle. Heavy enough to hurt whatever it hits.
- **Scene role (police):** lead pursuit unit, highway interceptor.
- **Typical ground:** Loop 9, Lantern Row, Old Harbor.

### Sentinel Bastion — `sentinel_bastion` · PoliceSuv · RWD · 48 000
A full-size body-on-frame SUV with a ladder frame, a V8 and a reinforced push bar. It is the
roadblock on wheels. Captains park it across the Meridian Bridge or the mouth of the Iron Bridge
and dare you. In a pursuit it is the slowest unit and leans like a ship, but it ends things by
mass: a Bastion nudge at 50 km/h moves a sports car sideways. Frontal and rear shunts bend
bodywork long before anything structural, but the high centre of gravity means a hard hit at the
wheels can trip it, and the big glass shatters early. South Precinct and the Port units favour it
because the terminal's trucks don't care about anything smaller.
- **Scene role (police):** roadblocks, box-in anchor, port and suburb patrol.
- **Typical ground:** river crossings, Port districts, Dunmore.

### Debug cars — excluded from the fiction
`debug_sedan` and `debug_coupe` are integration-lead test fixtures, not part of the world. In the
fiction they don't exist. If they ever appear in a player-facing build, call them "test mule"
in the UI and give them no market listing.

## 3. Maker houses (invented)

All names are invented for Port Vesper and must stay that way; if anyone finds a real maker with a
too-similar name, rename it here first.

| House | Origin | Cars | Identity |
|---|---|---|---|
| **Tsurugi Motor Co.** | Japan | Vireo XS, Kestrel TS, Shiro Turbo, Hakone GT | Engineering-led, high-revving fours and sixes; the scene's backbone. |
| **Ostrander Motor Works** | USA (Midwest) | Rampart 5.0, Ironside 440 | V8s since forever; heritage plus horsepower. |
| **Falkenrath Fahrzeugbau** | Germany | Sperber 32, Lindwurm 4T | Precise, expensive to fix, beloved by the clean-racing crowd. |
| **Benvenuti** | Italy | Lucciola 350 | Small-volume exotics; parts arrive slowly. |
| **Ridgeback Truck Co.** | USA | Hauler 1500 | Work trucks for the port and the farms beyond Route 40. |
| **Sentinel Fleet Systems** | USA | Sentinel Cruiser, Wolfhound, Bastion | Fleet upfitter building police packages; the push bar is their signature. |

Badges, grilles and logos for these houses are art direction's (11) job: simple geometric marks,
never a modified real logo.

## 4. Cosmetic themes (for `wp_economy`)

Gameplay facts that cosmetics must respect:
- **Body-kit pieces become detachable regions** (`BodyKitFront/Side/Rear`, via 03's
  `gen-lattice` extension points). A widebody tears off in a hit like any bumper. So a kit is
  partly a gameplay choice: it is more parts to lose and more parts to pay for.
- **Liveries and vinyls live in the 0–0.5 U band** of every car mesh (prompt 11 mesh spec). A
  theme is a set of textures in that band plus optional kit pieces; it must work on all body
  styles in the roster.
- **Visibility is a mechanic.** A loud livery makes the car easier for witnesses to describe
  (the stripes mod's tradeoff already says so). If 06 ever models descriptions, matte black and
  plain paint are the stealth option.

### 4.1 What the existing values look like

| Kind | Value | Look | In-world origin |
|---|---|---|---|
| BodyKitFront | `lip_a` | Thin black splitter lip under the stock bumper | Five Points Speed Parts, bolt-on |
| BodyKitFront | `wide_a` | Wide bumper that matches the widebody arches, big lower intake | Roundhouse Customs fabrication |
| BodyKitFront | `aggro_a` | Deep bumper, mesh openings, canards | Mail-order "time attack" style kits |
| BodyKitSide | `skirt_a` | Flat side skirts, lower the visual line | Five Points Speed Parts |
| BodyKitSide | `wide_a` | Riveted overfenders, exposed bolts | Roundhouse Customs |
| BodyKitRear | `rear_a` | Sport rear bumper with a diffuser strip | Five Points Speed Parts |
| BodyKitRear | `wide_a` | Wide rear bumper matching the arches | Roundhouse Customs |
| HoodStyle | `vented` | Twin heat-extractor vents | Rally and track builds |
| HoodStyle | `scoop` | Raised centre scoop | Muscle scene, Canal Street |
| RoofStyle | `scoop` | Roof intake (mostly for show) | Rally styling |
| RoofStyle | `rack` | Low steel roof rack | Workhorse and port look |
| Lights | `smoked` | Smoked lamp covers | Night scene staple (makes the car harder to read in the mirror) |
| Paint | `gloss` | Solid gloss | Factory |
| Paint | `metallic` | Metal-flake | Factory premium |
| Paint | `matte` | Flat finish, shows every scrape | Wrap shops in Midtown |
| Paint | `pearl` | Colour-shift pearl | Show cars; expensive to match after a crash |
| Vinyl | `stripes` | Twin over-the-top stripes | Muscle heritage |
| Vinyl | `flames` | Classic flames from the front wheels | Hot-rod heritage |
| Vinyl | `tribal` | Sharp black tribal graphics | Early-2000s tuner culture |
| Vinyl | `sponsor_generic` | Fake race-team livery with local sponsor decals (§4.3) | Day-event scene |
| Wheels | `steel` | Painted steel wheels, sometimes hubcaps | Fleet and beater |
| Wheels | `mesh` | Multi-spoke mesh, gold or silver | 90s import style |
| Wheels | `forged_5spoke` | Light forged five-spoke | Track crowd; bend on kerbs |
| WindowTint | `dark` | Dark tint | Everyone at night |
| Interior | `suede_look` | Suede-look trim | Show cars |

### 4.2 Proposed themes (collections)

Each theme is a set of new cosmetic values in existing kinds. Unlock sources map to existing
systems: shop purchase (08), rep tier (08), crew territory (15) and event reward (15).

| Theme | Story | New values (kind → value) | Suits | Unlock |
|---|---|---|---|---|
| **Harbor Rust** | Port workers' cars kept running on salt air and spare parts | Paint → `patina`; Vinyl → `rust_bloom`; Wheels → `steel_black`; RoofStyle → `rack` (existing) | Beater, Truck, Muscle (Ironside) | Gantry Garage stock |
| **Lantern Neon** | Downtown night scene: underglow colours, bright trim | Vinyl → `neon_lines`; Lights → `underglow_cyan`, `underglow_magenta`; Paint → `candy` | Compact, Sport | Rep tier (night race wins in Lantern Row) |
| **Westline Freight** | Yard crews: stencils, hazard stripes, rail-company greys | Vinyl → `freight_stencil`, `hazard_chevron`; Paint → `primer_grey`; BodyKitFront → `bull_bar_a` | Truck, Muscle, Beater | Roundhouse Customs; Westline Yards territory |
| **Bluff Line** | Old-money coastal touring: subtle pinstripes, polished wheels | Vinyl → `pinstripe`; Paint → `two_tone`; Wheels → `polished_classic` | Sport (Hakone, Sperber), Exotic | Meridian Motorworks; rep tier |
| **Knob Runner** | Touge crews on Hollins Drive: number boards, tow hooks, mismatched panels | Vinyl → `touge_number`, `mismatched_panels`; BodyKitFront → `tow_hook_a`; Wheels → `mesh_bronze` | Sport RWD (Shiro, Sperber), Compact | Event reward: R04/R05 wins |
| **Dunmore Workhorse** | Fleet-auction cars made respectable: steelies, plain paint, a single stripe | Paint → `fleet_white`; Vinyl → `single_stripe`; Wheels → `steel` (existing) + `hubcap_dish` | Kestrel, Beater, Compact | Default shop stock (cheap) |
| **Uptown Clean** | Clean racers: understated, factory-plus | Paint → `pearl` (existing); BodyKitFront → `lip_a` (existing); Vinyl → `none_badge_delete`; Wheels → `forged_10spoke` | Sport, Exotic | Clean-race rep tier |
| **Auction Block** | Decommissioned police look: black-and-white panels with the decals stripped and the light-bar holes plugged | Paint → `ex_fleet_bw`; Vinyl → `decal_ghost`; RoofStyle → `bar_mount_plugs` | Kestrel, Beater, Sport sedans (Sperber) | Police auction (rare) |

Rules for themes:
- No values that imitate real police markings well enough to pass as a real department (Auction
  Block is clearly decommissioned: ghosted decals, no lettering or crest).
- Neon underglow is cosmetic only, though at night it may be the brightest thing on the car. If 06
  adds visual-identification heat, it should count here.
- New body-kit values (`bull_bar_a`, `tow_hook_a`) are detachable regions like every kit piece. A
  bull bar is the exception that may deserve a small `Durability` effect, which would make it a
  Protection mod with mass and a tradeoff, not a cosmetic. 08 decides.

### 4.3 Invented sponsors for `sponsor_generic` and theme decals

All invented, all local to Port Vesper. None may resemble a real brand's name, logo or colours.

| Sponsor | What | Decal style |
|---|---|---|
| Kettle Street Tire & Rim | Tire shop near the drawbridge | Black on yellow, tread-pattern border |
| Gantry Lube | Industrial oil and lubricants | Crane silhouette, orange |
| WVSP 97.3 "The Static" | Late-night radio station (the scanner-and-synthwave show) | Waveform logo, scanner green |
| Five Points Speed Parts | Aftermarket parts counter in Canal Flats | Five-pointed junction mark |
| Lantern Row Diner | 24-hour diner downtown | Gas-lamp logo, red script |
| Vesper Towing 24/7 | The tow company that profits from everything | Hook logo, phone number |
| Dunmore Battery Supply | Batteries and electrics | Lightning bolt, blue |
| Harbor Ice & Bait | Fish-market supplier | Fish outline, white on teal |
| Calder Bay Ferry Co. | Ferry operator | Wave stripe |
| Starlite Drive-In | The derelict drive-in, now a meet venue | Retro star, faded |

## 5. Proposed additions for 08

Price bands follow the existing tiers (tier 1 ≈ 200–600, tier 2 ≈ 600–1 200, tier 3 ≈ 1 200–2 500,
tier 4 ≈ 2 500+). Mod ids follow the existing `<slot>_<name>` pattern.

| Mod id | Kind | Value | Price band | Theme |
|---|---|---|---|---|
| `paint_patina` | Paint | `patina` | tier 1 | Harbor Rust |
| `vinyl_rust_bloom` | Vinyl | `rust_bloom` | tier 1 | Harbor Rust |
| `wheels_steel_black` | Wheels | `steel_black` | tier 1 | Harbor Rust |
| `vinyl_neon_lines` | Vinyl | `neon_lines` | tier 2 | Lantern Neon |
| `body_underglow_cyan` | Lights | `underglow_cyan` | tier 2 | Lantern Neon |
| `body_underglow_magenta` | Lights | `underglow_magenta` | tier 2 | Lantern Neon |
| `paint_candy` | Paint | `candy` | tier 4 | Lantern Neon |
| `vinyl_freight_stencil` | Vinyl | `freight_stencil` | tier 1 | Westline Freight |
| `vinyl_hazard_chevron` | Vinyl | `hazard_chevron` | tier 1 | Westline Freight |
| `paint_primer_grey` | Paint | `primer_grey` | tier 1 | Westline Freight |
| `body_front_bull_bar` | BodyKitFront | `bull_bar_a` | tier 2 | Westline Freight (see §4.2 note) |
| `vinyl_pinstripe` | Vinyl | `pinstripe` | tier 2 | Bluff Line |
| `paint_two_tone` | Paint | `two_tone` | tier 3 | Bluff Line |
| `wheels_polished_classic` | Wheels | `polished_classic` | tier 3 | Bluff Line |
| `vinyl_touge_number` | Vinyl | `touge_number` | tier 1 (event reward) | Knob Runner |
| `vinyl_mismatched_panels` | Vinyl | `mismatched_panels` | tier 1 | Knob Runner |
| `body_front_tow_hook` | BodyKitFront | `tow_hook_a` | tier 1 | Knob Runner |
| `wheels_mesh_bronze` | Wheels | `mesh_bronze` | tier 2 | Knob Runner |
| `paint_fleet_white` | Paint | `fleet_white` | tier 1 | Dunmore Workhorse |
| `vinyl_single_stripe` | Vinyl | `single_stripe` | tier 1 | Dunmore Workhorse |
| `wheels_hubcap_dish` | Wheels | `hubcap_dish` | tier 1 | Dunmore Workhorse |
| `vinyl_badge_delete` | Vinyl | `none_badge_delete` | tier 1 | Uptown Clean |
| `wheels_forged_10spoke` | Wheels | `forged_10spoke` | tier 4 | Uptown Clean |
| `paint_ex_fleet_bw` | Paint | `ex_fleet_bw` | tier 2 (auction only) | Auction Block |
| `vinyl_decal_ghost` | Vinyl | `decal_ghost` | tier 1 (auction only) | Auction Block |
| `body_roof_bar_plugs` | RoofStyle | `bar_mount_plugs` | tier 1 (auction only) | Auction Block |

Open questions for 08:
1. A two-element body-kit front (`bull_bar_a`, `tow_hook_a`) shares the `BodyKitFront` slot with
   bumpers, so you can't run both. Is a separate `Accessory` kind wanted (pairs with
   `BodyRegion::Accessory`, accepted but deferred to contract v0.3.0)?
2. Should `forged_10spoke` carry the same `wheels.mass_kg` reduction as `forged_5spoke`? If so it
   stops being purely cosmetic and needs a tradeoff line.
