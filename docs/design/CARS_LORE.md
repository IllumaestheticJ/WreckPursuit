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

### Junkyard Beater — `junkyard_beater` · Beater · FWD · 3 000 base / 1 500 floor price
*The economy's floor car (`market.toml [floor_car]`). A tired 1980s 1.6 L four-door with a
4-speed stick, about 65 kW on a good day, 0-100 km/h in about 15 s and flat out near 150 km/h.*
Some years ago it was a family sedan. Now it is one colour on the doors, another on the
hood, and a third where the primer shows through. Every beater in Port Vesper comes from the same
place: the scrap dealers behind the Westline Yards, who will sell you anything with four wheels
and a title for whatever you have in your pocket. It is slow, it pulls left, and the heater is
stuck on. It is also the reason nobody's career ever really ends: when the pound auctions your car
and the bank account says zero, the beater is still there. Plenty of the scene's names did their
first Mercer Meet in one. Light hits fold it like paper, but nobody cares because nothing on it
is worth fixing. The springs sag, the dampers gave up years ago and the tyres are older than
some of the drivers, so it leans like a boat and squeals at every corner — which is exactly why
the old heads say you learn more in a beater than in anything you can buy.
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

### Lindwurm 4T — `lindwurm_4t` · Sport · AWD (on-demand clutch-pack, front-biased, up to 50 % rear) · 34 000
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
full-size rear-drive sedan with a V8, pursuit tires, big brakes and a push bar bolted through to
the front rails. Every precinct runs a fleet of them, and most of the city's stops start with one
in the mirror. It cannot outrun a sports car on a straight and is not supposed to: its job is
shadowing, boxing in, the PIT below the speed ceiling and holding the perimeter (see
`POLICE_DOCTRINE.md`). The push bar takes nose-to-tail shunts and makes the nose a weapon; once a
hard enough hit rips it off, the front is as soft as any sedan's. The long rear overhang crumples
in rear-enders. Retired Cruisers end up at the police auction, and some come back as beaters.
- **Scene role (police):** patrol, perimeter, box-in.
- **Typical ground:** every district; Central and South Precincts run the most.

### Sentinel Wolfhound — `sentinel_wolfhound` · PoliceInterceptor · AWD (on-demand, front-biased, up to 50 % rear) · 45 000
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

### Tansy 1.3 — `tansy_13` · Compact · FWD · 7 000
The city's most common car: a 2000s five-door supermini with a 1.3 L four that fills every
traffic queue on Meridian Avenue and every parking bay in Dunmore. Light, upright and lazy above
100 km/h, it leans hard and understeers safely, which is why driving schools and pizza places buy
them by the dozen. In the scene it is a moving obstacle rather than a rival, and the default
civilian you thread past on Lantern Row. There is very little car ahead of the cabin: frontals
reach the firewall early, a heavier car's nose goes deep into its doors, and the plastic bumpers
crumple at parking-lot speeds.
- **Scene role:** traffic; the cheapest five-door ride home.
- **Best routes:** R10.

### Drayman Cargo — `drayman_cargo` · Truck · RWD · 20 000
A 2000s full-size cargo van with a pushrod V8 on a separate frame, always double-parked outside a
Port District warehouse. It leans like a boat, brakes long and runs out of breath above 150 km/h,
but at city speeds it is a two-and-a-half-tonne wall that wins most shoves. The frame rails take
frontal and rear hits, so the bumpers and short nose crumple while the frame stays straight. The
cargo box behind the seat partition is thin sheet on light hoops: side and rear hits dent it deep
without touching the cab, and the high centre of gravity tips it in a hard swerve.
- **Scene role:** traffic; Delivery runs for crews who need the space.
- **Best routes:** R15, R09.

### Furrow 2.4 — `furrow_24` · Truck · RWD · 21 000
A 90s compact pickup with a 2.4 L four, a regular cab and a leaf-sprung live axle: the tradesman's
truck parked in every other driveway in the suburbs. It is slow, honest and cheap, with a light
rear end that steps out in the wet and a gearbox that likes to be rowed. The ladder frame and steel
bumpers shrug off low-speed knocks and the bed is just bent sheet, but the 90s cab is the weak
point: thin pillars and almost no crumple zone ahead of the short nose, so a hard frontal or
anything above frame height goes into the cab.
- **Scene role:** traffic; a cheap, hard-to-kill first truck.
- **Best routes:** R10, R15.

### Stoat 16V — `stoat_16v` · Compact · FWD · 9 500
An early-90s European three-door hot hatch with a torquey 2.0 L sixteen-valve four: the other
answer to the Vireo. It pulls cleanly from 2 500 rpm and turns in hard, and the short wheelbase and
a stiff rear bar make it lift-off happy, so trailing the brakes into a corner brings the tail
round to meet you. Mercer regulars argue Stoat versus Vireo the way their parents argued football.
There is very little crumple zone ahead of the transverse engine, so the cooling pack goes early in
any frontal, and the long, heavy doors and thin B-pillars make a T-bone nasty. Bumpers and the
steel hood are cheap to replace.
- **Scene role:** first real car for drivers who want torque, not revs.
- **Best routes:** R10, R13, R04.

### Marten 2.0 — `marten_20` · Compact · FWD · 9 500
The original-recipe hot hatch: an early-90s European hatch with a big, torquey 2.0 L in a small
shell. Where the Vireo needs revs, the Marten pulls from 3 000 rpm in any gear, so it is quicker
away from Lantern Row's lights and easier to drive fast, at the price of torque steer in the low
gears. A soft front and a stiff rear bar make it tuck in and rotate on a lift. Short crumple rails
put a moderate nose hit straight into the radiator and engine, and in Simulation one committed
frontal at speed ends it; the doors have no real intrusion beams. Every panel is cheap and bolts
back on.
- **Scene role:** budget junction-sprinter; the Mercer Meet regular's daily.
- **Best routes:** R02, R10, R13.

### Bulwark 400 — `bulwark_400` · Muscle · RWD · 26 000
An early-70s Ostrander four-door with the high-output 6.6 L V8: the family car with a grudge. Two
tonnes on a lazy three-speed, but the torque arrives early and the limited-slip axle on steel
radials lays it down, so it lights up both rear tyres on demand and holds a slide like a barge
holds a course. Soft, slow steering and period brakes make every move a plan. Thick mild-steel
panels and chrome bumpers barely mark in low-speed hits and the long nose gives crush room, but
the cabin is a 70s mild-steel cell, four doors are four soft spots in a T-bone, and a rear hit
tears the leaf-spring hangers out.
- **Scene role:** crew car that carries four; drag and Canal Street nights.
- **Best routes:** R07, R08, R12.

### Brigand 400 — `brigand_400` · Muscle · RWD · 23 500
A mid-70s Ostrander full-size four-door with a four-barrel 6.6 L V8 — the getaway car of every
crime film of its decade. It lays two black lines off the line and keeps pulling to 160 km/h, but
soft springs, slow steering and period brakes on bias-ply tyres make every corner a negotiation.
Slower and heavier than the Ironside, cheaper, and it carries four. It barges through light
traffic on its chrome bumpers, but there is no real crumple zone and the pillars are mild steel, so
a hard frontal goes into the cabin; the leaf hangers tear out in rear hits and the tank behind the
axle makes a hard rear-ender a fire risk.
- **Scene role:** cheap V8 drag car; crew getaway.
- **Best routes:** R07, R08.

### Arashi T4 — `arashi_t4` · Sport · AWD (40/60 permanent) · 32 000
A mid-90s Tsurugi rally-homologation saloon: a turbo 2.0 four, permanent all-wheel drive with a
rear-biased centre diff and a rear limited-slip. It launches like the modern AWD hatch but rotates
on the throttle like a rear-driver — keep the turbo above 3 000 rpm and steer with your right foot.
Long-travel suspension shrugs off Old Harbor cobbles and kerbs that unsettle the low sports cars.
The nose crumples readily and the front intercooler is the first casualty of any frontal; the prop
shaft and centre diff under the floor make sill and kerb strikes expensive.
- **Scene role:** rough-road and wet-night weapon; checkpoint runs.
- **Best routes:** R14, R11, R05.

### Talus 2.0T — `talus_20t` · Sport · AWD (45/55 permanent) · 38 000
A late-90s Tsurugi rally-homologation four-door with a hard-hitting turbo four, short gearing and
limited-slips front and rear: the fastest thing in Port Vesper out of a slow corner, in the wet or
over a kerb. It understeers on a trailing throttle, so the old heads brake late, flick it and drive
it out on the power. The cell is no stronger than any saloon of its era, the big intercooler and
radiator sit right behind a thin bumper so nose hits overheat it early, and heavy sill or kerb
strikes can take out drive to the rear.
- **Scene role:** Arashi's harder, dearer sibling; touge and checkpoint specialist.
- **Best routes:** R04, R05, R14.

### Satchel 2.0 — `satchel_20` · Truck · FWD · 20 000
A 2000s European front-drive high-roof panel van with a 2.0 L petrol four: the parcel van on every
corner. Tall and gutless, with a short first gear for pulling away loaded and not much after that;
empty, it is light at the back, so it understeers safely until a sudden lift lets the rear step
out, and the slab sides catch every crosswind and every shove. The car-style unibody nose crumples
like a hatchback's, the cargo box is thin sheet that dents deep without reaching the cab, and the
high centre of gravity tips it in a hard swerve.
- **Scene role:** traffic; the cheapest Delivery van.
- **Best routes:** R15, R09.

### Tumbril 2.0 — `tumbril_20` · Truck · FWD · 20 000
The Satchel's rival from the other parcel company: a 2000s front-drive high-roof van with a sliding
side door and a little petrol four geared short for stop-start rounds. Slow everywhere and a
crosswind on wheels above 120 km/h, it leans hard and understeers safely, and its empty rear goes
light under hard braking. Smaller, lighter and much slower than the Drayman. With no frame rails
under the cargo box it is softer than the Drayman in every direction; frontals reach the cab
early and the tall flat sides tip it in a hard swerve or a hit at the wheels.
- **Scene role:** traffic; delivery rounds in Dunmore and the Port.
- **Best routes:** R15, R10.

### Fulmine V10 — `fulmine_v10` · Exotic · AWD (30/70 permanent, mid-engine) · 125 000
A 2010s Benvenuti mid-engine supercar with a naturally aspirated 5.2 L V10 that shrieks to
8 500 rpm, a seven-speed dual-clutch and permanent all-wheel drive: the top of the market. It is
the quickest car off the line in any weather and keeps pulling where the Lucciola runs out of
breath, but it is still light at the front and rotates hard on a lift. Wins pay like nothing else;
a crash costs a small house. The bonded aluminium and carbon cell takes a frontal like a modern
car, but everything behind the bulkhead is sacrificial — rear hits kill drive — and the panels
crack and tear rather than dent.
- **Scene role:** endgame flagship; the car the Feared tier exists for.
- **Best routes:** R01, R12, R06.

### Sentinel Warden — `sentinel_warden` · PoliceCruiser · RWD · 26 000
The old workhorse still in service: a late-80s Sentinel body-on-frame sedan with a 5.7 L V8 and a
four-speed automatic. It leans, floats and squats like a boat, its drum rear brakes fade after a
few hard stops, and it has nothing for a sports car beyond 120 km/h — but it is a full-length steel
frame on wheels, and nobody wants it in their door. Low-heat patrols in Dunmore and Southmoor still
run them. The frame and steel bumpers shrug off shunts that fold a modern unibody; the 80s cabin is
the weak point, so hits above the frame rail reach the officers, and the long overhangs swing wide
in a spin.
- **Scene role (police):** low-heat patrol (levels 1–2), traffic stops.
- **Typical ground:** suburbs and the older precincts.

### Sentinel Vigil — `sentinel_vigil` · PoliceInterceptor · RWD · 52 000
The highway-patrol hammer, sent when nothing else can keep up: a 2010s muscle coupe with a 5.0 L
DOHC V8, performance tyres, big brakes and a push bar, upfitted by Sentinel as a slicktop with
visor and rear-window light bars. It runs with the modern sports cars to pursuit speed and brakes
later than any other unit, but two doors, a heavy nose and rear drive make it a handful in the wet
and useless for boxing in a truck. The reinforced bar and doubled bumper beam survive PIT contact;
the long doors and low sills are weak in side hits, and the cooling pack goes once the bar is gone.
- **Scene role (police):** top-heat interceptor (levels 4–5).
- **Typical ground:** Loop 9, the Westline Viaduct, Route 40.

### Halberd 40 — `halberd_40` · Exotic · RWD (mid-engine) · 125 000
A 2020s mid-engine supercar from a small British house: a twin-turbo flat-plane V8 in a carbon
tub, rear drive, a dual-clutch and an active rear wing. Lighter than the Fulmine by almost 200 kg
and with far more torque, it is the hardest-accelerating rear-driver in Port Vesper — and rear
drive is the catch: on its track tyres it lights them up in third on a damp Loop. Calm flat-out,
nervous on a roundabout. The tub shrugs off street-speed hits, but everything bolted to it is
sacrificial: the front crash box and radiators fold early, a rear hit kills drive, carbon panels
crack and shed, and the dihedral doors pop open in side hits.
- **Scene role:** Feared-tier rear-drive weapon; highway outruns.
- **Best routes:** R01, R12.

### Ombra V12 — `ombra_v12` · Exotic · AWD (25 % front, mid-engine) · 124 000
A 2010s Benvenuti flagship with a screaming V12, scissor doors and a body wider than a delivery
van — the loudest thing at any meet. Its automated manual thumps every shift and costs a few tenths
against the dual-clutch cars, but the AWD launch and the twelve's top end keep it on terms with
anything on a long road. It hates tight gaps. Rear hits kill drive, the short nose holds the front
diff and both radiators, the scissor doors tear off in side hits and the wide carbon bodywork
shatters on any contact at ruinous prices.
- **Scene role:** theatre and top speed; pink-slip trophy.
- **Best routes:** R01, R12, R06.

### Ostwind 6T — `ostwind_6t` · Exotic · AWD (30 % front, rear-engine) · 110 000
The everyday missile: a 2020s rear-engined twin-turbo flat-six with a rear-biased AWD coupling.
All its weight sits over the rear wheels, so it launches like nothing else and darts through
traffic on its short wheelbase, but lift mid-corner and the tail swings like a pendulum. It is
the toughest of the supercars: a steel-and-aluminium unibody with a front luggage bay gives real
crumple length, so nose hits are cheap — but the engine hangs behind the rear axle, right where a
rear-ender lands, and one firm rear hit stops it dead.
- **Scene role:** the supercar you can drive every night.
- **Best routes:** R02, R13, R11.

### Kagero HX — `kagero_hx` · Exotic · AWD (electric front axle, mid-engine) · 100 000
A 2010s Tsurugi mid-engine hybrid: twin-turbo V6 behind the cabin, electric motors on the front
axle, nine speeds. Heaviest and least powerful of the supercars on paper, but the electric torque
hits from walking pace and the front motors pull it out of corners, so it is the easiest supercar
to drive fast and the best in the rain on Old Harbor cobbles. It runs out of breath against the V8
and V12 on the Loop. The battery in the tunnel is guarded by the sills, the V6 and gearbox ride
the rear subframe (rear hits kill drive) and the front motors share the nose with the radiators.
- **Scene role:** wet-night and checkpoint supercar.
- **Best routes:** R14, R02, R11.

### Morrigan 5.5 — `morrigan_55` · Exotic · RWD (front-mid, race car) · 120 000
A 2010s GT endurance racer with an air-restricted pushrod V8, a door-sized wing and a sequential
box — a real race car loose in the city. Once it is up to speed nothing on the street corners or
brakes with it; below 80 km/h the aero does nothing, the stiff suspension skips over manhole
covers and the tall first gear hates junction starts. The welded cage makes the cabin tougher than
any road supercar's, but the carbon and fibreglass outside it shatters on contact and the low floor
pays for every kerb.
- **Scene role:** circuit and Loop specialist; hopeless at a red light.
- **Best routes:** R01, R13, R09.

### Nyx Proto — `nyx_proto` · Exotic · RWD (mid-engine prototype) · 125 000
A 2010s closed-cockpit sports prototype under a tonne, with downforce that would hold it to a
ceiling at 250 km/h: the fastest thing on four wheels in Port Vesper and the least suited to its
streets. On a smooth wide road it stops and turns where nothing can follow; in town it grounds on
every crest, needs three points for a U-turn and sees little out of its cockpit. The carbon
survival cell shrugs off almost any hit and almost nothing else does: the nose cone crushes, the
floor and wings shatter, and a rear hit can break the car in two.
- **Scene role:** Loop 9 time-trial legend; a liability anywhere else.
- **Best routes:** R01, R12.

### Merlo 2.0T — `merlo_20t` · Exotic · FWD (touring-car racer) · 100 000
A 2010s touring-car racer built to bang doors: front drive, turbo four, sequential box, a plated
front diff and a stiff rear that steps out on cue when you lift. Slow on the straights next to the
supercars, but it brakes later than any road car and shrugs off the contact that ends their night.
The welded cage, seam-welded shell and steel bumper beam make it the toughest race car in the city;
splitter, wing and bumpers break off on the first touch and are cheap, but the transverse engine
sits right behind the radiator, so hard frontals still end it.
- **Scene role:** contact racing; the cheapest race-built car.
- **Best routes:** R13, R09, R10.

### Mesquite PR — `mesquite_pr` · Truck · RWD (pre-runner) · 48 000
A 2010s full-size pre-runner pickup with a 6.2 L V8, a foot of suspension travel and a locker:
built to fly across the desert at 150 km/h, it treats Port Vesper as a set of jumps. Kerbs, medians
and staircases that break other cars barely register, and it takes short cuts through dirt lots and
parks the supercars cannot follow. On tarmac it rolls like a ship and takes a long time to stop. The
boxed frame and tube bumpers bulldoze traffic; the long-travel front end sticks out past the body,
so offset hits fold a front corner, and the bed and fender flares crumple cheaply.
- **Scene role:** short-cut king; Cannonball and Delivery runs.
- **Best routes:** R11, R15, R05.

### Erg Hamada — `erg_hamada` · Exotic · AWD (50/50 rally-raid prototype) · 110 000
A 2020s desert rally-raid prototype that survived a fortnight of dunes and wants more: an
air-restricted V8, permanent four-wheel drive with plated diffs and 35 cm of travel. Absurdly fast
across rough ground and over anything Port Vesper calls a jump; on smooth tarmac it is wide, tall,
draggy and capped well below supercar speeds, and the locked drivetrain scrubs in tight turns. The
chromoly spaceframe is the strongest cell in the city under composite panels that shatter and fall
off; the long-travel corners hang outside the body and break in offset hits. It keeps driving with
half its bodywork gone.
- **Scene role:** rough-ground outlaw; the Iron Bridge Delivery record holder.
- **Best routes:** R15, R11, R05.

### Debug cars — excluded from the fiction
`debug_sedan` and `debug_coupe` are integration-lead test fixtures, not part of the world. In the
fiction they don't exist. If they ever appear in a player-facing build, call them "test mule"
in the UI and give them no market listing.

## 3. Maker houses (invented)

All names are invented for Port Vesper and must stay that way; if anyone finds a real maker with a
too-similar name, rename it here first.

| House | Origin | Cars | Identity |
|---|---|---|---|
| **Tsurugi Motor Co.** | Japan | Vireo XS, Kestrel TS, Shiro Turbo, Hakone GT, Arashi T4, Talus 2.0T, Kagero HX | Engineering-led, high-revving fours and sixes; the scene's backbone. |
| **Ostrander Motor Works** | USA (Midwest) | Rampart 5.0, Ironside 440, Bulwark 400, Brigand 400 | V8s since forever; heritage plus horsepower. |
| **Falkenrath Fahrzeugbau** | Germany | Sperber 32, Lindwurm 4T | Precise, expensive to fix, beloved by the clean-racing crowd. |
| **Benvenuti** | Italy | Lucciola 350, Fulmine V10, Ombra V12 | Small-volume exotics; parts arrive slowly. |
| **Ridgeback Truck Co.** | USA | Hauler 1500 | Work trucks for the port and the farms beyond Route 40. |
| **Sentinel Fleet Systems** | USA | Sentinel Cruiser, Wolfhound, Bastion, Warden, Vigil | Fleet upfitter building police packages; the push bar is their signature. |

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
