// Blind handling A/B ("eye test", owner request 2026-10-22): "guide me through as a tutorial —
// don't tell me the settings, have me blind test it and ask me which feels better, like an eye
// doctor". Each test starts from the shipped handling and changes ONE thing. The two options are
// shown as "Option 1" / "Option 2" in a random order; the owner drives both and picks one (or
// "same"). The answers are revealed at the end with a short result code to paste back to the
// physics lead. UI only: the order is shuffled with Math.random, nothing here runs in the sim.

/** What the tune panel lets the eye test change. */
export interface EyeHost {
  /** Steering, tyres, grip balance and ride back to the shipped handling (assists untouched). */
  resetShipped(): void;
  /** Merge into the shipped player steering (saved, so a car restart keeps it). */
  setSteer(patch: Record<string, unknown>): void;
  /** Merge into the shipped tyres (saved). */
  setTyres(patch: Record<string, unknown>): void;
  /** Merge into the shipped grip balance (saved). */
  setGrip(patch: Record<string, unknown>): void;
  /** Merge into the shipped ride; `base` picks which ride default to start from (saved). */
  setRide(patch: Record<string, unknown>, base?: string): void;
  /** TCS level for the test, or `null` to restore the viewer's own choice. */
  setTcs(level: "Off" | "Half" | "Full" | null): void;
  /** Respawn the car (needed after inertia / ride changes). */
  restartCar(): void;
}

/** One option of a test: its real name (revealed at the end) and how to apply it. */
interface EyeOption {
  name: string;
  apply(h: EyeHost): void;
}

/** One blind comparison. */
interface EyeCase {
  id: string;
  /** What to pay attention to (does not say what differs). */
  focus: string;
  /** Respawn the car when switching options. */
  restart: boolean;
  a: EyeOption;
  b: EyeOption;
}

const RACK_SHIPPED = { speed_sensitivity: 0.75, sens_ref_kmh: 113.0, sens_exponent: 2.0 };
const RACK_REF = { speed_sensitivity: 0.75, sens_ref_kmh: 172.0, sens_exponent: 4.0 };
const RACK_FULL = { speed_sensitivity: 0.0, sens_ref_kmh: 113.0, sens_exponent: 2.0 };

const TESTS: EyeCase[] = [
  {
    id: "rack",
    focus: "Steering at speed (80–150 km/h): highway sweepers, quick lane changes, catching a slide at speed.",
    restart: false,
    a: { name: "rack shipped", apply: (h) => h.setSteer(RACK_SHIPPED) },
    b: { name: "rack reference", apply: (h) => h.setSteer(RACK_REF) },
  },
  {
    id: "rack2",
    focus: "Steering at speed again (80–150 km/h). Same kind of driving as the last test.",
    restart: false,
    a: { name: "rack reference", apply: (h) => h.setSteer(RACK_REF) },
    b: { name: "rack full lock", apply: (h) => h.setSteer(RACK_FULL) },
  },
  {
    id: "ride",
    focus: "How the weight moves: turn-in, lifting off mid-corner, switching direction. (The car respawns when you switch.)",
    restart: true,
    a: { name: "ride shipped", apply: () => {} },
    b: { name: "ride old lab (soft)", apply: (h) => h.setRide({}, "lab") },
  },
  {
    id: "drift",
    focus: "Holding a drift with the countersteer key held and the throttle, and catching it. (The car respawns when you switch.)",
    restart: true,
    a: { name: "tyres shipped", apply: () => {} },
    b: {
      // held_search: best lock-preserving set inside every guard rail (grip-balance handover).
      name: "tyres+weight candidate G",
      apply: (h) => {
        h.setTyres({ spin_b0: 6.48, spin_b2: 1.75, spin_c: 0.9, side_floor: 0.38, rear_relax: 1.41, rear_peak_slip: 1.1 });
        h.setGrip({ rear_stiffness: 0.533, front_stiffness: 0.666 });
        h.setRide({ yaw_inertia: 1.35, spring: 1.04, arb: 0.63, roll_inertia: 1.34 });
      },
    },
  },
  {
    id: "tcs",
    focus: "Throttle control in a drift: how steady the angle stays with the throttle held.",
    restart: false,
    a: { name: "TCS Off", apply: (h) => h.setTcs("Off") },
    b: { name: "TCS Half", apply: (h) => h.setTcs("Half") },
  },
];

const STORE = "wp.tune.eye.v1";

interface EyeState {
  /** Index of the current test; TESTS.length = finished. */
  i: number;
  /** Per test: true = Option 1 is `b`. */
  swap: boolean[];
  /** Per test: 1 / 2 picked, 0 = same, null = not yet. */
  picks: (0 | 1 | 2 | null)[];
  /** The option being driven (1 / 2), or null before the first click. */
  driving: 1 | 2 | null;
}

/** The blind A/B session shown at the top of the tune panel. */
export class EyeTest {
  private state: EyeState | null = null;

  constructor(private readonly host: EyeHost) {
    try {
      const v = JSON.parse(localStorage.getItem(STORE) ?? "null") as EyeState | null;
      if (v && Array.isArray(v.swap) && v.swap.length === TESTS.length) this.state = v;
    } catch {
      /* no saved session */
    }
  }

  /** A test is in progress (the panel hides everything else so nothing gives it away). */
  running(): boolean {
    return this.state !== null && this.state.i < TESTS.length;
  }

  private persist(): void {
    try {
      if (this.state) localStorage.setItem(STORE, JSON.stringify(this.state));
      else localStorage.removeItem(STORE);
    } catch {
      /* ignore */
    }
  }

  /** Start a fresh session (new random order). */
  start(): void {
    this.state = { i: 0, swap: TESTS.map(() => Math.random() < 0.5), picks: TESTS.map(() => null), driving: null };
    this.drive(1);
  }

  /** Stop and put the shipped handling back. */
  stop(): void {
    this.state = null;
    this.persist();
    this.host.resetShipped();
    this.host.setTcs(null);
    this.host.restartCar();
  }

  /** Apply Option `n` of the current test. */
  drive(n: 1 | 2): void {
    const s = this.state;
    if (!s || s.i >= TESTS.length) return;
    const t = TESTS[s.i];
    const useB = (n === 1) === s.swap[s.i];
    this.host.resetShipped();
    this.host.setTcs(null);
    (useB ? t.b : t.a).apply(this.host);
    const changed = s.driving !== n;
    s.driving = n;
    this.persist();
    if (t.restart && changed) this.host.restartCar();
  }

  /** Record the verdict for the current test and move on. */
  pick(p: 0 | 1 | 2): void {
    const s = this.state;
    if (!s || s.i >= TESTS.length) return;
    s.picks[s.i] = p;
    s.i += 1;
    s.driving = null;
    if (s.i < TESTS.length) {
      this.drive(1);
    } else {
      this.persist();
      this.host.resetShipped();
      this.host.setTcs(null);
      this.host.restartCar();
    }
  }

  /** The real name the owner preferred in test `k` ("same" for no preference). */
  private winner(k: number): string {
    const s = this.state!;
    const p = s.picks[k];
    if (p === null) return "skipped";
    if (p === 0) return "same";
    const useB = (p === 1) === s.swap[k];
    return (useB ? TESTS[k].b : TESTS[k].a).name;
  }

  /** Paste-back code: one `test:winner` per test. */
  resultCode(): string {
    return `eye1 ${TESTS.map((t, k) => `${t.id}=${this.winner(k).replace(/ /g, "_")}`).join(" ")}`;
  }

  /** The eye-test section; `rebuild` redraws the whole panel. */
  render(rebuild: () => void): HTMLElement {
    const box = document.createElement("div");
    box.id = "eye-test";
    box.style.cssText = "border:1px solid #2f6fdf;border-radius:4px;padding:6px 8px;margin:4px 0 8px;background:rgba(47,111,223,0.08)";
    const title = document.createElement("div");
    title.style.cssText = "font-weight:600;color:#9cf;margin-bottom:4px";
    box.appendChild(title);
    const text = (t: string, css = "") => {
      const d = document.createElement("div");
      d.textContent = t;
      d.style.cssText = `margin:3px 0;${css}`;
      box.appendChild(d);
      return d;
    };
    const btn = (label: string, on: () => void, active = false, id?: string) => {
      const b = document.createElement("button");
      b.textContent = active ? `✓ ${label}` : label;
      if (id) b.id = id;
      if (active) b.style.cssText = "background:#2f6fdf;color:#fff;font-weight:600";
      b.onclick = () => {
        on();
        rebuild();
      };
      return b;
    };
    const rowOf = (...els: HTMLElement[]) => {
      const r = document.createElement("div");
      r.style.cssText = "display:flex;gap:6px;flex-wrap:wrap;margin:4px 0";
      r.append(...els);
      box.appendChild(r);
    };
    const s = this.state;
    if (!s) {
      title.textContent = "Eye test (blind handling A/B)";
      text(`${TESTS.length} short tests. Each one starts from the shipped handling and changes one hidden thing. Drive Option 1, then Option 2 (switch as often as you like), then say which felt better. The answers are revealed at the end.`);
      rowOf(btn("start the eye test", () => this.start(), false, "eye-start"));
      return box;
    }
    if (s.i >= TESTS.length) {
      title.textContent = "Eye test: results";
      for (let k = 0; k < TESTS.length; k++) {
        const p = s.picks[k];
        const optName = (n: 1 | 2) => (((n === 1) === s.swap[k]) ? TESTS[k].b : TESTS[k].a).name;
        text(`${k + 1}. Option 1 = ${optName(1)}, Option 2 = ${optName(2)} → you picked ${p === 0 ? "same" : p === null ? "—" : `Option ${p}`} (${this.winner(k)})`);
      }
      text("The shipped handling is back. Paste this code to the physics lead:", "margin-top:6px");
      const code = document.createElement("input");
      code.id = "eye-code";
      code.readOnly = true;
      code.value = this.resultCode();
      code.style.cssText = "width:100%;font:11px ui-monospace,monospace";
      code.onclick = () => code.select();
      box.appendChild(code);
      const copy = document.createElement("button");
      copy.textContent = "copy code";
      copy.onclick = () => {
        code.select();
        void navigator.clipboard?.writeText(code.value).catch(() => document.execCommand("copy"));
        copy.textContent = "copied";
      };
      rowOf(copy, btn("run it again", () => this.start()), btn("close", () => this.stop()));
      return box;
    }
    const t = TESTS[s.i];
    title.textContent = `Eye test ${s.i + 1} of ${TESTS.length}`;
    text(`Pay attention to: ${t.focus}`);
    rowOf(
      btn("drive Option 1", () => this.drive(1), s.driving === 1, "eye-opt-1"),
      btn("drive Option 2", () => this.drive(2), s.driving === 2, "eye-opt-2"),
    );
    text("Which felt better?", "margin-top:6px");
    rowOf(
      btn("Option 1", () => this.pick(1), false, "eye-pick-1"),
      btn("Option 2", () => this.pick(2), false, "eye-pick-2"),
      btn("same / can't tell", () => this.pick(0), false, "eye-pick-0"),
    );
    rowOf(btn("stop the test", () => this.stop()));
    return box;
  }
}
