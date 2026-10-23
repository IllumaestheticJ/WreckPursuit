// Loading screen (owner-approved feature F5): building a city session blocks the main thread for
// several seconds (wasm world build, then the car meshes). Starting from the title, the screen
// goes up first (painted before the blocking step), shows the step and a progress bar, and a
// driving tip; it comes down after the first frame of the new session.

/** Tips shown while loading (one per load, in turn). */
export const TIPS = [
  "Gears are manual by default: Shift up, Ctrl down. Settings → Assists → Automatic gears if you'd rather not.",
  "Heat rises with speeding, red lights and hitting cars. The bar at the top fills; lose sight of the police to escape.",
  "Pull up right behind a street racer (a numbered dot on the map) and honk to challenge them.",
  "R puts the car back on the nearest lane. In a career run, repairs are bought in the garage.",
  "M or Tab opens the city map: garages, race meets and the police around you.",
  "Big crash? Enter plays it back. F7 is photo mode, live or in a replay.",
  "Rain soaks the road: brake earlier and feather the throttle out of corners.",
  "Spike strips and roadblocks are called out on the police radio before you reach them.",
];

/** Wait until the browser painted (the screen shows before the next blocking step). */
const painted = () => new Promise<void>((r) => requestAnimationFrame(() => setTimeout(r, 0)));

/** One loading step: its label and the work (sync or async). */
export interface LoadStep {
  label: string;
  run: () => unknown;
}

/** The loading screen (`#wp-loading`). */
export class LoadingScreen {
  readonly el: HTMLElement;
  /** A load is running. */
  active = false;
  /** Loads finished this page (e2e). */
  loads = 0;
  private readonly title: HTMLElement;
  private readonly stage: HTMLElement;
  private readonly fill: HTMLElement;
  private readonly tip: HTMLElement;
  private tipIndex = 0;

  constructor(parent: HTMLElement) {
    this.el = document.createElement("div");
    this.el.id = "wp-loading";
    this.el.className = "wp-ui hidden";
    this.el.setAttribute("role", "progressbar");
    this.el.setAttribute("aria-valuemin", "0");
    this.el.setAttribute("aria-valuemax", "100");
    const card = document.createElement("div");
    card.className = "wp-loading-card";
    this.title = document.createElement("h2");
    this.stage = document.createElement("div");
    this.stage.className = "wp-loading-stage";
    const bar = document.createElement("div");
    bar.className = "wp-loading-bar";
    this.fill = document.createElement("i");
    bar.append(this.fill);
    this.tip = document.createElement("p");
    this.tip.className = "wp-loading-tip";
    card.append(this.title, this.stage, bar, this.tip);
    this.el.append(card);
    parent.appendChild(this.el);
  }

  /** Run `steps` with the screen up: each step's label and the bar move before it runs. */
  async run(title: string, steps: LoadStep[]): Promise<void> {
    this.active = true;
    this.title.textContent = title;
    this.tip.textContent = `Tip: ${TIPS[this.tipIndex++ % TIPS.length]}`;
    this.el.classList.remove("hidden");
    try {
      for (let i = 0; i < steps.length; i++) {
        this.progress(steps[i].label, i / steps.length);
        await painted();
        await steps[i].run();
      }
      this.progress("Ready", 1);
      await painted();
    } finally {
      this.active = false;
      this.loads++;
      this.el.classList.add("hidden");
    }
  }

  private progress(label: string, frac: number): void {
    this.stage.textContent = label;
    const pct = Math.round(frac * 100);
    this.fill.style.width = `${pct}%`;
    this.el.setAttribute("aria-valuenow", String(pct));
  }
}
