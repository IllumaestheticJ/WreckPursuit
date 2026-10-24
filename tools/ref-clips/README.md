# Reference clips: the car's yaw from the cockpit video

Tools for the owner's reference-game keyboard clips (`docs/reference/wf-clips/`). They measure
how the car rotates while a steer key is held, so we can tell a held **countersteer** key (the
car rotates against the key) from a held key **into** the turn, and whether the car spins.

## How it works
1. `yaw_from_clip.py`: decodes a horizon band of the windscreen at 30 fps (ffmpeg) and measures
   its sideways shift between frames (phase correlation of the band's horizontal gradient).
   The scenery is far away, so the shift is the car's yaw. It writes the shift per frame plus
   the clip's key states.
2. `held_keys.py`: turns the shift into a yaw rate and lists every steer-key hold of 0.8 s or
   more, with how much of it the car rotated against / with the key, the heading change and
   spins.

## Run
From the repo root (needs ffmpeg and numpy):

    python3 -I tools/ref-clips/yaw_from_clip.py docs/reference/wf-clips/2026-10-21_1634_parity_rwd_f.mp4 \
        docs/reference/wf-clips/2026-10-21_1634_parity_rwd_f_track.json top /tmp/1634_raw.json
    python3 -I tools/ref-clips/held_keys.py /tmp/1634_raw.json \
        docs/reference/wf-clips/2026-10-21_1634_parity_rwd_f_yaw.json Off

`top` / `bottom` is the clip's layout: where the game view sits in the 960×540 recording
(16:30 and 16:34: top; 16:37 and 16:39: bottom). `run_all.sh` does all four clips.

## Limits
- The field of view is a guess (~70° across the game view), so yaw **rates** and heading
  changes are approximate. Signs, and so countersteer / into-turn, do not depend on it.
- The sign was checked by eye on two holds (16:34 at 9.9–11.4 s, 16:37 at 15–16 s).
- Speed and slide angle are not measured.

## Result (2026-10-22)
Held countersteer keys of 0.8 s or more, with the throttle mostly on: 16:30 (TCS Off) 6 holds,
longest 3.6 s, no spin; 16:34 (Off) 6, longest 5.7 s, 1 spin (a 5.7 s hold that slowed to
~5 km/h); 16:37 (Off) 9, longest 8.2 s, no spin; 16:39 (TCS Half) 5, longest 3.3 s, no spin.
In the reference a held countersteer key at full lock holds the drift.
