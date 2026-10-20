// Drift Lab W (lead/drift-lab-w): deliberately PLAIN keyboard driving, to A/B against the
// reference (docs/reference/wreckfest-keyboard.md). Keys reach the sim raw (−1 / 0 / +1,
// throttle 0 / 1); the rack slews at a constant rate (wp_vehicle::plain: full lock in t_full,
// back to centre in t_return, lock shrinking with speed by the speed-sensitivity curve). No
// drift intent modes, no throttle shaping, TCS / ESC off, manual gears. Same course as Drift
// Lab C. The four reference-parity test cars (data/cars/parity/) are selectable here only.
import type * as THREE from "three";
import { buildCourse } from "./drift_lab_b";
import { setRawKeyboard } from "./input";
import type { sim as Sim } from "./wasm/sim";

/** Scenario name of the lab (sim.scenario().name). */
export const DRIFT_LAB_W = "drift_lab_w";
/** Lab title. */
export const DRIFT_LAB_W_TITLE = "Drift Lab W";
/** The lab's default car: the reference clip-1 drift car. */
export const DRIFT_LAB_W_CAR = "parity_rwd_d";
/** Roster cars offered next to the parity cars (the Drift Lab C comparison pair). */
const ROSTER_PICKS = ["shiro_turbo", "rampart_50"];

/** Free-drive (flat world) scenario JSON turned into the lab: no traffic / parked cars /
 * police, noon. */
export function driftLabWScenario(freeDriveJson: string): string {
  const sc = JSON.parse(freeDriveJson);
  return JSON.stringify({
    ...sc,
    name: DRIFT_LAB_W,
    time_of_day_h: 12,
    time_ratio: 1,
    traffic_cars: false,
    parked_cars: false,
    police_cars: false,
    traffic_density: 0,
    police_presence: 0,
    weather: null,
  });
}

const FIELDS: Record<string, { label: string; min: number; max: number; step: number; scale?: number; unit?: string; hint: string }> = {
  t_full_s: { label: "full lock time", min: 0.05, max: 0.8, step: 0.01, unit: "s", hint: "centre → full lock with a key held (reference ≈ 0.28 s)" },
  t_return_s: { label: "return time", min: 0.05, max: 0.8, step: 0.01, unit: "s", hint: "full lock → centre after release (reference ≈ 0.33 s)" },
  speed_sensitivity: { label: "speed sensitivity", min: 0, max: 1, step: 0.05, scale: 100, unit: "%", hint: "less lock at speed (the owner's reference setting: 75 %)" },
  sens_ref_kmh: { label: "sens. ref speed", min: 40, max: 250, step: 1, unit: "km/h", hint: "speed where lock = 1 / (1 + sensitivity) (75 %: 57 % lock)" },
  sens_exponent: { label: "sens. exponent", min: 1, max: 4, step: 0.5, hint: "lock = 1 / (1 + s·(v / ref)^exp); 2 = like the angle a constant-g corner needs" },
  self_align: { label: "self-align", min: 0, max: 1, step: 0.05, scale: 100, unit: "%", hint: "keys released: the rack returns toward the front axle's travel instead of the centre (A/B; default 0)" },
};

const STORE = "wp.driftLabW.steer";
const pct = (x: number) => `${Math.round((x ?? 0) * 100)}%`;

/** Lock fraction at `kmh` for constants `p` (mirrors wp_vehicle::plain::PlainSteer::lock_frac). */
function lockFrac(p: Record<string, number>, kmh: number): number {
  const s = Math.min(1, Math.max(0, p.speed_sensitivity));
  if (s <= 0) return 1;
  const halves = Math.round(Math.min(4, Math.max(1, p.sens_exponent)) * 2);
  const x = kmh / Math.max(3.6, p.sens_ref_kmh);
  return 1 / (1 + s * x ** (halves / 2));
}

/** The lab's panel + readout. `restart(car)` restarts the lab with another car. */
export class DriftLabWPanel {
  readonly root: HTMLDivElement;
  private readonly body: HTMLDivElement;
  private readonly readout: HTMLDivElement;
  private readonly course: THREE.Group;
  private active = false;
  private shown = false;
  private open = true;
  /** The lab's current car id (set by the session start). */
  car = DRIFT_LAB_W_CAR;
  private prevTitle = "";

  constructor(
    parent: HTMLElement,
    private readonly sim: typeof Sim,
    private readonly world: THREE.Object3D,
    private readonly player: () => number,
    private readonly restart: (car: string) => void,
  ) {
    this.course = buildCourse();
    this.root = document.createElement("div");
    this.root.id = "drift-lab-w";
    this.root.style.cssText =
      "position:absolute;left:272px;top:196px;z-index:30;font:12px/1.35 system-ui,sans-serif;color:#eee;pointer-events:auto;max-width:min(330px,calc(100vw - 24px));display:none";
    const btn = document.createElement("button");
    btn.textContent = "tune";
    btn.title = `${DRIFT_LAB_W_TITLE}: plain steering constants and the test car`;
    btn.onclick = () => {
      this.open = !this.open;
      this.body.style.display = this.open ? "block" : "none";
    };
    this.readout = document.createElement("div");
    this.readout.style.cssText =
      "background:rgba(0,0,0,.6);padding:6px 8px;border-radius:6px;margin-top:4px;font-variant-numeric:tabular-nums;white-space:pre;border-left:4px solid #40a0e0";
    this.body = document.createElement("div");
    this.body.style.cssText = "background:rgba(0,0,0,.72);padding:8px;border-radius:6px;margin-top:4px;max-height:60vh;overflow:auto";
    this.root.append(btn, this.readout, this.body);
    parent.appendChild(this.root);
  }

  /** Whether the lab session is running: plain steering + raw keys on, course shown. */
  setActive(on: boolean): void {
    if (on === this.active) {
      if (on) this.build();
      return;
    }
    this.active = on;
    this.root.style.display = on ? "block" : "none";
    if (on && !this.shown) this.world.add(this.course);
    if (!on && this.shown) this.world.remove(this.course);
    this.shown = on;
    setRawKeyboard(on);
    if (on) this.prevTitle = document.title;
    document.title = on ? DRIFT_LAB_W_TITLE : this.prevTitle;
    if (on) {
      let saved = "";
      try {
        saved = localStorage.getItem(STORE) ?? "";
      } catch {
        /* storage blocked: defaults */
      }
      try {
        this.sim.plainSteerSet(saved || JSON.stringify(this.sim.plainSteerDefaults()));
      } catch {
        this.sim.plainSteerSet(JSON.stringify(this.sim.plainSteerDefaults()));
      }
      this.build();
    } else {
      this.sim.plainSteerSet("");
    }
  }

  /** The lab's assists (call after any assist preset is applied): the owner's reference
   * settings — TCS off, ESC off, ABS on, manual gears (automatic clutch), no pedal filters, no
   * countersteer help (it is bypassed by plain steering anyway). */
  applyAssists(): void {
    if (!this.active) return;
    const id = this.player();
    const a = this.sim.assists(id);
    this.sim.setAssists(id, {
      ...a,
      abs: true,
      tcs: false,
      esc: false,
      countersteer_help: 0,
      throttle_filter: 0,
      brake_filter: 0,
      auto_gears: false,
      auto_clutch: true,
    });
  }

  private save(p: Record<string, number>): void {
    const text = JSON.stringify(p);
    this.sim.plainSteerSet(text);
    try {
      localStorage.setItem(STORE, text);
    } catch {
      /* ignore */
    }
  }

  private build(): void {
    const p = this.sim.plainSteer() ?? this.sim.plainSteerDefaults();
    const defaults = this.sim.plainSteerDefaults();
    this.body.replaceChildren();
    const row = (label: string, hint: string, input: HTMLElement, val: HTMLSpanElement) => {
      const l = document.createElement("label");
      l.title = hint;
      l.style.cssText = "display:grid;grid-template-columns:1fr 110px 54px;gap:6px;align-items:center;margin:2px 0";
      const s = document.createElement("span");
      s.textContent = label;
      input.style.width = "110px";
      l.append(s, input, val);
      this.body.appendChild(l);
    };
    // Car selector: the parity cars, then the roster comparison cars.
    {
      const sel = document.createElement("select");
      const current = this.car;
      for (const id of [...this.sim.parityCarIds(), ...ROSTER_PICKS]) sel.add(new Option(id, id));
      if (current) sel.value = current;
      sel.onchange = () => this.restart(sel.value);
      row("car", "reference-parity test cars (parity_*) and roster cars; restarts the lab", sel, document.createElement("span"));
    }
    for (const [key, f] of Object.entries(FIELDS)) {
      const value = p[key] ?? defaults[key];
      const input = document.createElement("input");
      input.type = "range";
      input.min = String(f.min);
      input.max = String(f.max);
      input.step = String(f.step);
      input.value = String(value);
      const val = document.createElement("span");
      const show = () => {
        const v = Number(input.value) * (f.scale ?? 1);
        val.textContent = `${Number(v.toFixed(2))}${f.unit ? ` ${f.unit}` : ""}`;
        val.style.color = Math.abs(Number(input.value) - defaults[key]) > 1e-6 ? "#ffd27a" : "";
      };
      show();
      input.oninput = () => {
        show();
        this.save({ ...(this.sim.plainSteer() ?? defaults), [key]: Number(input.value) });
        this.curve();
      };
      row(f.label, f.hint, input, val);
    }
    const curve = document.createElement("div");
    curve.id = "drift-lab-w-curve";
    curve.style.cssText = "margin-top:4px;color:#bbb;font-variant-numeric:tabular-nums";
    this.body.appendChild(curve);
    this.curve();
    const actions = document.createElement("div");
    actions.style.cssText = "display:flex;gap:6px;margin-top:6px";
    const reset = document.createElement("button");
    reset.textContent = "defaults";
    reset.onclick = () => {
      try {
        localStorage.removeItem(STORE);
      } catch {
        /* ignore */
      }
      this.sim.plainSteerSet(JSON.stringify(defaults));
      this.build();
    };
    const copy = document.createElement("button");
    copy.textContent = "copy JSON";
    copy.title = "copy the current constants (paste them into the owner's notes)";
    copy.onclick = () => void navigator.clipboard?.writeText(JSON.stringify(this.sim.plainSteer(), null, 1));
    actions.append(reset, copy);
    this.body.appendChild(actions);
  }

  /** The lock-vs-speed line under the sliders. */
  private curve(): void {
    const el = this.body.querySelector("#drift-lab-w-curve");
    const p = this.sim.plainSteer();
    if (!el || !p) return;
    el.textContent = "lock: " + [20, 60, 100, 140, 160].map((k) => `${k} ${pct(lockFrac(p, k))}`).join(" · ");
  }

  /** Update the readout (cheap: one small typed array). */
  frame(): void {
    if (!this.active) return;
    let r: Float32Array;
    try {
      r = this.sim.plainReadout(this.player());
    } catch {
      return;
    }
    const key = r[0] > 0.5 ? "RIGHT" : r[0] < -0.5 ? "LEFT" : "–";
    const rack = Number.isNaN(r[1]) ? "off" : `${r[1] >= 0 ? "R" : "L"} ${Math.abs(Math.round(r[1] * 100))}%`;
    const gear = r[6] < 0 ? "R" : r[6] === 0 ? "N" : String(r[6]);
    const slide = Math.abs(r[3]);
    this.readout.textContent =
      `${DRIFT_LAB_W_TITLE} · plain steering · TCS off · ESC off · manual\n` +
      `key ${key.padEnd(5)} rack ${rack.padEnd(7)} lock ${Number.isNaN(r[2]) ? "–" : pct(r[2])} · wheel ${r[8].toFixed(1)}°\n` +
      `slide ${slide.toFixed(1)}° · throttle key ${pct(r[4])} → engine ${pct(r[5])}\n` +
      `gear ${gear} · ${r[7].toFixed(0)} km/h`;
    this.readout.style.borderLeftColor = slide > 50 ? "#e04040" : slide > 10 ? "#40c070" : "#40a0e0";
  }
}
