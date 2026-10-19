# Playtest log (owner feedback)

## 2026-10-19 — first city chase on the published link
| # | Feedback | Owner of the fix | Status |
|---|---|---|---|
| 1 | No city buildings on the published link | lead (artifact can't serve .glb → base64 fallback in `scene.ts`) | fixed, link v5 |
| 2 | No visible car damage | lead: lattice view on by default; real denting panels = R (prompt 21) | interim fix on link |
| 3 | Cars feel really slow | helper `lead/playtest-feel`: browser sim perf (sim fell behind real time at ~35–50 ms/frame in city chase) + car speed/feel | dispatched |
| 4 | Transmission is sloppy | helper `lead/playtest-feel` (shift logic) | dispatched |
| 5 | Use real open-source sounds | J (audio): CC0 recorded packs | dispatched |
| 6 | Dispatcher and cops are way too talkative | helper `lead/playtest-radio` | dispatched |
| 7 | Camera zooms way too far out; need more angles | helper `lead/playtest-camera` | fixed on `lead/playtest-camera`: chase was a world-space spring trailing ≈12 m at chase speed → car-relative near chase 5.6 m back / 1.8 m up (≤1.2 m pull-back; 6.4 m measured at 115 km/h); 8 angles on C / 1–8 (near, far, hood, bumper, cockpit, side, TV, orbit), V hold look back, mouse/stick look-around, remembered per viewer; shots in `docs/media/cameras/` |
