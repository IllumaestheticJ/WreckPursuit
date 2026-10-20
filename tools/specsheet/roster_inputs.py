"""Engineering inputs for the WreckPursuit car roster (workstream 09).

Every number in `data/cars/<id>.json`, `data/cars/<id>.spec.md` and the hand adjustments in
`data/lattices/<id>.json` is derived from the entries below by `build_roster.py`. Edit here,
then run `python3 tools/specsheet/build_roster.py`.

Naming policy: no real makers, models, badges or trademarked tech names anywhere (see
`data/cars/README.md`). `nameplate_check.py` enforces it.

Units: SI unless the key says otherwise (`_mm`, `_deg`, `_bar`). Tire sizes use the generic
ISO metric code `width/aspect R rim` (a size, not a brand).

Per-car keys
------------
id, display_name, cls, analog_of, era ('70s' | '80s' | '90s' | '2000s' | '2010s'), profile
('hatch' | 'sedan' | 'coupe' | 'mr' | 'truck' | 'suv' | 'van'), curb_kg, front_frac, wheelbase,
track_f, track_r, body (w, l, h), fuel_l (tank, L), clearance, front_overhang (front axle → bumper face, m), cg (m), cg_why, inertia_k (roll, pitch, yaw
scale on the box formula), cd, area_k (frontal area = area_k·w·h), extra_cda (lightbar etc.),
lift (front, rear Cl·A), engine{...}, trans{...}, tires{...}, susp{...}, align{...},
brakes{...}, steer{...}, price, durability, note, breaks, sources, lattice{...}.
"""

DRIVER_KG = 75.0  # one driver
FUEL_KG_PER_L = 0.745  # petrol
# VehicleDef.mass_kg = curb (full tank, as manufacturer curb figures are quoted) − half a tank
# + one driver: the contract (v0.2.0) puts fuel in mass_kg at 50 % fill.

# Brake line pressure at full pedal with ABS cycling, bar. Modern boosted systems reach
# 100-120 bar in a panic stop; 70s single-diaphragm boosters with organic pads ~65 bar.
P_MODERN = 100.0
P_70S = 65.0

CARS = [
    # ------------------------------------------------------------------ 1
    dict(
        id="vireo_xs",
        display_name="Vireo XS",
        cls="Compact",
        analog_of="late-90s Japanese FF hot hatch, 1.6 L naturally aspirated I4 with two-stage cam",
        era="90s",
        profile="hatch",
        curb_kg=1050.0,
        fuel_l=45,
        front_frac=0.62,
        wheelbase=2.62,
        track_f=1.475,
        track_r=1.475,
        body=(1.695, 4.18, 1.37),
        clearance=0.14,
        front_overhang=0.85,
        cg=0.50,
        cg_why="small hatch with tall greenhouse; class band 0.45-0.52 m, upper end for the short wheelbase and high seating",
        inertia_k=(0.80, 0.90, 0.92),
        cd=0.33,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.10, 0.12),
        engine=dict(
            desc="1.6 L DOHC 16-valve I4, naturally aspirated, two-stage variable cam (crossover ~5300 rpm)",
            curve=[(1000, 98), (1500, 112), (2000, 123), (2500, 131), (3000, 136), (3500, 139),
                   (4000, 141), (4500, 142), (5000, 141), (5500, 150), (6000, 152), (6500, 148),
                   (7000, 140), (7600, 124), (7900, 110)],
            idle=800, redline=7600, limit=7900, inertia=0.10, brake_frac=0.12,
            why="Class-typical specific output for a late-90s high-rev 1.6 (~64 kW/L); the torque step at the cam crossover is the signature of owner dyno sheets for this engine family.",
        ),
        trans=dict(
            desc="5-speed manual, open front differential",
            layout="Fwd",
            gears=[3.23, 1.90, 1.27, 0.97, 0.78], final=4.40, reverse=3.00, shift=0.25,
            diff_f="Open", diff_r="Open", clutch_factor=1.4,
        ),
        tires=dict(front="195/55R15", rear="195/55R15", compound="street", unsprung=(18.0, 16.0)),
        susp=dict(hz=(1.55, 1.70), zeta=0.30, travel=(0.16, 0.17), roll_grad_deg_g=3.0,
                  roll_front_share=0.42, bump_stop=150000.0, preload=0.02),
        align=dict(camber_deg=(-0.5, -1.0), toe_deg=(0.0, 0.15), caster_deg=3.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=262, pistons_mm=[54], pad_mu=0.38),
            rear=dict(kind="disc", d_mm=239, pistons_mm=[34], pad_mu=0.38),
            bar=P_MODERN, handbrake_frac=0.9,
        ),
        steer=dict(ratio=15.0, turn_circle_m=10.0, ackermann=0.7, rack_speed=1.5),
        price=7500,
        durability=2,
        note=("The beater-tier hero: light, eager and cheap to fix. It is slow in a straight line "
              "but carries speed through traffic, and its high-revving four rewards keeping it "
              "above the cam crossover. Front-heavy FF balance with a stiff rear bar gives "
              "lift-off rotation for players who learn it; everyone else gets safe understeer."),
        breaks=("Thin 90s unibody with modest crumple zones. Plastic bumpers and the steel hood "
                "crumple at parking-lot speeds; the transverse engine sits right behind the "
                "radiator so a 40 km/h nose hit already cooks the cooling pack. The cabin is "
                "acceptable for its era but the A-pillars fold in a hard offset hit, so in "
                "Simulation one committed frontal at speed is the end of it."),
        sources=["manufacturer spec-class data for late-90s 1.6 L FF hatches (mass, dimensions, gearing)",
                 "owner chassis-dyno averages for the high-rev two-stage-cam 1.6 engine family",
                 "engineering tables: tire size → radius, ride-frequency spring sizing, brake torque from caliper hydraulics"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="steel", pushbar=False),
    ),
    # ------------------------------------------------------------------ 2
    dict(
        id="kestrel_ts",
        display_name="Kestrel TS",
        cls="Sport",
        analog_of="mid-2000s Japanese FF sport sedan, 2.0 L turbocharged I4 (AWD variant via mod)",
        era="2000s",
        profile="sedan",
        curb_kg=1400.0,
        fuel_l=60,
        front_frac=0.61,
        wheelbase=2.64,
        track_f=1.53,
        track_r=1.52,
        body=(1.77, 4.60, 1.45),
        clearance=0.14,
        front_overhang=0.95,
        cg=0.52,
        cg_why="mid-size sedan with turbo plumbing high in the bay; class band 0.45-0.52 m",
        inertia_k=(0.80, 0.90, 0.92),
        cd=0.31,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.08, 0.10),
        engine=dict(
            desc="2.0 L DOHC I4, single turbo, intercooled, plateau 2500-4500 rpm",
            curve=[(1000, 150), (1500, 205), (2000, 280), (2500, 330), (3000, 335), (3500, 335),
                   (4000, 333), (4500, 325), (5000, 310), (5500, 295), (6000, 288), (6500, 262),
                   (6800, 240)],
            idle=800, redline=6500, limit=6800, inertia=0.13, brake_frac=0.11,
            why="Class-typical 2000s 2.0 T: ~90 kW/L, torque plateau limited by the factory boost map; spool shape from owner dyno averages.",
        ),
        trans=dict(
            desc="6-speed manual, mild clutch-type front LSD (AWD centre coupling is a mod)",
            layout="Fwd",
            gears=[3.54, 2.13, 1.48, 1.12, 0.89, 0.73], final=4.06, reverse=3.30, shift=0.25,
            diff_f=dict(preload=40.0, power=0.30, coast=0.10), diff_r="Open", clutch_factor=1.35,
        ),
        tires=dict(front="225/45R17", rear="225/45R17", compound="sport", unsprung=(22.0, 20.0)),
        susp=dict(hz=(1.50, 1.60), zeta=0.30, travel=(0.16, 0.17), roll_grad_deg_g=3.0,
                  roll_front_share=0.55, bump_stop=150000.0, preload=0.02),
        align=dict(camber_deg=(-0.7, -1.2), toe_deg=(0.0, 0.15), caster_deg=4.5),
        brakes=dict(
            front=dict(kind="disc", d_mm=300, pistons_mm=[57], pad_mu=0.40),
            rear=dict(kind="disc", d_mm=280, pistons_mm=[38], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.8,
        ),
        steer=dict(ratio=15.0, turn_circle_m=11.2, ackermann=0.6, rack_speed=1.4),
        price=22000,
        durability=3,
        note=("A sleeper sedan with a big turbo hit: lazy below 2500 rpm, then a wall of torque "
              "that tugs the wheel. Front-drive in stock form, so it is about managing torque "
              "steer and wheelspin out of junctions; the drivetrain mod converts it to the "
              "all-wheel-drive rally-bred variant."),
        breaks=("Mid-2000s HSS cabin with decent front rails: it survives a 56 km/h frontal with "
                "the cabin intact but the intercooler and radiator are first in line, so a "
                "moderate nose hit leaves it limping on overheat. Four doors means four weak "
                "spots in a T-bone."),
        sources=["manufacturer spec-class data for mid-2000s 2.0 T sport sedans",
                 "owner chassis-dyno averages for 2.0 L turbo I4s on stock boost",
                 "engineering tables (tire radius, ride frequency, caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="aluminum", pushbar=False),
    ),
    # ------------------------------------------------------------------ 3
    dict(
        id="shiro_turbo",
        display_name="Shiro Turbo",
        cls="Sport",
        analog_of="early-2000s Japanese FR coupe, 2.0 L turbocharged I4, clutch LSD",
        era="2000s",
        profile="coupe",
        curb_kg=1250.0,
        fuel_l=65,
        front_frac=0.55,
        wheelbase=2.525,
        track_f=1.48,
        track_r=1.47,
        body=(1.695, 4.445, 1.285),
        clearance=0.13,
        front_overhang=0.9,
        cg=0.46,
        cg_why="low coupe with iron-block four set well back; sports band 0.40-0.46 m",
        inertia_k=(0.80, 0.88, 0.88),
        cd=0.29,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.06, 0.08),
        engine=dict(
            desc="2.0 L DOHC I4, single ball-bearing turbo, intercooled",
            curve=[(1000, 140), (1500, 170), (2000, 205), (2500, 240), (3000, 258), (3500, 266),
                   (4000, 272), (4500, 275), (5000, 276), (5500, 272), (6000, 268), (6500, 268),
                   (7000, 240), (7500, 205)],
            idle=850, redline=7200, limit=7500, inertia=0.13, brake_frac=0.12,
            why="Class-typical early-2000s 2.0 T coupe (~92 kW/L); late, flat torque from a larger turbo, per owner dyno averages.",
        ),
        trans=dict(
            desc="6-speed manual, clutch-type 1.5-way rear LSD",
            layout="Rwd",
            gears=[3.32, 1.90, 1.31, 1.00, 0.76, 0.66], final=4.08, reverse=3.38, shift=0.22,
            diff_f="Open", diff_r=dict(preload=60.0, power=0.45, coast=0.30), clutch_factor=1.45,
        ),
        tires=dict(front="215/45R17", rear="225/45R17", compound="sport", unsprung=(20.0, 22.0)),
        susp=dict(hz=(1.85, 1.95), zeta=0.32, travel=(0.13, 0.14), roll_grad_deg_g=2.0,
                  roll_front_share=0.60, bump_stop=170000.0, preload=0.02),
        align=dict(camber_deg=(-1.0, -1.3), toe_deg=(0.0, 0.2), caster_deg=6.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=296, pistons_mm=[42, 42], pad_mu=0.42),
            rear=dict(kind="disc", d_mm=297, pistons_mm=[38], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.9,
        ),
        steer=dict(ratio=15.0, turn_circle_m=9.3, ackermann=0.5, rack_speed=1.6),
        price=28000,
        durability=2,
        note=("The drift scene's favourite: balanced, light, and with a 1.5-way LSD that locks on "
              "and off throttle so it holds a slide. Not brutally fast, but the most "
              "communicative chassis in the roster and the easiest to throw sideways on purpose."),
        breaks=("Light coupe shell with long doors and thin sills: side impacts reach the cabin "
                "quickly. The front clip is cheap to replace (bumper, hood and fenders bolt on and "
                "come off easily). The rear subframe sits right under the boot, so rear-enders "
                "bend the axle geometry early."),
        sources=["manufacturer spec-class data for early-2000s 2.0 T FR coupes",
                 "owner dyno averages for 2.0 L turbo I4s in that segment",
                 "engineering tables (tire radius, ride frequency, opposed-piston caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="steel", pushbar=False),
    ),
    # ------------------------------------------------------------------ 4
    dict(
        id="hakone_gt",
        display_name="Hakone GT",
        cls="Sport",
        analog_of="90s Japanese FR grand tourer, 3.0 L sequential twin-turbo I6",
        era="90s",
        profile="coupe",
        curb_kg=1550.0,
        fuel_l=70,
        front_frac=0.53,
        wheelbase=2.55,
        track_f=1.52,
        track_r=1.53,
        body=(1.81, 4.515, 1.275),
        clearance=0.13,
        front_overhang=0.95,
        cg=0.46,
        cg_why="low GT coupe, but a tall iron straight-six; top of the sports band",
        inertia_k=(0.80, 0.88, 0.90),
        cd=0.31,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.04, 0.03),
        engine=dict(
            desc="3.0 L DOHC I6, sequential twin turbo, intercooled",
            curve=[(1000, 230), (1500, 280), (2000, 320), (2500, 355), (3000, 390), (3500, 420),
                   (4000, 435), (4500, 432), (5000, 425), (5500, 412), (6000, 385), (6500, 345),
                   (7000, 300)],
            idle=750, redline=6800, limit=7000, inertia=0.18, brake_frac=0.11,
            why="Export-spec power class for 90s 3.0 TT straight-sixes (~80 kW/L); the second-turbo handover near 4000 rpm shows as the torque peak.",
        ),
        trans=dict(
            desc="6-speed manual, clutch-type rear LSD",
            layout="Rwd",
            gears=[3.83, 2.36, 1.69, 1.31, 1.00, 0.79], final=3.13, reverse=3.28, shift=0.25,
            diff_f="Open", diff_r=dict(preload=80.0, power=0.40, coast=0.25), clutch_factor=1.4,
        ),
        tires=dict(front="235/45R17", rear="255/40R17", compound="sport", unsprung=(24.0, 26.0)),
        susp=dict(hz=(1.80, 1.90), zeta=0.30, travel=(0.14, 0.15), roll_grad_deg_g=2.2,
                  roll_front_share=0.62, bump_stop=170000.0, preload=0.02),
        align=dict(camber_deg=(-0.8, -1.3), toe_deg=(0.0, 0.15), caster_deg=6.5),
        brakes=dict(
            front=dict(kind="disc", d_mm=323, pistons_mm=[40, 40], pad_mu=0.42),
            rear=dict(kind="disc", d_mm=324, pistons_mm=[36, 36], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.8,
        ),
        steer=dict(ratio=15.5, turn_circle_m=10.2, ackermann=0.55, rack_speed=1.4),
        price=48000,
        durability=3,
        note=("A heavy, stable highway missile. Off boost below 3000 rpm it feels ordinary; past "
              "4000 the second turbo arrives and it pulls hard to the limiter in every gear. Big "
              "brakes and a long, calm wheelbase make it the escape car of choice on open roads, "
              "less so in tight downtown blocks."),
        breaks=("90s HSS unibody, stiff and heavy: it shrugs off light hits but the long front "
                "overhang puts the intercooler and twin radiators in the firing line. Glass "
                "hatch is large and shatters early. Cabin holds up to its era's standards; the "
                "real cost is parts — everything on it is expensive."),
        sources=["manufacturer spec-class data for 90s 3.0 TT FR GTs (export trim)",
                 "owner dyno averages for sequential-turbo 3.0 I6s on stock boost",
                 "engineering tables (tire radius, ride frequency, opposed-piston caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="aluminum", pushbar=False),
    ),
    # ------------------------------------------------------------------ 5
    dict(
        id="rampart_50",
        display_name="Rampart 5.0",
        cls="Muscle",
        analog_of="2010s American muscle coupe, 5.0 L DOHC V8, independent rear suspension",
        era="2010s",
        profile="coupe",
        curb_kg=1700.0,
        fuel_l=60,
        front_frac=0.54,
        wheelbase=2.72,
        track_f=1.58,
        track_r=1.65,
        body=(1.92, 4.78, 1.38),
        clearance=0.13,
        front_overhang=0.95,
        cg=0.50,
        cg_why="modern muscle coupe: low roof but heavy V8 and tall body sides; mid band",
        inertia_k=(0.80, 0.90, 0.92),
        cd=0.35,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.08, 0.10),
        engine=dict(
            desc="5.0 L DOHC 32-valve V8, naturally aspirated",
            curve=[(1000, 350), (1500, 395), (2000, 430), (2500, 460), (3000, 485), (3500, 505),
                   (4000, 522), (4250, 529), (4750, 525), (5250, 515), (5750, 500), (6250, 482),
                   (6750, 452), (7000, 435), (7400, 395)],
            idle=700, redline=7000, limit=7400, inertia=0.22, brake_frac=0.10,
            why="Class-typical 2010s NA 5.0 V8 (~64 kW/L) with the broad, flat torque curve owner dynos show for modern DOHC V8s.",
        ),
        trans=dict(
            desc="6-speed manual, helical-gear-style rear LSD (modelled as clutch LSD)",
            layout="Rwd",
            gears=[3.66, 2.43, 1.69, 1.32, 1.00, 0.65], final=3.55, reverse=3.53, shift=0.25,
            diff_f="Open", diff_r=dict(preload=50.0, power=0.50, coast=0.35), clutch_factor=1.35,
        ),
        tires=dict(front="255/40R19", rear="275/40R19", compound="sport", unsprung=(26.0, 28.0)),
        susp=dict(hz=(1.55, 1.65), zeta=0.30, travel=(0.16, 0.17), roll_grad_deg_g=2.8,
                  roll_front_share=0.60, bump_stop=170000.0, preload=0.02),
        align=dict(camber_deg=(-0.8, -1.0), toe_deg=(0.0, 0.2), caster_deg=7.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=380, pistons_mm=[46, 40, 36], pad_mu=0.42),
            rear=dict(kind="disc", d_mm=330, pistons_mm=[45], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.8,
        ),
        steer=dict(ratio=15.7, turn_circle_m=10.4, ackermann=0.6, rack_speed=1.4),
        price=42000,
        durability=3,
        note=("Big V8 noise and big straight-line pace for the money. It is traction-limited "
              "off the line and in 2nd, so throttle discipline is the skill; once hooked up it "
              "out-drags everything except the exotic. Heavy nose means it pushes when rushed "
              "into a corner, then snaps loose if you stab the throttle to fix it."),
        breaks=("Modern crash structure: UHSS cabin, long crumple rails, so it absorbs a hard "
                "frontal and the cabin stays intact even in Simulation at moderate speed. That "
                "safety costs the front clip — the big cooling pack and low splitter are toast "
                "after any nose hit. Wide doors dent easily."),
        sources=["manufacturer spec-class data for 2010s 5.0 V8 muscle coupes",
                 "owner dyno averages for modern NA 5.0 DOHC V8s",
                 "engineering tables (tire radius, ride frequency, multi-piston caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="aluminum", pushbar=False),
    ),
    # ------------------------------------------------------------------ 6
    dict(
        id="ironside_440",
        display_name="Ironside 440",
        cls="Muscle",
        analog_of="early-70s American big-block muscle coupe, 7.2 L pushrod V8, live rear axle",
        era="70s",
        profile="coupe",
        curb_kg=1650.0,
        fuel_l=80,
        front_frac=0.56,
        wheelbase=2.95,
        track_f=1.52,
        track_r=1.50,
        body=(1.95, 5.20, 1.35),
        clearance=0.15,
        front_overhang=1.0,
        cg=0.55,
        cg_why="tall cast-iron big-block and a high seating position on a long, soft body; above the sedan band",
        inertia_k=(0.80, 0.95, 0.97),
        cd=0.47,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.15, 0.18),
        engine=dict(
            desc="7.2 L OHV pushrod V8, 4-barrel carburettor (net rating)",
            curve=[(800, 380), (1200, 450), (1600, 500), (2000, 530), (2400, 550), (2800, 558),
                   (3200, 560), (3600, 552), (4000, 515), (4400, 465), (4800, 415), (5200, 365),
                   (5500, 330)],
            idle=650, redline=5200, limit=5500, inertia=0.30, brake_frac=0.12,
            why="Net (installed, with accessories and exhaust) output class for early-70s 7.2 L big-blocks, i.e. well below the period gross ratings; torque peaks low as owner dynos show for long-runner carbureted big-blocks.",
        ),
        trans=dict(
            desc="3-speed automatic with torque converter, open live rear axle",
            layout="Rwd",
            gears=[2.45, 1.45, 1.00], final=3.23, reverse=2.20, shift=0.40,
            torque_converter=dict(stall_rpm=2200, ratio=2.1, coupling_rpm=2800),
            diff_f="Open", diff_r="Open", clutch_factor=1.5,
        ),
        tires=dict(front="235/70R15", rear="235/70R15", compound="vintage_bias", unsprung=(30.0, 45.0)),
        susp=dict(hz=(1.10, 1.20), zeta=0.22, travel=(0.20, 0.22), roll_grad_deg_g=6.0,
                  roll_front_share=0.70, bump_stop=120000.0, preload=0.03),
        align=dict(camber_deg=(0.25, 0.0), toe_deg=(0.1, 0.0), caster_deg=2.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=279, pistons_mm=[70], pad_mu=0.30),
            rear=dict(kind="drum", d_mm=279, wheel_cyl_mm=22.2, brake_factor=1.8),
            bar=P_70S, handbrake_frac=0.8,
        ),
        steer=dict(ratio=19.0, turn_circle_m=12.8, ackermann=0.8, rack_speed=1.0),
        price=38000,
        durability=4,
        note=("Two tonnes of attitude (well, 1.7). Enormous low-down torque through a lazy "
              "three-speed, vague slow steering, soft springs that let it wallow and pitch, and "
              "brakes that are frankly scary: single-piston front discs with organic pads and "
              "rear drums cannot lock the tires on dry tarmac, so stopping distances are long "
              "and get longer. Point it, plant it, and plan your stops early."),
        breaks=("Thick-gauge mild-steel panels and chrome steel bumpers dent for days without "
                "falling off — light hits barely cost anything. But there is no crumple zone to "
                "speak of and the cabin pillars are mild steel, so a hard frontal goes straight "
                "into the occupants (Simulation lethality is high). The live rear axle hangs on "
                "leaf-spring hangers that tear out of the floor in a rear or rear-quarter hit, "
                "taking a wheel with them. The period fuel tank sits behind the axle under the "
                "trunk floor, so a hard rear-ender is a fire risk."),
        sources=["manufacturer spec-class data for early-70s big-block intermediates (mass, dimensions, axle ratios)",
                 "period net-rating class and modern owner dyno averages for 7.2 L big-blocks",
                 "engineering tables (bias-ply-equivalent tire radius, drum brake factor, ride frequency)"],
        lattice=dict(mass_frac=0.24, bumper="steel", hood="steel", pushbar=False),
    ),
    # ------------------------------------------------------------------ 7
    dict(
        id="sperber_32",
        display_name="Sperber 32",
        cls="Sport",
        analog_of="2000s German FR sport sedan, 3.2 L high-revving naturally aspirated I6",
        era="2000s",
        profile="sedan",
        curb_kg=1550.0,
        fuel_l=63,
        front_frac=0.51,
        wheelbase=2.73,
        track_f=1.51,
        track_r=1.52,
        body=(1.78, 4.49, 1.37),
        clearance=0.13,
        front_overhang=0.85,
        cg=0.47,
        cg_why="low sport sedan with the six set behind the front axle; lower edge of the sedan band",
        inertia_k=(0.80, 0.88, 0.90),
        cd=0.32,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.05, 0.06),
        engine=dict(
            desc="3.2 L DOHC I6, naturally aspirated, individual throttle bodies, 8000 rpm",
            curve=[(1000, 215), (1500, 245), (2000, 275), (3000, 322), (3500, 338), (4000, 350),
                   (4500, 360), (4900, 365), (5500, 358), (6000, 348), (6500, 335), (7000, 325),
                   (7500, 312), (7900, 300), (8100, 280)],
            idle=800, redline=7900, limit=8100, inertia=0.15, brake_frac=0.10,
            why="Class-typical 2000s high-specific-output NA six (~78 kW/L) with a high, flat top end per owner dyno averages.",
        ),
        trans=dict(
            desc="6-speed manual, variable-locking clutch rear LSD",
            layout="Rwd",
            gears=[4.23, 2.53, 1.67, 1.23, 1.00, 0.83], final=3.62, reverse=3.75, shift=0.22,
            diff_f="Open", diff_r=dict(preload=40.0, power=0.50, coast=0.40), clutch_factor=1.4,
        ),
        tires=dict(front="225/45R18", rear="255/40R18", compound="sport", unsprung=(23.0, 25.0)),
        susp=dict(hz=(1.80, 1.95), zeta=0.32, travel=(0.14, 0.15), roll_grad_deg_g=2.2,
                  roll_front_share=0.58, bump_stop=170000.0, preload=0.02),
        align=dict(camber_deg=(-1.2, -1.5), toe_deg=(0.0, 0.2), caster_deg=7.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=325, pistons_mm=[60], pad_mu=0.42),
            rear=dict(kind="disc", d_mm=328, pistons_mm=[42], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.8,
        ),
        steer=dict(ratio=15.4, turn_circle_m=10.3, ackermann=0.55, rack_speed=1.5),
        price=36000,
        durability=3,
        note=("The precision instrument: near 50:50 balance, a howling 8000 rpm six and a diff "
              "that lets you steer on the throttle. It needs revs to be fast and punishes "
              "lazy gear choices, but on a twisty route nothing in its price bracket is "
              "quicker. Four doors make it a sensible-looking getaway car."),
        breaks=("Solid 2000s HSS shell with good crumple rails; it handles a frontal well. The "
                "long nose holds a big radiator and the six's sump hangs low, so kerb strikes and "
                "nose hits hurt the engine sooner than you'd expect. Doors are steel and heavy, "
                "decent in side hits."),
        sources=["manufacturer spec-class data for 2000s 3.2 L I6 sport sedans",
                 "owner dyno averages for high-rev NA 3.2 I6s",
                 "engineering tables (tire radius, ride frequency, caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="steel", pushbar=False),
    ),
    # ------------------------------------------------------------------ 8
    dict(
        id="lindwurm_4t",
        display_name="Lindwurm 4T",
        cls="Sport",
        analog_of="2010s German AWD hot hatch, 2.0 L turbocharged I4, on-demand clutch-pack AWD, dual-clutch gearbox",
        era="2010s",
        profile="hatch",
        curb_kg=1500.0,
        fuel_l=55,
        front_frac=0.60,
        wheelbase=2.63,
        track_f=1.535,
        track_r=1.51,
        body=(1.80, 4.28, 1.44),
        clearance=0.13,
        front_overhang=0.9,
        cg=0.52,
        cg_why="tall-ish hatch with transverse engine and AWD hardware; sedan band upper end",
        inertia_k=(0.80, 0.90, 0.92),
        cd=0.33,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.06, 0.04),
        engine=dict(
            desc="2.0 L DOHC I4, single turbo, direct injection, plateau 1800-5500 rpm",
            curve=[(1000, 200), (1500, 300), (1800, 380), (2500, 380), (3000, 380), (3500, 380),
                   (4000, 380), (4500, 380), (5000, 380), (5500, 380), (6000, 350), (6500, 310),
                   (6700, 290)],
            idle=750, redline=6500, limit=6700, inertia=0.12, brake_frac=0.10,
            why="Class-typical 2010s 2.0 T (~110 kW/L) with the electronically flattened torque plateau owner dynos show.",
        ),
        trans=dict(
            desc="6-speed dual-clutch, on-demand clutch-pack rear coupling (95 % front until the fronts slip 5 %, then up to 50 % rear), open diffs with brake-based torque vectoring (assist)",
            layout=("AwdOnDemand", 0.95, 0.50, 0.05),
            gears=[3.46, 2.15, 1.45, 1.10, 0.87, 0.71], final=3.65, reverse=3.00, shift=0.08,
            diff_f="Open", diff_r="Open", clutch_factor=1.4,
        ),
        tires=dict(front="225/40R18", rear="225/40R18", compound="sport", unsprung=(22.0, 22.0)),
        susp=dict(hz=(1.80, 1.90), zeta=0.30, travel=(0.15, 0.16), roll_grad_deg_g=2.2,
                  roll_front_share=0.55, bump_stop=170000.0, preload=0.02),
        align=dict(camber_deg=(-0.8, -1.3), toe_deg=(0.0, 0.2), caster_deg=7.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=340, pistons_mm=[60], pad_mu=0.42),
            rear=dict(kind="disc", d_mm=310, pistons_mm=[41], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.8,
        ),
        steer=dict(ratio=13.6, turn_circle_m=10.9, ackermann=0.6, rack_speed=1.7),
        price=34000,
        durability=3,
        note=("Point-and-squirt in any weather. Instant dual-clutch shifts and AWD launches make "
              "it brutally quick to 100 km/h for its power, and it is the most forgiving fast "
              "car in the roster. The price is a front-heavy chassis that understeers when "
              "pushed and a ceiling the rear-drive cars break through on open roads."),
        breaks=("Modern UHSS cell and long crumple rails make it one of the safest cabins on the "
                "list; frontals crush the nose but rarely the occupants. The rear coupling and "
                "prop shaft run under the floor, so heavy kerb or sill strikes can kill drive to "
                "the rear. Plastic everywhere outside the cell: cheap to bend, cheap to fix."),
        sources=["manufacturer spec-class data for 2010s AWD 2.0 T hot hatches",
                 "owner dyno averages for 2010s 2.0 T direct-injection I4s",
                 "engineering tables (tire radius, ride frequency, caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="steel", pushbar=False),
    ),
    # ------------------------------------------------------------------ 9
    dict(
        id="lucciola_350",
        display_name="Lucciola 350",
        cls="Exotic",
        analog_of="90s Italian mid-engine exotic coupe, 3.5 L flat-plane V8, transverse-mounted gearbox behind the engine",
        era="90s",
        profile="mr",
        curb_kg=1300.0,
        fuel_l=82,
        front_frac=0.42,
        wheelbase=2.45,
        track_f=1.51,
        track_r=1.615,
        body=(1.90, 4.25, 1.17),
        clearance=0.11,
        front_overhang=0.85,
        cg=0.41,
        cg_why="mid-engine, very low roof and dry-sumped V8 low between the axles; bottom of the sports/exotic band",
        inertia_k=(0.78, 0.75, 0.75),
        cd=0.33,
        area_k=0.82,
        extra_cda=0.0,
        lift=(-0.05, -0.12),
        engine=dict(
            desc="3.5 L DOHC 40-valve flat-plane V8, naturally aspirated, dry sump",
            curve=[(1000, 180), (2000, 240), (3000, 285), (3500, 305), (4000, 320), (4500, 335),
                   (5000, 348), (5500, 358), (6000, 363), (6500, 360), (7000, 352), (7500, 343),
                   (8000, 330), (8250, 322), (8800, 280)],
            idle=1000, redline=8500, limit=8800, inertia=0.12, brake_frac=0.10,
            why="Class-typical 90s exotic flat-plane V8 (~80 kW/L), torque peaking high and power at 8250 rpm, per published spec-class data and owner dynos.",
        ),
        trans=dict(
            desc="6-speed manual (open gate), clutch-type rear LSD",
            layout="Rwd",
            gears=[3.08, 2.16, 1.61, 1.27, 1.03, 0.84], final=4.30, reverse=2.92, shift=0.22,
            diff_f="Open", diff_r=dict(preload=60.0, power=0.50, coast=0.35), clutch_factor=1.4,
        ),
        tires=dict(front="225/40R18", rear="265/40R18", compound="sport", unsprung=(20.0, 24.0)),
        susp=dict(hz=(2.00, 2.15), zeta=0.32, travel=(0.12, 0.13), roll_grad_deg_g=1.6,
                  roll_front_share=0.50, bump_stop=200000.0, preload=0.02),
        align=dict(camber_deg=(-1.0, -1.8), toe_deg=(0.0, 0.25), caster_deg=7.5),
        brakes=dict(
            front=dict(kind="disc", d_mm=330, pistons_mm=[40, 40], pad_mu=0.45),
            rear=dict(kind="disc", d_mm=310, pistons_mm=[36, 36], pad_mu=0.42),
            bar=P_MODERN, handbrake_frac=0.6,
        ),
        steer=dict(ratio=16.0, turn_circle_m=10.5, ackermann=0.5, rack_speed=1.6),
        price=120000,
        durability=1,
        note=("The poster car. Rear-biased mid-engine weight, a screaming 8500 rpm V8 and the "
              "most grip in the roster make it the fastest thing through a corner, but low yaw "
              "inertia means it rotates instantly — lift mid-corner and the rear comes round. "
              "Glass-cannon economics: winning in it pays well, crashing it is ruinous."),
        breaks=("Everything behind the cabin is sacrificial: the aluminium rear subframe carries "
                "the engine, gearbox and both rear suspension corners, so any rear or "
                "rear-quarter hit kills drive, and the side fuel tanks behind the seats make "
                "T-bones dangerous. The short front has almost no crumple length. Aluminium "
                "panels tear rather than dent. Durability class 1."),
        sources=["manufacturer spec-class data for 90s mid-engine 3.5 L V8 exotics",
                 "published dyno sheets for flat-plane 3.5 V8s",
                 "engineering tables (tire radius, ride frequency, opposed-piston caliper hydraulics)"],
        lattice=dict(mass_frac=0.20, bumper="plastic", hood="aluminum", pushbar=False),
    ),
    # ------------------------------------------------------------------ 10
    dict(
        id="hauler_1500",
        display_name="Hauler 1500",
        cls="Truck",
        analog_of="2010s American full-size half-ton crew-cab pickup, 5.3 L pushrod V8, body-on-frame, solid rear axle",
        era="2010s",
        profile="truck",
        curb_kg=2400.0,
        fuel_l=98,
        front_frac=0.57,
        wheelbase=3.645,
        track_f=1.745,
        track_r=1.71,
        body=(2.03, 5.85, 1.88),
        clearance=0.22,
        front_overhang=0.95,
        cg=0.72,
        cg_why="body-on-frame pickup with an empty bed; truck/SUV band 0.65-0.75 m",
        inertia_k=(0.85, 0.95, 0.97),
        cd=0.44,
        area_k=0.82,
        extra_cda=0.0,
        lift=(0.20, 0.15),
        engine=dict(
            desc="5.3 L OHV pushrod V8, naturally aspirated, cylinder deactivation (ignored)",
            curve=[(800, 360), (1200, 400), (1600, 440), (2000, 465), (2500, 485), (3000, 500),
                   (3500, 512), (4100, 519), (4500, 510), (5000, 485), (5600, 452), (6000, 410)],
            idle=600, redline=5800, limit=6000, inertia=0.25, brake_frac=0.12,
            why="Class-typical 2010s 5.3 L truck V8 (~50 kW/L), torque-rich low end per owner dyno averages.",
        ),
        trans=dict(
            desc="6-speed automatic with lock-up torque converter, automatic-locking rear diff modelled as high-preload LSD; 4WD transfer case is a mod",
            layout="Rwd",
            gears=[4.03, 2.36, 1.53, 1.15, 0.85, 0.67], final=3.42, reverse=3.06, shift=0.30,
            torque_converter=dict(stall_rpm=2200, ratio=2.0, coupling_rpm=2600),
            diff_f="Open", diff_r=dict(preload=150.0, power=0.60, coast=0.20), clutch_factor=1.6,
        ),
        tires=dict(front="265/70R17", rear="265/70R17", compound="all_terrain", unsprung=(40.0, 55.0)),
        susp=dict(hz=(1.20, 1.45), zeta=0.28, travel=(0.24, 0.26), roll_grad_deg_g=4.5,
                  roll_front_share=0.75, bump_stop=300000.0, preload=0.03),
        align=dict(camber_deg=(-0.2, 0.0), toe_deg=(0.1, 0.0), caster_deg=4.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=330, pistons_mm=[54, 54], pad_mu=0.40),
            rear=dict(kind="disc", d_mm=345, pistons_mm=[54], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.6,
        ),
        steer=dict(ratio=17.5, turn_circle_m=14.0, ackermann=0.7, rack_speed=1.2),
        price=32000,
        durability=5,
        note=("The battering ram. Slow to turn, slow to stop and top-heavy enough to lift a "
              "wheel in a hard swerve, but it accelerates harder than it has any right to and "
              "wins every shoving match. Police block it at their peril; in a pursuit it is the "
              "car that clears the roadblock for everyone else."),
        breaks=("Body-on-frame: two boxed steel frame rails run the full length and take the hits, "
                "so bumpers, grille and bed panels get mangled while the frame and cab stay "
                "straight. The steel bumpers soak low-speed hits for free. Weak spots are the "
                "side-saddle fuel tank between the axles (side impacts) and the tall cab, which "
                "is not reinforced like a modern car's — rollovers and hits above frame height "
                "go straight into the cab."),
        sources=["manufacturer spec-class data for 2010s half-ton crew-cab pickups",
                 "owner dyno averages for 5.3 L truck V8s",
                 "engineering tables (truck tire radius, leaf/coil ride frequency, caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="steel", hood="steel", pushbar=False, ladder_frame=True),
    ),
    # ------------------------------------------------------------------ 11
    dict(
        id="sentinel_cruiser",
        display_name="Sentinel Cruiser",
        cls="PoliceCruiser",
        analog_of="2010s American full-size RWD police sedan, 5.7 L pushrod V8, pursuit package",
        era="2010s",
        profile="sedan",
        curb_kg=2000.0,
        fuel_l=72,
        front_frac=0.55,
        wheelbase=3.05,
        track_f=1.60,
        track_r=1.62,
        body=(1.91, 5.08, 1.48),
        clearance=0.14,
        front_overhang=1.0,
        cg=0.53,
        cg_why="full-size sedan plus lightbar, partition and trunk equipment raising the CG; top of the sedan band",
        inertia_k=(0.80, 0.92, 0.94),
        cd=0.34,
        area_k=0.84,
        extra_cda=0.05,
        lift=(0.10, 0.12),
        engine=dict(
            desc="5.7 L OHV 16-valve V8, naturally aspirated, pursuit calibration (no cylinder deactivation)",
            curve=[(800, 400), (1200, 440), (1600, 470), (2000, 490), (2500, 505), (3000, 515),
                   (3500, 525), (4000, 533), (4400, 535), (4800, 528), (5200, 505), (5600, 470),
                   (5900, 440)],
            idle=600, redline=5800, limit=5900, inertia=0.24, brake_frac=0.10,
            why=("Class-typical 2010s 5.7 L pushrod police V8 (~48 kW/L, 276 kW at 5200 rpm, "
                 "~535 N·m at 4400) per fleet spec-class data and owner dynos. Pursuit tuning "
                 "(round 6b): the V6 cruiser could not catch the traffic sedan to the 144 km/h "
                 "pursuit cap; the V8 package is the period-correct fix."),
        ),
        trans=dict(
            desc="5-speed automatic with torque converter, mild rear LSD (pursuit package, V8 final drive)",
            layout="Rwd",
            gears=[3.59, 2.19, 1.41, 1.00, 0.83], final=3.07, reverse=3.16, shift=0.30,
            torque_converter=dict(stall_rpm=2300, ratio=2.0, coupling_rpm=2700),
            diff_f="Open", diff_r=dict(preload=40.0, power=0.30, coast=0.15), clutch_factor=1.6,
        ),
        tires=dict(front="225/60R18", rear="225/60R18", compound="police_pursuit", unsprung=(26.0, 28.0)),
        susp=dict(hz=(1.45, 1.55), zeta=0.32, travel=(0.17, 0.18), roll_grad_deg_g=3.2,
                  roll_front_share=0.57, bump_stop=190000.0, preload=0.02),
        align=dict(camber_deg=(-1.0, -0.8), toe_deg=(0.0, 0.15), caster_deg=6.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=350, pistons_mm=[54, 54], pad_mu=0.42),
            rear=dict(kind="disc", d_mm=345, pistons_mm=[44], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.8,
        ),
        steer=dict(ratio=16.5, turn_circle_m=11.9, ackermann=0.6, rack_speed=1.3),
        price=30000,
        durability=5,
        note=("The everyday patrol car: rear-drive, heavy and planted, with pursuit-rated tires, "
              "big brakes and the V8 pursuit package. It gets to pursuit speed before anything "
              "ordinary (traffic, beaters, hatches, trucks, old muscle, FF sport sedans) but "
              "not before a modern sports car or exotic — those are the interceptor's job. "
              "Boxing in, PIT manoeuvres and the push bar do the rest."),
        breaks=("Modern full-size unibody with UHSS cabin, plus a reinforced push bar bolted "
                "through to the front rails: nose-to-tail shunts at pursuit speed mostly bend the "
                "bar and the bumper cover before the radiator. Hard enough hits rip the bar off, "
                "after which the front is as soft as any sedan's. Long rear overhang crumples in "
                "rear-enders."),
        sources=["fleet spec-class data for 2010s full-size RWD police sedans",
                 "owner/fleet dyno averages for 5.7 L pushrod V8s",
                 "engineering tables (tire radius, ride frequency, caliper hydraulics)"],
        lattice=dict(mass_frac=0.25, bumper="plastic", hood="steel", pushbar=True,
                     police_package=dict(
                         pushbar_kg=40.0, pushbar_impulse_ns=30000.0, pushbar_section_scale=1.5,
                         bumper_impulse_scale=2.0,
                         hd_components=dict(Radiator=1.5, SteeringRack=1.5, SuspensionFL=1.5,
                                            SuspensionFR=1.5, WheelFL=1.4, WheelFR=1.4,
                                            BrakeFL=1.3, BrakeFR=1.3, Engine=1.2, Battery=1.3))),
    ),
    # ------------------------------------------------------------------ 12
    dict(
        id="sentinel_wolfhound",
        display_name="Sentinel Wolfhound",
        cls="PoliceInterceptor",
        analog_of="2010s American AWD police pursuit sedan, 3.5 L twin-turbo V6",
        era="2010s",
        profile="sedan",
        curb_kg=2000.0,
        fuel_l=70,
        front_frac=0.56,
        wheelbase=2.87,
        track_f=1.66,
        track_r=1.66,
        body=(1.94, 5.05, 1.55),
        clearance=0.15,
        front_overhang=1.0,
        cg=0.56,
        cg_why="tall full-size AWD sedan with police equipment; above the sedan band",
        inertia_k=(0.80, 0.92, 0.94),
        cd=0.33,
        area_k=0.84,
        extra_cda=0.05,
        lift=(0.08, 0.10),
        engine=dict(
            desc="3.5 L DOHC V6, twin turbo, direct injection",
            curve=[(800, 260), (1200, 330), (1600, 400), (2000, 455), (2500, 495), (3000, 495),
                   (3500, 495), (4000, 495), (4500, 490), (5000, 490), (5500, 472), (6000, 420),
                   (6300, 380)],
            idle=650, redline=6000, limit=6300, inertia=0.18, brake_frac=0.10,
            why="Class-typical 2010s 3.5 TT V6 (~78 kW/L), boost plateau from 2500 rpm per fleet spec-class data and owner dynos.",
        ),
        trans=dict(
            desc="6-speed automatic with torque converter, on-demand AWD tuned for launches (90 % front until the fronts slip 2 %, then up to 50 % rear), open diffs",
            layout=("AwdOnDemand", 0.90, 0.50, 0.02),
            gears=[4.48, 2.87, 1.84, 1.41, 1.00, 0.74], final=3.16, reverse=2.88, shift=0.30,
            torque_converter=dict(stall_rpm=2600, ratio=1.9, coupling_rpm=3000),
            diff_f="Open", diff_r="Open", clutch_factor=1.6,
        ),
        tires=dict(front="245/55R18", rear="245/55R18", compound="police_pursuit", unsprung=(28.0, 28.0)),
        susp=dict(hz=(1.50, 1.60), zeta=0.30, travel=(0.17, 0.18), roll_grad_deg_g=3.0,
                  roll_front_share=0.60, bump_stop=170000.0, preload=0.02),
        align=dict(camber_deg=(-0.6, -0.9), toe_deg=(0.0, 0.15), caster_deg=5.5),
        brakes=dict(
            front=dict(kind="disc", d_mm=350, pistons_mm=[54, 54], pad_mu=0.42),
            rear=dict(kind="disc", d_mm=345, pistons_mm=[44], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.8,
        ),
        steer=dict(ratio=15.5, turn_circle_m=12.2, ackermann=0.6, rack_speed=1.4),
        price=45000,
        durability=4,
        note=("The pursuit specialist. AWD and a twin-turbo V6 give it launch traction and "
              "mid-range punch to stay on a sports car's bumper in the city, and it is stable at "
              "speed. Heavy and tall, so it cannot match a real sports car through fast corners "
              "— it wins by never letting the gap open."),
        breaks=("Same reinforced police front as the cruiser (push bar through to the rails) on a "
                "modern UHSS cell. AWD hardware under the floor means sill and centre-tunnel hits "
                "can take out drive to one axle. Heavier than it looks: it hurts what it hits."),
        sources=["fleet spec-class data for 2010s AWD police pursuit vehicles",
                 "owner dyno averages for 3.5 L twin-turbo V6s",
                 "engineering tables (tire radius, ride frequency, caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="steel", pushbar=True),
    ),
    # ------------------------------------------------------------------ 13
    dict(
        id="sentinel_bastion",
        display_name="Sentinel Bastion",
        cls="PoliceSuv",
        analog_of="2010s American full-size police SUV, 5.3 L pushrod V8, body-on-frame, RWD",
        era="2010s",
        profile="suv",
        curb_kg=2600.0,
        fuel_l=98,
        front_frac=0.53,
        wheelbase=2.95,
        track_f=1.73,
        track_r=1.72,
        body=(2.04, 5.18, 1.89),
        clearance=0.22,
        front_overhang=0.95,
        cg=0.74,
        cg_why="body-on-frame full-size SUV with roof-mounted lightbar; top of the truck/SUV band",
        inertia_k=(0.85, 0.95, 0.97),
        cd=0.42,
        area_k=0.82,
        extra_cda=0.05,
        lift=(0.20, 0.15),
        engine=dict(
            desc="5.3 L OHV pushrod V8, naturally aspirated, police calibration",
            curve=[(800, 360), (1200, 400), (1600, 440), (2000, 465), (2500, 485), (3000, 500),
                   (3500, 512), (4100, 519), (4500, 510), (5000, 485), (5600, 452), (6000, 410)],
            idle=600, redline=5800, limit=6000, inertia=0.25, brake_frac=0.12,
            why="Same class engine as the Hauler (2010s 5.3 L truck V8).",
        ),
        trans=dict(
            desc="6-speed automatic with torque converter, open rear diff",
            layout="Rwd",
            gears=[4.03, 2.36, 1.53, 1.15, 0.85, 0.67], final=3.08, reverse=3.06, shift=0.30,
            torque_converter=dict(stall_rpm=2200, ratio=2.0, coupling_rpm=2600),
            diff_f="Open", diff_r="Open", clutch_factor=1.6,
        ),
        tires=dict(front="265/60R17", rear="265/60R17", compound="police_pursuit", unsprung=(36.0, 50.0)),
        susp=dict(hz=(1.25, 1.35), zeta=0.28, travel=(0.22, 0.24), roll_grad_deg_g=4.5,
                  roll_front_share=0.70, bump_stop=300000.0, preload=0.03),
        align=dict(camber_deg=(-0.3, 0.0), toe_deg=(0.1, 0.0), caster_deg=4.5),
        brakes=dict(
            front=dict(kind="disc", d_mm=345, pistons_mm=[54, 54], pad_mu=0.42),
            rear=dict(kind="disc", d_mm=345, pistons_mm=[54], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.6,
        ),
        steer=dict(ratio=17.0, turn_circle_m=12.6, ackermann=0.7, rack_speed=1.2),
        price=48000,
        durability=5,
        note=("The roadblock on wheels. Slowest of the police units and it leans like a ship in "
              "a hard turn, but it is heavy, tall and nearly unstoppable in a shove. Captains "
              "park it across junctions; in a pursuit it ends things by sheer mass."),
        breaks=("Body-on-frame with a full-length ladder frame and a reinforced push bar: frontal "
                "and rear shunts bend bodywork long before anything structural. The high CG "
                "means hard side hits at the wheels can trip it. Large glass area shatters "
                "early; the solid rear axle is robust."),
        sources=["fleet spec-class data for 2010s full-size police SUVs",
                 "owner dyno averages for 5.3 L truck V8s",
                 "engineering tables (truck tire radius, ride frequency, caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="steel", pushbar=True, ladder_frame=True,
                     reinforce_bumper_beam=True),
    ),

    # ------------------------------------------------------------------ 14 (economy floor)
    dict(
        id="junkyard_beater",
        display_name="Junkyard Beater",
        cls="Beater",
        analog_of="1980s compact FF sedan, 1.6 L SOHC I4, 4-speed manual, high mileage (tired engine, sagging springs, worn dampers, old tyres)",
        era="80s",
        profile="sedan",
        curb_kg=990.0,
        fuel_l=45,
        front_frac=0.60,
        wheelbase=2.50,
        track_f=1.43,
        track_r=1.42,
        body=(1.66, 4.30, 1.38),
        clearance=0.14,
        front_overhang=0.90,
        cg=0.50,
        cg_why="small upright sedan; sedan band 0.45-0.52 m, upper end for the tall cabin",
        inertia_k=(0.80, 0.90, 0.92),
        cd=0.38,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.12, 0.14),
        engine=dict(
            desc="1.6 L SOHC 8-valve I4, twin-choke carburettor, worn (≈ 10 % down on its new rating)",
            curve=[(800, 82), (1200, 94), (1600, 103), (2000, 110), (2500, 116), (3000, 120),
                   (3500, 123), (4000, 124), (4500, 123), (5000, 121), (5500, 113), (5800, 104),
                   (6000, 96)],
            idle=850, redline=5800, limit=6000, inertia=0.12, brake_frac=0.12,
            why="Class-typical late-80s 1.6 SOHC economy engine (~45 kW/L new), derated ~10 % for wear (compression loss) per owner dyno sheets of high-mileage examples.",
        ),
        trans=dict(
            desc="4-speed manual with a tired second-gear synchro (slow shifts), open front differential",
            layout="Fwd",
            gears=[3.17, 1.84, 1.26, 0.97], final=4.33, reverse=3.17, shift=0.55,
            efficiency=0.78,
            efficiency_why=("worn CV joints, tired wheel bearings and dragging front calipers; "
                            "class-typical 0.85 minus ≈ 8 % for high mileage"),
            diff_f="Open", diff_r="Open", clutch_factor=1.3,
        ),
        tires=dict(front="175/70R13", rear="175/70R13", compound="vintage_bias", unsprung=(15.0, 14.0)),
        susp=dict(hz=(1.15, 1.25), zeta=0.14, travel=(0.16, 0.17), roll_grad_deg_g=6.5,
                  roll_front_share=0.60, bump_stop=120000.0, preload=0.01),
        align=dict(camber_deg=(-1.0, -0.5), toe_deg=(0.0, 0.0), caster_deg=2.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=240, pistons_mm=[51], pad_mu=0.35),
            rear=dict(kind="drum", d_mm=200, wheel_cyl_mm=17.5, brake_factor=1.8),
            bar=P_MODERN, handbrake_frac=1.0,
        ),
        steer=dict(ratio=17.5, turn_circle_m=9.8, ackermann=0.7, rack_speed=1.1),
        price=3000,
        durability=2,
        note=("The economy floor: always affordable, never fast. Sagging springs and worn "
              "dampers make it lean hard and bob over every crest, the old hard tyres squeal "
              "long before they let go, and the tired four needs every one of its 5800 rpm "
              "through a long-throw 4-speed with a slow shift. Flat out it buzzes on the limiter "
              "at about 150 km/h. It is what you drive after the impound took your real car — "
              "slow, but it tells you exactly what the weight is doing, so it is a surprisingly "
              "good teacher."),
        breaks=("A thin late-80s shell with a weak passenger cell: modest crumple rails and "
                "light pillars mean a hard frontal reaches the cabin (Simulation lethality is "
                "high), but panels are cheap and everything bolts back on. Small radiator right "
                "behind a plastic bumper — nose taps overheat it quickly."),
        sources=["manufacturer spec-class data for 1980s 1.6 L FF compact sedans (4-speed manual / 3-speed automatic trims)",
                 "owner dyno sheets for high-mileage 1.6 SOHC carburettor engines",
                 "tyre test data for aged (hardened) radials: ≈ 0.85 × new street grip, earlier and louder breakaway",
                 "engineering tables (tire radius, ride frequency, disc/drum brake hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="steel", pushbar=False),
    ),
    # ------------------------------------------------------------------ 15 (traffic)
    dict(
        id="tansy_13",
        display_name="Tansy 1.3",
        cls="Compact",
        analog_of="2000s European/Japanese FF supermini hatch, 1.3 L DOHC I4, five-door",
        era="2000s",
        profile="hatch",
        curb_kg=1000.0,
        fuel_l=45,
        front_frac=0.62,
        wheelbase=2.46,
        track_f=1.44,
        track_r=1.43,
        body=(1.67, 3.85, 1.50),
        clearance=0.14,
        front_overhang=0.75,
        cg=0.52,
        cg_why="tall-roofed supermini (1.50 m) with a short wheelbase; top of the 0.45-0.52 m car band",
        inertia_k=(0.80, 0.90, 0.92),
        cd=0.33,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.10, 0.12),
        engine=dict(
            desc="1.3 L DOHC 16-valve I4, port injection",
            curve=[(800, 80), (1200, 92), (1600, 100), (2000, 106), (2500, 112), (3000, 116),
                   (3500, 119), (4000, 121), (4500, 120), (5000, 117), (5500, 112), (6000, 105),
                   (6400, 95)],
            idle=750, redline=6200, limit=6500, inertia=0.10, brake_frac=0.12,
            why="Class-typical 2000s 1.3 L supermini engine (~50 kW/L, 66 kW at 6000 rpm, ~121 N·m at 4000) per manufacturer spec-class data.",
        ),
        trans=dict(
            desc="5-speed manual, open front differential",
            layout="Fwd",
            gears=[3.55, 1.90, 1.28, 0.97, 0.78], final=4.06, reverse=3.31, shift=0.25,
            diff_f="Open", diff_r="Open", clutch_factor=1.35,
        ),
        tires=dict(front="175/65R14", rear="175/65R14", compound="street", unsprung=(15.0, 14.0)),
        susp=dict(hz=(1.35, 1.50), zeta=0.27, travel=(0.15, 0.16), roll_grad_deg_g=5.5,
                  roll_front_share=0.62, bump_stop=120000.0, preload=0.01),
        align=dict(camber_deg=(-0.5, -1.0), toe_deg=(0.0, 0.15), caster_deg=3.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=256, pistons_mm=[54], pad_mu=0.38),
            rear=dict(kind="drum", d_mm=203, wheel_cyl_mm=17.5, brake_factor=1.8),
            bar=P_MODERN, handbrake_frac=1.0,
        ),
        steer=dict(ratio=16.0, turn_circle_m=10.0, ackermann=0.7, rack_speed=1.1),
        price=7000,
        durability=2,
        note=("The city's most common car: a five-door supermini that fills traffic queues and "
              "parking bays. Light, upright and lazy above 100 km/h, it leans hard, understeers "
              "safely and is easy to shove aside — a moving obstacle more than a rival."),
        breaks=("A modern small-car shell: short crumple zones and a decent HSS cell for its "
                "size, but there is little car in front of the cabin, so frontals reach the "
                "firewall early and a heavier car's nose goes deep into the doors. Plastic "
                "bumpers and thin panels crumple at parking-lot speeds."),
        sources=["manufacturer spec-class data for 2000s 1.3 L FF superminis",
                 "road-test figures for the class (0-100, braking, skidpad)",
                 "engineering tables (tire radius, ride frequency, disc/drum brake hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="plastic", hood="steel", pushbar=False),
    ),
    # ------------------------------------------------------------------ 16 (traffic)
    dict(
        id="drayman_cargo",
        display_name="Drayman Cargo",
        cls="Truck",
        analog_of="2000s American full-size RWD cargo van, 4.8 L OHV V8, body-on-frame, solid rear axle",
        era="2000s",
        profile="van",
        curb_kg=2450.0,
        fuel_l=117,
        front_frac=0.56,
        wheelbase=3.43,
        track_f=1.73,
        track_r=1.72,
        body=(2.01, 5.69, 2.08),
        clearance=0.20,
        front_overhang=0.90,
        cg=0.78,
        cg_why="empty full-size van: tall body-on-frame box (2.08 m) with the mass low on the frame; just above the 0.65-0.75 m truck/SUV band",
        inertia_k=(0.85, 0.95, 0.97),
        cd=0.38,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.15, 0.12),
        engine=dict(
            desc="4.8 L OHV 16-valve V8, naturally aspirated, truck calibration",
            curve=[(800, 300), (1200, 330), (1600, 350), (2000, 365), (2500, 378), (3000, 388),
                   (3500, 395), (4000, 399), (4400, 400), (4800, 393), (5200, 380), (5600, 350),
                   (5900, 320)],
            idle=600, redline=5600, limit=5900, inertia=0.24, brake_frac=0.12,
            why="Class-typical 2000s 4.8 L van/truck V8 (~43 kW/L, ~207 kW at 5200 rpm, ~400 N·m at 4400) per manufacturer spec-class data.",
        ),
        trans=dict(
            desc="4-speed automatic with lock-up torque converter, open rear differential",
            layout="Rwd",
            gears=[3.06, 1.63, 1.00, 0.70], final=3.42, reverse=2.29, shift=0.35,
            torque_converter=dict(stall_rpm=2000, ratio=2.0, coupling_rpm=2400),
            diff_f="Open", diff_r="Open", clutch_factor=1.6,
        ),
        tires=dict(front="245/75R16", rear="245/75R16", compound="street", unsprung=(40.0, 60.0)),
        susp=dict(hz=(1.25, 1.55), zeta=0.27, travel=(0.20, 0.22), roll_grad_deg_g=5.5,
                  roll_front_share=0.70, bump_stop=300000.0, preload=0.03),
        align=dict(camber_deg=(0.0, 0.0), toe_deg=(0.1, 0.0), caster_deg=4.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=325, pistons_mm=[57, 57], pad_mu=0.40),
            rear=dict(kind="disc", d_mm=330, pistons_mm=[54], pad_mu=0.40),
            bar=P_MODERN, handbrake_frac=0.6,
        ),
        steer=dict(ratio=18.0, turn_circle_m=13.4, ackermann=0.7, rack_speed=1.2),
        price=20000,
        durability=4,
        note=("The delivery van that is always double-parked. Heavy, tall and soft, it leans "
              "like a boat, brakes long and gets out of breath above 150 km/h — but at "
              "city speeds it is a two-and-a-half-tonne wall that wins most shoves."),
        breaks=("Body-on-frame: full-length frame rails take frontal and rear hits, so bumpers "
                "and the short nose crumple while the frame stays straight. The cargo box "
                "behind the seat partition is thin sheet on light hoops — side and rear hits "
                "dent it deeply without touching the cab. The high, flat side is an easy target "
                "and the high CG tips it in hard swerves."),
        sources=["manufacturer spec-class data for 2000s full-size cargo vans",
                 "owner dyno averages for 4.8 L truck V8s",
                 "engineering tables (truck tire radius, leaf/coil ride frequency, caliper hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="steel", hood="steel", pushbar=False, ladder_frame=True,
                     cargo_box=True),
    ),
    # ------------------------------------------------------------------ 17 (traffic)
    dict(
        id="furrow_24",
        display_name="Furrow 2.4",
        cls="Truck",
        analog_of="90s compact RWD pickup, 2.4 L SOHC I4, regular cab, body-on-frame, leaf-sprung live axle",
        era="90s",
        profile="truck",
        curb_kg=1320.0,
        fuel_l=60,
        front_frac=0.58,
        wheelbase=2.85,
        track_f=1.43,
        track_r=1.41,
        body=(1.69, 4.75, 1.60),
        clearance=0.20,
        front_overhang=0.80,
        cg=0.65,
        cg_why="small body-on-frame pickup with an empty bed; bottom of the 0.65-0.75 m truck band (lower and lighter than a full-size)",
        inertia_k=(0.85, 0.95, 0.97),
        cd=0.45,
        area_k=0.84,
        extra_cda=0.0,
        lift=(0.15, 0.15),
        engine=dict(
            desc="2.4 L SOHC 8-valve I4, multi-point injection, truck calibration",
            curve=[(800, 150), (1200, 170), (1600, 185), (2000, 196), (2500, 205), (3000, 212),
                   (3500, 216), (4000, 218), (4500, 213), (5000, 203), (5400, 190), (5800, 170)],
            idle=700, redline=5500, limit=5800, inertia=0.15, brake_frac=0.12,
            why="Class-typical 90s 2.4 L compact-truck four (~44 kW/L, ~106 kW at 5000 rpm, ~218 N·m at 4000) per manufacturer spec-class data.",
        ),
        trans=dict(
            desc="5-speed manual, open rear differential on a leaf-sprung live axle",
            layout="Rwd",
            gears=[3.83, 2.22, 1.40, 1.00, 0.86], final=4.10, reverse=3.83, shift=0.35,
            diff_f="Open", diff_r="Open", clutch_factor=1.35,
        ),
        tires=dict(front="215/70R14", rear="215/70R14", compound="street", unsprung=(30.0, 50.0)),
        susp=dict(hz=(1.30, 1.60), zeta=0.25, travel=(0.18, 0.20), roll_grad_deg_g=5.5,
                  roll_front_share=0.72, bump_stop=200000.0, preload=0.02),
        align=dict(camber_deg=(0.0, 0.0), toe_deg=(0.1, 0.0), caster_deg=3.0),
        brakes=dict(
            front=dict(kind="disc", d_mm=274, pistons_mm=[60], pad_mu=0.38),
            rear=dict(kind="drum", d_mm=254, wheel_cyl_mm=22.0, brake_factor=1.8),
            bar=P_MODERN, handbrake_frac=1.0,
        ),
        steer=dict(ratio=18.5, turn_circle_m=11.4, ackermann=0.7, rack_speed=1.0),
        price=21000,
        durability=4,
        note=("The tradesman's truck: a small, light pickup with a light rear end that steps "
              "out in the wet and a gearbox that likes to be rowed. Slow, but honest, cheap "
              "and hard to kill — every other driveway in the suburbs has one."),
        breaks=("Separate ladder frame and steel bumpers shrug off low-speed knocks, and the "
                "bed is just bent sheet. The 90s regular cab is the weak point: thin pillars "
                "and no real crumple zone ahead of a short nose, so hard frontals and anything "
                "above frame height go into the cab."),
        sources=["manufacturer spec-class data for 90s compact pickups",
                 "road-test figures for the class (0-100, braking, skidpad)",
                 "engineering tables (truck tire radius, leaf-spring ride frequency, disc/drum brake hydraulics)"],
        lattice=dict(mass_frac=0.22, bumper="steel", hood="steel", pushbar=False, ladder_frame=True),
    ),
]
