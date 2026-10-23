# Police radio priority tiers: example transcripts (owner playtest row 144)

Five excerpts from the bench runs on this branch. The columns are:

- **time**: sim time, s.
- **level**: the highest wanted level of any racer at that moment.
- **tier**: 1 actionable, 2 escalation, 3 impact, 4 investigation, 5 routine.
- **cut**: the line cut in over a lower-tier line that was on air.

Excerpts 1 to 4 come from the police harness:

```
cargo run --release -p wp_police --example radio_bench -- <scenario> --log 4
```

Excerpt 5 comes from the full-physics AI street race (`city_sprint_police.ron`, run through `wp_race`'s `race_police` harness).

In the browser:

- Tier 1 lines play a two-tone cue before the voice and skip the scanner delay. The roadblock and spike markers appear on the map in the same frame.
- Tier 3 lines are spoken faster and higher, with louder static.
- Units are male voices; dispatch is a female voice.

## 1. Roadblock and spike sequence, then a takeover (`radio_city_chase.ron`, run 1)

```
    4.1 s  L0  T4      Dispatch: caller reports reckless driver, downtown. Unit 2 responding.
   10.5 s  L1  T2 cut  Unit 2, I just watched racer #1000 westbound downtown speeding past me, I'm on it.
   19.2 s  L2  T1      Dispatch, racer #1000: spikes 350 metres ahead, southbound in the suburbs.
   24.1 s  L3  T1      Dispatch, racer #1000: roadblock 650 metres ahead, southbound in the suburbs.
   29.1 s  L5  T1      Dispatch: air unit launching on racer #1000, ETA 120 seconds.
   33.6 s  L5  T1      Unit 7 moving to intercept racer #1000 ahead in the industrial district, 400 metres.
   42.2 s  L5  T2      Dispatch, all units: racer #1000 is now level 5, southbound in the suburbs. 5 units on him. Tactical units authorised, all units, stop him by any means.
   55.8 s  L5  T3      Unit 4, going for the PIT!
```

How to read it:

- Each tool is announced at the moment it is planned, with the distance ahead and the direction.
- Before this branch, these were "Unit 6 setting up a roadblock on …" lines. They arrived 7 to 14 s after the decision, and most were never spoken at all (see the bench in `police-radio-v2.md`).
- No deploy chatter is spoken at L0 or L1. The call announcement at 4.1 s is investigation (tier 4).

## 2. Department takeover (`radio_race_pack.ron`, run 1: four racers, 2.8× the limit)

```
    0.1 s  L1  T4      Dispatch: units responding, racer #1002 eastbound in the suburbs.
    2.0 s  L1  T2 cut  Unit 3, I just watched racer #1001 eastbound downtown speeding past me, I'm on it.
   11.0 s  L2  T2      Dispatch: 10-80, all units, pursuit of racer #1001 eastbound downtown.
   17.2 s  L3  T1      Dispatch, racer #1001: roadblock 650 metres ahead, southbound in the suburbs.
   22.1 s  L5  T1      Dispatch, racer #1002: spikes 250 metres ahead, northbound in the suburbs.
   29.6 s  L5  T2      Dispatch, all units: racer #1002 is now level 3, northbound in the suburbs. One unit on him. Highway Patrol is taking over the pursuit. Hard rams and rolling blocks authorised.
   32.4 s  L5  T1 cut  Unit 15 moving to intercept racer #1001 ahead in the industrial district, 450 metres.
   38.5 s  L5  T1      Unit 16 moving to intercept racer #1002 ahead in the suburbs, 450 metres.
   46.7 s  L5  T2      Dispatch, all units: racer #1002 is now level 5, southbound in the suburbs. 3 units on him. Tactical units authorised, all units, stop him by any means.
```

How to read it:

- The one "units responding" summary at 0.1 s is the only deploy-style line at L1.
- The level-up lines name the department that takes over and the rams it brings. These come from the police config's `hard_ram_level` and `rear_ram_level`, so the radio cannot promise rams the AI will not use.

## 3. Heavy units, then a big wreck stacked as two lines (`radio_race_pack.ron`, run 3)

```
   46.7 s  L5  T2      Dispatch, all units: racer #1001 is now level 4, eastbound in the industrial district. No units on him yet. Heavy units authorised.
   47.3 s  L5  T1 cut  Unit 24 moving to intercept racer #1003 ahead in the suburbs, 600 metres.
   55.5 s  L5  T2      Dispatch, all units: racer #1003 is now level 5, northbound downtown. 3 units on him. Tactical units authorised, all units, stop him by any means.
   57.6 s  L5  T1 cut  Dispatch, racer #1000: spikes 400 metres ahead, northbound downtown.
   65.8 s  L5  T3      Unit 17, going for the PIT!
   69.6 s  L5  T3      Unit 14, spikes hit! Spikes hit! He's losing tires northbound downtown!
   75.3 s  L5  T4      Unit 10, boxing him in, westbound downtown.
   81.2 s  L5  T3      Unit 6, bent wheel! Bent wheel! I'm out of the chase downtown!
   87.4 s  L5  T3      Unit 6, I'm wrecked! I'm out, I'm out!
   91.9 s  L5  T3      Dispatch: officer down downtown! EMS rolling, all units!
  102.8 s  L5  T1      Dispatch, racer #1000: spikes 500 metres ahead, southbound downtown.
  107.0 s  L5  T1      Dispatch, racer #1003: roadblock 350 metres ahead, southbound in the suburbs.
  116.4 s  L5  T3      Unit 15, I'm wrecked! I'm out, I'm out!
  121.0 s  L5  T3      Unit 24, Unit 15 just got wrecked downtown! Get EMS rolling!
```

## 4. Big wreck between tier 1 calls (`radio_city_chase.ron`, run 3)

```
  177.5 s  L5  T3      Unit 17, bent wheel! Bent wheel! I'm out of the chase downtown!
  179.7 s  L5  T1 cut  Air 16: lost visual on racer #1000 northbound downtown. Units, check the cover.
  184.7 s  L5  T1 cut  Dispatch, racer #1000: spikes 150 metres ahead, northbound downtown.
  191.4 s  L5  T3      Unit 12, officer down downtown! Need EMS!
  195.6 s  L5  T3      Unit 4, Unit 12 just got wrecked downtown! Get EMS rolling!
  198.8 s  L5  T1 cut  Air 16: reacquired, racer #1000 northbound downtown.
  206.2 s  L5  T1      Dispatch, racer #1000: roadblock 700 metres ahead, westbound in the suburbs.
```

How to read excerpts 3 and 4:

- A wreck is two shouted lines back to back. The first is the wrecked unit, and the second is the nearest unit on the same racer, or dispatch.
- Helicopter and tool calls (tier 1) still cut in between other lines.

## 5. Real AI street race with police (full physics, `city_sprint_police.ron` extended to 150 s)

This is from the base (`lead/round12`). Tiers are classified from the text, because the base has no tiers:

```
    9.2 s  L1  T2  Unit 4, I just watched racer #1 northbound downtown speeding past me, I'm on it.
   18.2 s  L1  T2  Dispatch: 10-80, all units, pursuit of racer #1 northbound downtown.
   ...
   50.1 s  L1  T2  Dispatch: 10-80, all units, pursuit of racer #1 downtown.
   58.4 s  L1  T5  Unit 24 in pursuit of racer #1 downtown.
   65.3 s  L1  T2  Dispatch: Unit 3 has racer #1 in custody downtown. Code 4, all units.
```

This is the same race on this branch:

```
    9.2 s  L1  T2  Unit 4, I just watched racer #1 northbound downtown speeding past me, I'm on it.
   18.2 s  L1  T2  Dispatch: 10-80, all units, pursuit of racer #1 northbound downtown.
   25.2 s  L1  T2  Unit 4, I just watched racer #2 northbound downtown speeding past me, I'm on it.
   34.2 s  L1  T2  Dispatch: all units, we've lost racer #1, last seen northbound downtown. Resume patrol.
   46.4 s  L1  T4  Unit 13, lost visual. Last seen northbound downtown.
   50.1 s  L1  T2  Dispatch: 10-80, all units, pursuit of racer #1 downtown.
   56.8 s  L1  T3  Unit 12, I'm into the wall! I'm into the wall!
   63.5 s  L1  T3  Unit 15, I'm into the wall! I'm into the wall!
   65.3 s  L1  T2  Dispatch: Unit 3 has racer #1 in custody downtown. Code 4, all units.
   73.5 s  L1  T2  Dispatch: Unit 4 has racer #3 in custody downtown. Code 4, all units.
   84.9 s  L1  T2  Dispatch: Unit 9 has racer #2 in custody downtown. Code 4, all units.
   93.1 s  L0  T2  Dispatch: Unit 15 has racer #4 in custody downtown. Code 4, all units.
```

How to read it:

- The level 1 "in pursuit of" deploy line is gone.
- The real physics contacts now produce impact lines picked by severity.
- This race exposed two wrong lines, which are fixed on this branch:
  - "officer down" for a car that then kept driving. Collision energy alone now tops out at "heavy", and only a car the damage model wrecked gets a wreck line.
  - "still on him!" from a unit scraping a car after every racer was in custody. A unit with nobody to chase no longer narrates its own crashes.
