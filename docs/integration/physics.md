# Vehicle physics lead — `lead/physics`

Owner of handling (brief: `docs/briefs/vehicle-physics-lead.md`). Branch from `lead/round12`.

## 2026-10-22 — start
- Baseline `held_search ONLY_BASE=1`: held 2.24 s, yaw_ok 0.35, taps 20 / 36, handling fails 0.
- **Shipped player rack already shrinks lock with speed:** a held key gets 84 % / 65 % / 49 % of
  full lock at 60 / 100 / 140 km/h (grip-balance measurement). With no speed reduction
  (`RACK=full`) the held drift drops to 2.13 s, taps 12 / 36.
- grip-balance (session_01FKfdoiNS5WT1M85cD1WAu9) asked to push its lock-fixed re-search and
  hand off.

## Reference rack vs speed (owner question: "is it speed scaled?")
Speedometer read from the 16:30 / 16:34 clips (`tools/ref-clips/speedo_glyphs.py` segments the
digits; 10 hand-labelled templates; 0.5 s median; read on 61–72 % of frames). Held steer keys,
≥ 0.3 s into the hold; "past view" = wheel marker beyond ~70° (hidden by the dash, i.e. near or
at full lock; the wheel's full-lock marker angle is unknown, > 90° on parity_rwd_f).

| speed | rwd_e past view | rwd_f past view | visible held angle |
|---|---|---|---|
| < 48 km/h | 29 % | 94 % | 19–48° (low-speed taps / parking) |
| 48–97 km/h | 86–90 % | 100 % | — |
| 97–121 km/h | 36 % | 96 % | 25–62° |
| 121–145 km/h | 0 % | 52 % | 46–59° |
| > 145 km/h | — | 0 % (n = 3) | ~58° |

Reading: **no visible speed reduction up to ~110–120 km/h; above that a held key stops near
~50–60° of wheel marker** (some reduction; the exact share of full lock is unknown because full
lock is out of view). Ours already cuts to 65 % at 100 km/h, earlier and harder than the
reference.

## Touched outside my crates (to compile)
- `wp_police` (round12 8757f12f did not compile after PR #42 met the row-144 radio tiers):
  `race.rs` `RadioTier::Critical` → `Escalation` (its documented successor), and
  `SpikeStrip::new` sets `scripted: false` (as the other constructor). `cargo test -p wp_police`
  green. The lead should take its own fix over this one if it differs.
