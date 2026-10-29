# Traffic pop-in in Free Roam (lead/car-popin)

Owner report: "cars also render in free roam too close". Evidence for the fix in
`docs/integration/car-popin.md`.

Every image is the chase camera in Free Roam (`?roam=10`, combined world, 1280×720, `gfx=high`),
the player moved to the spot, 40 s for traffic to settle around it, then 6 s of driving down the
road. `before_*` is `main` at 7fa64d8a, `after_*` this branch; same script, same spots, same
seeds.

| File | What |
|---|---|
| `<label>_vesper_downtown_{day,night}.png` | Port Vesper, the long downtown avenue at x ≈ 0 southbound (~980 m of clear sightline, the busiest district), 13:00 and 22:00 |
| `<label>_ashpeak_route7_{day,night}.png` | Ashpeak, Route 7 southbound north of Ashpeak City (~1060 m of clear sightline) |
| `*_zoom.png` | The nearest traffic car that appeared in plain view (frame before / frame it appeared in, 2× crop); when none did, the nearest that appeared in the view cone behind cover |
| `<label>_summary.json` | Per shot: traffic census by distance, every car that appeared or vanished inside the camera's view cone (position, camera position, frames), traffic not drawn within the draw distance, frame times |
| `<label>_los.json` | Per event: in plain view (line of sight from the camera to the car's body or roof on the same map) or behind cover |

Rings in the strips: **red** a traffic car that appeared in plain view in the second before the
frame, **grey** one that appeared behind cover (a building, crest or bend), **cyan** / **dark**
the same for cars that vanished.

Regenerate (after `npm run wasm` in both trees; raw frames go to `raw/`, not committed):

```sh
cd apps/testbed-web
node e2e/popin_shots.mjs ../../docs/media/popin before /path/to/main-worktree/apps/testbed-web
node e2e/popin_shots.mjs ../../docs/media/popin after
cargo test -p wp_world --release --test popin -- --ignored --nocapture classify_browser_popins
node e2e/popin_compose.mjs ../../docs/media/popin before after
```
