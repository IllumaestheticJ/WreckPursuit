# data/tires — tire compounds

**Owned by workstream 02 (vehicle dynamics).** Schema: `schemas/tire_def.schema.json`.

Simplified Magic Formula per axis: `F = D·Fz·sin(C·atan(Bx − E(Bx − atan Bx)))`, with
`D` the peak friction coefficient at `fz0_n`, load sensitivity
`D_eff = d·(1 − ls·(Fz/Fz0 − 1))`, combined slip by normalised slip vector
(`peak_slip_*` are the normalisers and must match the curves' peaks — they are computed from
the coefficients, not hand-typed).

| id | lat D | long D | peak α | peak κ | wet | paint | Use |
|---|---|---|---|---|---|---|---|
| `street` | 0.96 | 1.10 | 7.6° | 0.115 | 0.70 | 0.95 | all-season touring, default (debug sedan 0.82 g) |
| `sport` | 1.10 | 1.20 | 6.2° | 0.101 | 0.62 | 0.94 | summer performance |
| `semi_slick` | 1.28 | 1.36 | 4.7° | 0.080 | 0.50 | 0.92 | track-day, edgy past the peak |
| `wet_biased` | 0.96 | 1.03 | 8.0° | 0.134 | 0.86 | 0.96 | rain / winter-ish, forgiving |
| `police_pursuit` | 1.04 | 1.14 | 7.0° | 0.108 | 0.72 | 0.95 | heavy-duty pursuit, low wear, higher Fz0 |
| `all_terrain` | 0.86 | 0.98 | 12.0° | 0.158 | 0.72 | 0.94 | pickups/SUVs: ≈ 0.9 × street, soft (low cornering stiffness), +33% rolling resistance, best on gravel/dirt (0.82–0.84) |
| `vintage_bias` | 0.81 | 0.93 | 8.3° | 0.132 | 0.60 | 0.93 | 70s bias-ply: ≈ 0.85 × street, ~half street's cornering stiffness, strong fall-off past the peak (85% at 2× peak slip), long relaxation length (lazy) |
| `drag_radial` | 0.98 | 1.45 | 11.5° | 0.130 | 0.45 | 0.92 | street-legal drag tire: huge straight-line traction, soft sidewall (weak cornering response), terrible in the wet, fast wear |

## Source reasoning

- **Peak μ:** published flat-track and skidpad data put road-car tires at 0.85–1.0 g on street
  rubber and ~1.0–1.1 g on summer performance tires; semi-slicks 1.2–1.3. Longitudinal peaks
  sit ~5–10% above lateral. The D values are a little above the car-level g because load
  transfer and load sensitivity take ~8–10% back (the debug cars land at 0.89 / 0.95 g on a
  50 m skidpad).
- **Peak slip:** passenger tires peak at 6–9° slip angle and 10–15% slip ratio; stiffer
  carcasses (semi-slick) peak earlier (4–5°, ~8%). `B` was chosen to place the peak, `C`
  ≈ 1.3–1.45 lateral / 1.55–1.7 longitudinal (typical Pacejka shape factors), `E` slightly
  negative laterally (gentle fall-off) and positive longitudinally.
- **Load sensitivity 0.10–0.15:** typical μ loss of 10–15% per doubling of load.
- **Relaxation length:** ~1–1.5× the contact-patch length scale; shorter for stiff
  sidewalls. Also the keyboard-friendliness knob.
- **Surfaces:** wet asphalt keeps 60–85% of dry grip depending on tread; loose surfaces
  ~0.45–0.84 (all-terrain highest). Dry grass uses the same multiplier as dirt so it drives
  like a dirt road; it only gets slicker than dirt once wet (`wet_loss_scale` 1.3 vs 1.2).
- **`PaintedLine` (contract v0.3.0, added by 09 / H in round 8)** is the *dry* tire-on-paint
  multiplier: 0.92–0.96. Road paint fills the asphalt macrotexture, so the hysteresis part of
  grip drops a few percent; compounds that lean hardest on hysteresis (semi-slick, drag radial,
  hard 70s bias-ply rubber) lose most, soft siped tires least. The **wet** loss on paint (to
  roughly half of dry) is not in this table: it comes from the conditions model
  (`wp_vehicle::surface` / `wp_weather::grip`), scaled by each tire's `WetAsphalt / Asphalt`
  ratio like every other surface, so the paint value must not also carry a dry paint factor
  there (see `docs/integration/cars.md`).
- **`treaded`** (contract v0.3.0): `semi_slick` is `false` (aquaplanes at the slick onset);
  every other compound, the drag radial included (DOT street tire with shallow tread), is
  treaded (`None`).
- `street` lateral D = 0.96 (was 1.0): all-season car-level grip is 0.82–0.92 g (09's class
  band); 1.0 read like a summer tire.
- **`treaded`** (contract v0.3.0): `false` on `semi_slick` and `drag_radial` (no drainage
  grooves: they aquaplane at ≈ 0.65× the treaded onset speed). Omitted = inferred from the wet
  ratio (`≥ 0.6` → treaded), which matches every other compound here.
- **`PaintedLine`**: no tire lists it; the vehicle uses the tire's `Asphalt` entry there and
  applies the paint factor itself (`wp_vehicle::surface`: ×0.95 dry, ×0.65 of the wet road).
  Add a `PaintedLine` entry only for a compound that should differ on paint when dry.
- Rolling resistance 0.010–0.016; `wear_rate` is a placeholder (wear not simulated yet).

All coefficient sets are generic engineering approximations, not fitted to any branded tire.
