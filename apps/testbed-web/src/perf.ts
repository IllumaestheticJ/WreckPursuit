// Per-frame cost breakdown of the testbed's frame loop (browser perf pass, docs/integration/
// browser-perf.md). `lap(name)` charges the time since the previous lap to a section. Smoothed
// values feed the HUD; raw sums (`acc`, `frames`) let automation average over a window
// (e2e/frame_profile.mjs resets them, waits, and reads them back). No allocation per frame.

/** Frame-loop sections, in loop order. "render" is the CPU side of the WebGL submit only;
 * SwiftShader / GPU raster shows up as the wall frame time minus the sum of the sections. */
export const SECTIONS = ["input", "sim", "views", "events", "traffic", "weather", "camera", "audio", "ui", "render"] as const;
export type Section = (typeof SECTIONS)[number];

export class FrameProfiler {
  /** Smoothed ms per section (EMA, 0.1). */
  readonly ms: Record<Section, number>;
  /** Raw ms per section since the last `resetAcc()`. */
  readonly acc: Record<Section, number>;
  /** Frames counted in `acc`. */
  frames = 0;
  private last = 0;
  private readonly frameMs: Record<Section, number>;

  constructor() {
    const zero = () => Object.fromEntries(SECTIONS.map((s) => [s, 0])) as Record<Section, number>;
    this.ms = zero();
    this.acc = zero();
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
    }
    this.frames++;
  }

  resetAcc(): void {
    for (const s of SECTIONS) this.acc[s] = 0;
    this.frames = 0;
  }
}
