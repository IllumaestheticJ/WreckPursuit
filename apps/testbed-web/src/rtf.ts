// Real-time factor: simulated seconds per wall-clock second over a rolling window. Below 1 the
// game runs in slow motion (the browser cannot keep up with the 240 Hz fixed step, or frames
// are so slow that the per-frame catch-up cap drops time).

/** Below this the HUD warns that the game is running in slow motion. */
export const RTF_WARN = 0.95;

/** Rolling real-time-factor meter fed once per animation frame. */
export class RealTimeMeter {
  private wall = 0;
  private simS = 0;
  private lastTick = -1;
  /** Last completed window's factor (1 until the first window completes). */
  factor = 1;

  /** `windowS`: averaging window in wall seconds. */
  constructor(private readonly windowS = 1.0) {}

  /**
   * Feed one frame: `wallDt` = unclamped wall seconds since the previous frame, `tick` = sim
   * tick after this frame's step, `running` = false while paused or replaying (not measured).
   */
  push(wallDt: number, tick: number, running: boolean): void {
    if (!running || this.lastTick < 0 || tick < this.lastTick) {
      // Paused, first frame, or a new session (tick went back): restart the window.
      this.lastTick = tick;
      this.wall = 0;
      this.simS = 0;
      if (!running) this.factor = 1;
      return;
    }
    this.wall += Math.max(0, wallDt);
    this.simS += (tick - this.lastTick) / 240;
    this.lastTick = tick;
    if (this.wall >= this.windowS) {
      this.factor = this.simS / this.wall;
      this.wall = 0;
      this.simS = 0;
    }
  }

  /** True when the last window ran noticeably slower than real time. */
  get slow(): boolean {
    return this.factor < RTF_WARN;
  }
}
