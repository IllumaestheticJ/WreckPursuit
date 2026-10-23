/**
 * Car thumbnails and turntable strips (`data/cars/thumbs/<id>.png` / `<id>.turn.png`), rendered
 * from each car's panel mesh by `cargo run -p car-assets --bin gen-thumbs` (cars audit F26).
 * Vite turns them into asset URLs, so the kit's demo and the testbed both find them.
 */
const thumbFiles = import.meta.glob("../../../../data/cars/thumbs/*.png", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

const thumbs = new Map<string, string>();
const turns = new Map<string, string>();
for (const [path, url] of Object.entries(thumbFiles)) {
  const file = path.slice(path.lastIndexOf("/") + 1);
  if (file.endsWith(".turn.png")) turns.set(file.slice(0, -".turn.png".length), url);
  else thumbs.set(file.slice(0, -".png".length), url);
}

/** Frames in a turntable strip (`car_assets::thumb::TURN_FRAMES`). */
export const TURN_FRAMES = 12;

/** URL of a car's three-quarter thumbnail (undefined: no thumbnail for that id). */
export function carThumbUrl(id: string): string | undefined {
  return thumbs.get(id);
}

/** URL of a car's turntable strip ({@link TURN_FRAMES} frames side by side). */
export function carTurntableUrl(id: string): string | undefined {
  return turns.get(id);
}
