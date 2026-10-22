// Per-frame cost breakdown of the testbed's frame loop (browser perf pass, docs/integration/
// browser-perf.md). `lap(name)` charges the time since the previous lap to a section. Smoothed
// values feed the HUD; raw sums (`acc`, `frames`) let automation average over a window
// (e2e/frame_profile.mjs resets them, waits, and reads them back). No allocation per frame.

/** Frame-loop sections, in loop order. "render" is the CPU side of the WebGL submit only;
 * SwiftShader / GPU raster shows up as the wall frame time minus the sum of the sections. */
export const SECTIONS = [
  "input",
  "sim",
  // Crash replay director (offer / camera; the replay buffer itself runs inside "sim").
  "crash",
  "views",
  "events",
  "fx",
  "traffic",
  "weather",
  "camera",
  "audio",
  "ui",
  "map",
  "layout",
  // Instanced traffic / police fleet (render/lod.ts) and parked cars / cones (render/props.ts).
  "fleet",
  "props",
  // Shader prewarm (SceneView.prewarmEvery: ≤ 1 Hz, compiles only materials not seen before).
  "shaders",
  "render",
] as const;
export type Section = (typeof SECTIONS)[number];

export class FrameProfiler {
  /** Smoothed ms per section (EMA, 0.1). */
  readonly ms: Record<Section, number>;
  /** Raw ms per section since the last `resetAcc()`. */
  readonly acc: Record<Section, number>;
  /** Worst single frame per section since the last `resetAcc()`, ms (spikes: GC, a compile). */
  readonly peak: Record<Section, number>;
  /** Frames counted in `acc`. */
  frames = 0;
  private last = 0;
  private readonly frameMs: Record<Section, number>;

  constructor() {
    const zero = () => Object.fromEntries(SECTIONS.map((s) => [s, 0])) as Record<Section, number>;
    this.ms = zero();
    this.acc = zero();
    this.peak = zero();
    this.frameMs = zero();
  }

  /** Start of a frame. */
  begin(now = performance.now()): void {
    this.last = now;
    for (const s of SECTIONS) this.frameMs[s] = 0;
  }

  /** Charge the time since the previous lap (or `begin`) to `section`. */
  lap(section: Section): void {
    const now = performance.now();
    this.frameMs[section] += now - this.last;
    this.last = now;
  }

  /** End of a frame: fold this frame into the smoothed and raw sums. */
  end(): void {
    for (const s of SECTIONS) {
      const v = this.frameMs[s];
      this.ms[s] += (v - this.ms[s]) * 0.1;
      this.acc[s] += v;
      if (v > this.peak[s]) this.peak[s] = v;
    }
    this.frames++;
  }

  resetAcc(): void {
    for (const s of SECTIONS) this.acc[s] = this.peak[s] = 0;
    this.frames = 0;
  }
}

/** Render features the perf report attributes draw calls and raster time to
 * (e2e/frame_profile.mjs walks each drawn object's parents to the first tagged one). */
export const FEATURES = ["city", "lamps", "sky", "cars", "fleet", "props", "fx", "signals", "weather", "markings"] as const;
export type Feature = (typeof FEATURES)[number];

/** Tag a scene root with its feature (`userData.perf`); untagged draws count as "other". */
export function tagFeature<T extends { userData: Record<string, unknown> }>(obj: T, feature: Feature): T {
  obj.userData.perf = feature;
  return obj;
}
