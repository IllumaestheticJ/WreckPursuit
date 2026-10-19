# data/tires — tire compounds

**Owned by workstream 02 (vehicle dynamics).** Schema: `schemas/tire_def.schema.json`.

Simplified Magic Formula per axis: `F = D·Fz·sin(C·atan(Bx − E(Bx − atan Bx)))`, with
`D` the peak friction coefficient at `fz0_n`, load sensitivity
`D_eff = d·(1 − ls·(Fz/Fz0 − 1))`, combined slip by normalised slip vector
(`peak_slip_*` are the normalisers and must match the curves' peaks — they are computed from
the coefficients, not hand-typed).

| id | lat D | long D | peak α | peak κ | wet | Use |
|---|---|---|---|---|---|---|
| `street` | 1.00 | 1.10 | 7.6° | 0.115 | 0.70 | all-season touring, default |
| `sport` | 1.10 | 1.20 | 6.2° | 0.101 | 0.62 | summer performance |
| `semi_slick` | 1.28 | 1.36 | 4.7° | 0.080 | 0.50 | track-day, edgy past the peak |
| `wet_biased` | 0.96 | 1.03 | 8.0° | 0.134 | 0.86 | rain / winter-ish, forgiving |
| `police_pursuit` | 1.04 | 1.14 | 7.0° | 0.108 | 0.72 | heavy-duty pursuit, low wear, higher Fz0 |

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
  ~0.45–0.68; grass lowest.
- Rolling resistance 0.010–0.013; `wear_rate` is a placeholder (wear not simulated yet).

All coefficient sets are generic engineering approximations, not fitted to any branded tire.
