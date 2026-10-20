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

const RIDE_FIELDS: Record<string, { label: string; min: number; max: number; step: number; unit?: string; hint: string }> = {
  spring: { label: "springs ×", min: 0.5, max: 1.5, step: 0.05, hint: "spring rate scale, all corners (below ~0.85 some cars sit near the bump stops)" },
  arb: { label: "anti-roll bars ×", min: 0, max: 1.5, step: 0.05, hint: "anti-roll bar scale, both axles (front / rear balance kept)" },
  damper_bump: { label: "dampers bump ×", min: 0.5, max: 3, step: 0.05, hint: "damper scale, compression" },
  damper_rebound: { label: "dampers rebound ×", min: 0.5, max: 3, step: 0.05, hint: "damper scale, extension" },
  roll_centre_m: { label: "roll centre", min: 0, max: 0.3, step: 0.01, unit: "m", hint: "height where the tyres' side force acts on the body (0 = contact patch, the data); higher = less roll" },
  pitch_centre_m: { label: "pitch centre", min: 0, max: 0.3, step: 0.01, unit: "m", hint: "height where drive / brake forces act on the body (anti-dive / anti-squat feel)" },
};

const CAR_FIELDS: Record<string, { label: string; min: number; max: number; step: number; unit?: string; hint: string }> = {
  lat_under_spin: { label: "side grip under spin", min: 0, max: 1, step: 0.05, hint: "how much wheelspin costs a tyre's side force (1 = the data's friction ellipse; lower = a spinning rear keeps more side grip). Drive force unchanged" },
  rear_peak_slip: { label: "rear peak slip ×", min: 0.5, max: 3, step: 0.05, hint: "rear tyres' lateral peak slip angle (curve stretched: later, softer peak)" },
  front_peak_slip: { label: "front peak slip ×", min: 0.5, max: 3, step: 0.05, hint: "front tyres' lateral peak slip angle" },
  lock: { label: "steering lock ×", min: 0.6, max: 1.6, step: 0.05, hint: "× the car's max steer angle" },
  torque: { label: "engine torque ×", min: 0.3, max: 1.5, step: 0.05, hint: "engine torque scale" },
  spin_b0: { label: "spin model: B0", min: 0.5, max: 20, step: 0.05, hint: "tyre-slip spin model (tick 'spin model'): how fast wheelspin removes side force at small slip angles (higher = power oversteer starts sooner)" },
  spin_b2: { label: "spin model: B2", min: 0.5, max: 8, step: 0.01, unit: "/rad", hint: "1 / slip angle where a spinning tyre gets its side force back (1.63 ≈ 35°; lower = later)" },
  spin_p: { label: "spin model: sharpness p", min: 1, max: 8, step: 1, hint: "how sharply the side force comes back with slip angle" },
  spin_c: { label: "spin model: C", min: 0.3, max: 1, step: 0.01, hint: "weighting shape; below 1 a spinning tyre always keeps some side force" },
  spin_fx_slide: { label: "spin model: drive grip spinning", min: 0, max: 1, step: 0.01, hint: "fraction of peak drive force a spinning tyre keeps (0 = the tyre data's curve, ~0.55)" },
  spin_fx_yield: { label: "spin model: ellipse cap", min: 0, max: 1, step: 0.05, hint: "how much drive force yields to stay inside the friction ellipse (0 = MF-style, no cap; 1 = hard cap)" },
  rear_relax: { label: "rear relaxation ×", min: 0.1, max: 5, step: 0.05, hint: "rear tyres' relaxation length (slip-angle lag)" },
  yaw_damping: { label: "yaw damping (diag)", min: 0, max: 2, step: 0.05, unit: "/s", hint: "NOT physical: a yaw damping torque −k·m·L²·r, to see how much damping a held drift needs" },
};

const STORE = "wp.driftLabW.steer";
const CAR_STORE = "wp.driftLabW.car";
const RIDE_STORE = "wp.driftLabW.ride";
const pct = (x: number) => `${Math.round((x ?? 0) * 100)}%`;

/** Lock fraction at `kmh` for constants `p` (mirrors wp_vehicle::plain::PlainSteer::lock_frac). */
function lockFrac(p: Record<string, number>, kmh: number): number {
  if ((p as Record<string, unknown>).sens_applies === "tyre") return 1;
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
    btn.id = "drift-w-tune";
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
      let ride = "";
      try {
        ride = localStorage.getItem(RIDE_STORE) ?? "";
      } catch {
        /* storage blocked: defaults */
      }
      try {
        this.sim.rideTuneSet(ride || JSON.stringify(this.sim.rideTuneDefaults().lab));
      } catch {
        this.sim.rideTuneSet(JSON.stringify(this.sim.rideTuneDefaults().lab));
      }
      let carTune = "";
      try {
        carTune = localStorage.getItem(CAR_STORE) ?? "";
      } catch {
        /* storage blocked: data */
      }
      try {
        this.sim.carTuneSet(carTune || JSON.stringify(this.sim.carTuneDefaults().data));
      } catch {
        this.sim.carTuneSet(JSON.stringify(this.sim.carTuneDefaults().data));
      }
      this.build();
    } else {
      this.sim.plainSteerSet("");
      this.sim.rideTuneSet("");
      this.sim.carTuneSet("");
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
    // Speed sensitivity A/B (clip 8): lock shrinks with speed (rack) vs full lock, tyres limit.
    {
      const sel = document.createElement("select");
      sel.id = "drift-lab-w-sens";
      sel.add(new Option("rack (lock shrinks)", "rack"));
      sel.add(new Option("tyre (full lock)", "tyre"));
      sel.value = String((p as Record<string, unknown>).sens_applies ?? "rack");
      sel.onchange = () => {
        this.save({ ...(this.sim.plainSteer() ?? defaults), sens_applies: sel.value } as unknown as Record<string, number>);
        this.curve();
      };
      row(
        "sensitivity on",
        "rack: the road-wheel lock shrinks with speed (the curve below); tyre: full road-wheel lock at every speed and the front tyres limit the car (clip 8: the reference's wheel turns as far at 150 km/h)",
        sel,
        document.createElement("span"),
      );
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
    // Suspension (ride tuning override, wp_vehicle::ride_lab).
    {
      const h = document.createElement("div");
      h.textContent = "suspension (lab override; × the car data)";
      h.style.cssText = "margin:8px 0 2px;color:#9cf";
      this.body.appendChild(h);
      const rd = this.sim.rideTuneDefaults();
      const rt = this.sim.rideTune() ?? rd.data;
      for (const [key, f] of Object.entries(RIDE_FIELDS)) {
        const input = document.createElement("input");
        input.type = "range";
        input.min = String(f.min);
        input.max = String(f.max);
        input.step = String(f.step);
        input.value = String(rt[key] ?? rd.data[key]);
        const val = document.createElement("span");
        const show = () => {
          val.textContent = `${Number(Number(input.value).toFixed(2))}${f.unit ? ` ${f.unit}` : ""}`;
          val.style.color = Math.abs(Number(input.value) - rd.lab[key]) > 1e-6 ? "#ffd27a" : "";
        };
        show();
        input.oninput = () => {
          show();
          const text = JSON.stringify({ ...(this.sim.rideTune() ?? rd.data), [key]: Number(input.value) });
          this.sim.rideTuneSet(text);
          try {
            localStorage.setItem(RIDE_STORE, text);
          } catch {
            /* ignore */
          }
        };
        row(f.label, f.hint, input, val);
      }
      const rideBtns = document.createElement("div");
      rideBtns.style.cssText = "display:flex;gap:6px;margin-top:4px";
      for (const [label, which] of [["lab default", "lab"], ["suspension data", "data"]] as const) {
        const b = document.createElement("button");
        b.textContent = label;
        b.title = which === "lab" ? "calmer, slower weight transfer (springs 0.9, bars 0.25, dampers 1.4 / 1.8)" : "no scaling: the car files as they are";
        b.onclick = () => {
          const text = JSON.stringify(this.sim.rideTuneDefaults()[which]);
          this.sim.rideTuneSet(text);
          try {
            localStorage.setItem(RIDE_STORE, text);
          } catch {
            /* ignore */
          }
          this.build();
        };
        rideBtns.appendChild(b);
      }
      this.body.appendChild(rideBtns);
    }
    // Car / tyre experiments (wp_vehicle::car_lab).
    {
      const h = document.createElement("div");
      h.textContent = "car / tyre experiments (lab override)";
      h.style.cssText = "margin:8px 0 2px;color:#9cf";
      this.body.appendChild(h);
      const cd = this.sim.carTuneDefaults();
      const ct = this.sim.carTune() ?? cd.data;
      const saveCar = (t: Record<string, number | boolean>) => {
        const text = JSON.stringify(t);
        this.sim.carTuneSet(text);
        try {
          localStorage.setItem(CAR_STORE, text);
        } catch {
          /* ignore */
        }
      };
      for (const [key, f] of Object.entries(CAR_FIELDS)) {
        const input = document.createElement("input");
        input.type = "range";
        input.min = String(f.min);
        input.max = String(f.max);
        input.step = String(f.step);
        input.value = String(ct[key] ?? cd.data[key]);
        const val = document.createElement("span");
        const show = () => {
          val.textContent = `${Number(Number(input.value).toFixed(2))}${f.unit ? ` ${f.unit}` : ""}`;
          val.style.color = Math.abs(Number(input.value) - Number(cd.data[key])) > 1e-6 ? "#ffd27a" : "";
        };
        show();
        input.oninput = () => {
          show();
          saveCar({ ...(this.sim.carTune() ?? cd.data), [key]: Number(input.value) });
        };
        row(f.label, f.hint, input, val);
      }
      for (const [key, label, hint] of [
        ["rear_locked", "rear diff locked", "lock the rear differential (else the car's own)"],
        ["rear_open", "rear diff open", "open rear differential (else the car's own)"],
        ["combined_rear_only", "side grip: rear only", "side grip under spin / the spin model apply to the rear tyres only (else all four)"],
        ["spin_model", "spin model (tyre-slip)", "combined slip where a spinning tyre's side force comes back as the slip angle grows (off = the data's friction ellipse)"],
        ["spin_on_braking", "spin model: braking too", "also apply the spin model to braking / locked wheels"],
        ["aligning_on_body", "aligning moment on body", "apply the tyres' self-aligning moment to the body as yaw torque (data: force feedback only)"],
      ] as const) {
        const input = document.createElement("input");
        input.type = "checkbox";
        input.checked = Boolean(ct[key] ?? cd.data[key]);
        input.onchange = () => saveCar({ ...(this.sim.carTune() ?? cd.data), [key]: input.checked });
        row(label, hint, input, document.createElement("span"));
      }
      const btns = document.createElement("div");
      btns.style.cssText = "display:flex;gap:6px;margin-top:4px";
      for (const [label, which, hint] of [
        ["tyre data", "data", "no change: the car and tyre files as they are"],
        ["spin grip + damper (test)", "spin_damper", "step-4 test set (side grip under spin 0.15, lock 0.9, yaw damping 0.2; the damper is not physical). It does NOT hold the drift: the car swings through to the other side"],
        ["spin model", "spin", "tyre-slip spin model on the rears, no damper (holds ~41–44° on the lab's standard entry; tuned to a knife edge)"],
      ] as const) {
        const b = document.createElement("button");
        b.textContent = label;
        b.title = hint;
        b.onclick = () => {
          saveCar(this.sim.carTuneDefaults()[which]);
          this.build();
        };
        btns.appendChild(b);
      }
      this.body.appendChild(btns);
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
    copy.title = "copy the steering + suspension constants (paste them into the owner's notes)";
    copy.onclick = () => void navigator.clipboard?.writeText(JSON.stringify({ steer: this.sim.plainSteer(), ride: this.sim.rideTune(), car: this.sim.carTune() }, null, 1));
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
    let ride = "";
    try {
      const m = this.sim.rideReadout(this.player());
      ride = `\nroll ${m[5].toFixed(1)}° · roll ${m[0].toFixed(2)} Hz ζ ${m[1].toFixed(2)} (${m[2].toFixed(1)}°/g) · pitch ${m[3].toFixed(2)} Hz ζ ${m[4].toFixed(2)}`;
    } catch {
      /* readout unavailable */
    }
    this.readout.textContent =
      `${DRIFT_LAB_W_TITLE} · plain steering · TCS off · ESC off · manual\n` +
      `key ${key.padEnd(5)} rack ${rack.padEnd(7)} lock ${Number.isNaN(r[2]) ? "–" : pct(r[2])} · wheel ${r[8].toFixed(1)}°\n` +
      `slide ${slide.toFixed(1)}° · throttle key ${pct(r[4])} → engine ${pct(r[5])}\n` +
      `gear ${gear} · ${r[7].toFixed(0)} km/h` +
      ride;
    this.readout.style.borderLeftColor = slide > 50 ? "#e04040" : slide > 10 ? "#40c070" : "#40a0e0";
  }
}
