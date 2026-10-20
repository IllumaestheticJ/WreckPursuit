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
  align_gain: { label: "VR: align gain", min: 0, max: 1.2, step: 0.05, hint: "velocity-relative steering: how far the wheels follow the front axle's travel in a slide (1 = along it)" },
  t_align_s: { label: "VR: slide align time", min: 0.05, max: 0.8, step: 0.01, unit: "s", hint: "velocity-relative steering: in a slide the wheels move at least full lock in this time" },
  slide_key_gain: { label: "VR: key in slide ×", min: 0, max: 1, step: 0.05, hint: "velocity-relative steering: the key's offset from the travel in a full slide, × lock(speed)" },
  self_align: { label: "self-align", min: 0, max: 1, step: 0.05, scale: 100, unit: "%", hint: "keys released: the rack returns toward the front axle's travel instead of the centre (A/B; default 0)" },
};

const RIDE_FIELDS: Record<string, { label: string; min: number; max: number; step: number; unit?: string; hint: string }> = {
  spring: { label: "springs ×", min: 0.5, max: 1.5, step: 0.05, hint: "spring rate scale, all corners (below ~0.85 some cars sit near the bump stops)" },
  arb: { label: "anti-roll bars ×", min: 0, max: 1.5, step: 0.05, hint: "anti-roll bar scale, both axles (front / rear balance kept)" },
  damper_bump: { label: "dampers bump ×", min: 0.5, max: 3, step: 0.05, hint: "damper scale, compression" },
  damper_rebound: { label: "dampers rebound ×", min: 0.5, max: 3, step: 0.05, hint: "damper scale, extension" },
  roll_centre_m: { label: "roll centre", min: 0, max: 0.3, step: 0.01, unit: "m", hint: "height where the tyres' side force acts on the body (0 = contact patch, the data); higher = less roll" },
  pitch_centre_m: { label: "pitch centre", min: 0, max: 0.3, step: 0.01, unit: "m", hint: "height where drive / brake forces act on the body (anti-dive / anti-squat feel)" },
  travel: { label: "suspension travel ×", min: 0.8, max: 2.5, step: 0.05, hint: "more travel = more room before the bump stops when the body rolls further" },
  roll_inertia: { label: "roll inertia × (restarts)", min: 0.5, max: 4, step: 0.1, hint: "body roll inertia (Ixx): slower, heavier roll; applied when the car is built — the lab restarts the car" },
  pitch_inertia: { label: "pitch inertia × (restarts)", min: 0.5, max: 4, step: 0.1, hint: "body pitch inertia (Iyy): slower dive / squat; restarts the car" },
  yaw_inertia: { label: "yaw inertia × (restarts)", min: 0.5, max: 4, step: 0.1, hint: "body yaw inertia (Izz): slower heading build; restarts the car" },
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
  spin_fx_slide_from_deg: { label: "spin model: drive grip from", min: 0, max: 30, step: 0.5, unit: "°", hint: "slip angle where the spinning drive-grip floor starts (straight-line launches below it keep the tyre data's curve)" },
  spin_fx_slide_full_deg: { label: "spin model: drive grip full at", min: 0, max: 45, step: 0.5, unit: "°", hint: "slip angle where the drive-grip floor is fully on" },
  side_floor: { label: "side grip floor (spinning)", min: 0, max: 1, step: 0.05, hint: "SHIPPED DEFAULT 0.4: a tyre spinning far past its peak slip ratio keeps at least this share of its side force (fixes spinning off the line on cambered roads); 0 = the old friction ellipse" },
  drift_lock_extra: { label: "drift lock ±", min: -0.7, max: 1, step: 0.01, hint: "extra (or, negative, less) steering lock once the body slip passes 'drift lock from' (ramped over 10°); grip driving below it is unchanged. 0 = off" },
  drift_lock_from_deg: { label: "drift lock from", min: 0, max: 45, step: 0.5, unit: "°", hint: "body slip where the drift lock change starts" },
  front_deep_falloff: { label: "front deep-slip falloff", min: 0, max: 0.9, step: 0.01, hint: "fraction of front side force lost at large front slip angles (between the two angles below); grip driving untouched. 0 = off" },
  front_deep_from_deg: { label: "front falloff from", min: 0, max: 40, step: 0.5, unit: "°", hint: "front slip angle where the falloff starts" },
  front_deep_full_deg: { label: "front falloff full at", min: 1, max: 60, step: 0.5, unit: "°", hint: "front slip angle where the falloff is complete" },
  front_lat_c: { label: "front MF C", min: 0, max: 2, step: 0.01, hint: "front tyres' lateral shape factor C (0 = data; higher = steeper falloff past the peak; cornering stiffness kept). Changes grip driving" },
  rack_compliance: { label: "rack compliance", min: 0, max: 80, step: 1, unit: "°/kN·m", hint: "compliant rack / column: road wheels yield to the front tyres' steering-axis moment (aligning + caster trail × side force) by this many degrees per kN·m (0 = rigid rack, the data)" },
  rack_compliance_tau_s: { label: "rack compliance lag", min: 0.01, max: 0.5, step: 0.01, unit: "s", hint: "lag of the rack compliance (column / hands)" },
  spin_fx_yield: { label: "spin model: ellipse cap", min: 0, max: 1, step: 0.05, hint: "how much drive force yields to stay inside the friction ellipse (0 = MF-style, no cap; 1 = hard cap)" },
  rear_relax: { label: "rear relaxation ×", min: 0.1, max: 5, step: 0.05, hint: "rear tyres' relaxation length (slip-angle lag)" },
  yaw_damping: { label: "yaw damping (diag)", min: 0, max: 2, step: 0.05, unit: "/s", hint: "NOT physical: a yaw damping torque −k·m·L²·r, to see how much damping a held drift needs" },
};

const STORE = "wp.driftLabW.steer";
// v2: the lab's default became the spin model (launches straight on the map's road crown); an
// older saved "tyre data" must not override it.
const CAR_STORE = "wp.driftLabW.car.v2";
const TCS_STORE = "wp.driftLabW.tcs";
const RIDE_STORE = "wp.driftLabW.ride";
const GRIP_STORE = "wp.driftLabW.grip";

/** Grip-balance lab sliders (wp_vehicle::grip_lab::GripBalance). */
const GRIP_FIELDS: Record<string, { label: string; min: number; max: number; step: number; scale?: number; unit?: string; hint: string }> = {
  front_mu: { label: "front grip (μ) ×", min: 0.7, max: 1.3, step: 0.01, hint: "front tyres' peak friction" },
  rear_mu: { label: "rear grip (μ) ×", min: 0.7, max: 1.3, step: 0.01, hint: "rear tyres' peak friction (less = looser at the limit)" },
  front_stiffness: { label: "front cornering stiffness ×", min: 0.5, max: 2, step: 0.05, hint: "front lateral curve squeezed along the slip axis: same peak, reached at a smaller slip angle" },
  rear_stiffness: { label: "rear cornering stiffness ×", min: 0.5, max: 2, step: 0.05, hint: "rear: less = bigger rear slip angle (body slip) but less high-speed stability" },
  front_load_sens: { label: "front load sensitivity ×", min: 0, max: 3, step: 0.1, hint: "more = a loaded outside front loses more μ (push)" },
  rear_load_sens: { label: "rear load sensitivity ×", min: 0, max: 3, step: 0.1, hint: "more = a loaded outside rear loses more μ (looser at the limit)" },
  front_relax: { label: "front relaxation ×", min: 0.2, max: 3, step: 0.05, hint: "front tyres' slip-angle lag" },
  rear_lat_c: { label: "rear past-peak shape C", min: 0, max: 2, step: 0.05, hint: "0 = data (1.4); lower = side force falls less past the peak (cornering stiffness kept)" },
  front_arb: { label: "front bar ×", min: 0, max: 3, step: 0.05, hint: "front anti-roll bar, on top of the suspension section (less = more front grip)" },
  rear_arb: { label: "rear bar ×", min: 0, max: 30, step: 0.5, hint: "rear anti-roll bar, on top of the suspension section (more = looser); the data's rear bar is ~5 % of the front's" },
  front_downforce: { label: "front downforce C·A", min: 0, max: 2, step: 0.05, unit: "m²", hint: "extra front downforce (½ρv² · C·A at the front axle)" },
  rear_downforce: { label: "rear downforce C·A", min: 0, max: 2, step: 0.05, unit: "m²", hint: "extra rear downforce: grows with v², stabilises the rear at speed without touching low-speed balance (0.8 ≈ 0.35 kN at 60 mph, 1 kN at 100)" },
  probe_width: { label: "wheel probe width ×", min: 0, max: 1.5, step: 0.0125, hint: "ground probe cylinder width (1 = the vehicle's). It rolls with the body, so in a corner it touches on its edge and the tyre load chatters tick to tick; 0.0125 removes the chatter (A/B for the vehicle owner)" },
  smart_ay_g: { label: "smart lock: corner g", min: 0, max: 2, step: 0.05, unit: "g", hint: "steady-corner steer angle allowed on top of the front peak slip (L·a/v²)" },
  smart_margin: { label: "smart lock: past peak", min: -0.8, max: 2, step: 0.05, scale: 100, unit: "%", hint: "front slip allowed past the tyre's peak slip angle" },
  smart_margin_hi: { label: "smart lock: past peak at speed", min: -0.8, max: 2, step: 0.05, scale: 100, unit: "%", hint: "the past-peak allowance at high speed (blended in between the two speeds below): fronts a little past their peak keep a held key stable at 100 mph" },
  smart_hi_from_kmh: { label: "smart lock: speed blend from", min: 40, max: 200, step: 5, unit: "km/h", hint: "speed where the high-speed allowance starts to blend in" },
  smart_hi_full_kmh: { label: "smart lock: speed blend to", min: 60, max: 250, step: 5, unit: "km/h", hint: "speed where the high-speed allowance is fully in" },
  smart_slide_from_deg: { label: "smart lock off from slip", min: 0, max: 30, step: 0.5, unit: "°", hint: "body slip where the plain lock starts to come back (slides steer as before)" },
  smart_slide_full_deg: { label: "smart lock off by slip", min: 0, max: 40, step: 0.5, unit: "°", hint: "body slip where the plain lock is fully back" },
};
const pct = (x: number) => `${Math.round((x ?? 0) * 100)}%`;

/** Whether two grip-balance sets are the same (numbers within 1e-6; JSON key order and f32
 * round-trips do not matter). */
function sameGrip(a: Record<string, unknown> | null, b: Record<string, unknown>): boolean {
  if (!a) return false;
  return Object.keys(b).every((k) =>
    typeof b[k] === "number" ? Math.abs(Number(a[k]) - Number(b[k])) < 1e-6 : a[k] === b[k],
  );
}

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
        this.sim.carTuneSet(carTune || JSON.stringify(this.sim.carTuneDefaults().spin));
      } catch {
        this.sim.carTuneSet(JSON.stringify(this.sim.carTuneDefaults().spin));
      }
      let grip = "";
      try {
        grip = localStorage.getItem(GRIP_STORE) ?? "";
      } catch {
        /* storage blocked: the new balance */
      }
      try {
        this.sim.gripBalanceSet(grip || JSON.stringify(this.sim.gripBalanceDefaults().new));
      } catch {
        this.sim.gripBalanceSet(JSON.stringify(this.sim.gripBalanceDefaults().new));
      }
      this.build();
    } else {
      this.sim.plainSteerSet("");
      this.sim.rideTuneSet("");
      this.sim.carTuneSet("");
      this.sim.gripBalanceSet("");
    }
  }

  /** TCS on (the existing on/off assist) — the panel checkbox, default off (the owner's
   * reference) until the Off / Half / Full levels land. */
  private tcs(): boolean {
    try {
      return localStorage.getItem(TCS_STORE) === "1";
    } catch {
      return false;
    }
  }

  /** The lab's assists (call after any assist preset is applied): the owner's reference
   * settings — TCS off (panel checkbox), ESC off, ABS on, manual gears (automatic clutch), no pedal filters, no
   * countersteer help (it is bypassed by plain steering anyway). */
  applyAssists(): void {
    if (!this.active) return;
    const id = this.player();
    const a = this.sim.assists(id);
    this.sim.setAssists(id, {
      ...a,
      abs: true,
      tcs: this.tcs(),
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
    // Velocity-relative steering (clip 8 cockpit): A/B, default off (step 7: it over-catches).
    {
      const input = document.createElement("input");
      input.type = "checkbox";
      input.id = "drift-lab-w-vr";
      input.checked = Boolean((p as Record<string, unknown>).velocity_relative);
      input.onchange = () =>
        this.save({ ...(this.sim.plainSteer() ?? defaults), velocity_relative: input.checked } as unknown as Record<string, number>);
      row(
        "velocity-relative steering",
        "clip 8: in a slide the wheels follow the front axle's travel and the key adds lock on top (A/B; measured: a strong catch that swings through — see drift-lab-w.md step 7)",
        input,
        document.createElement("span"),
      );
    }
    // TCS (the existing on/off assist) until Off / Half / Full lands.
    {
      const input = document.createElement("input");
      input.type = "checkbox";
      input.id = "drift-lab-w-tcs";
      input.checked = this.tcs();
      input.onchange = () => {
        try {
          localStorage.setItem(TCS_STORE, input.checked ? "1" : "0");
        } catch {
          /* ignore */
        }
        this.applyAssists();
      };
      row("TCS", "traction control on / off (the reference's Off / Half / Full comes later); default off as the owner drives", input, document.createElement("span"));
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
        // Body inertia is set when the car's body is built: restart the lab on release.
        if (key.endsWith("_inertia")) input.onchange = () => this.restart(this.car);
        row(f.label, f.hint, input, val);
      }
      const rideBtns = document.createElement("div");
      rideBtns.style.cssText = "display:flex;gap:6px;margin-top:4px";
      {
        const input = document.createElement("input");
        input.type = "checkbox";
        input.id = "drift-lab-w-keep-height";
        input.checked = Boolean((rt as Record<string, unknown>).keep_ride_height);
        input.onchange = () => {
          const text = JSON.stringify({ ...(this.sim.rideTune() ?? rd.data), keep_ride_height: input.checked });
          this.sim.rideTuneSet(text);
          try {
            localStorage.setItem(RIDE_STORE, text);
          } catch {
            /* ignore */
          }
        };
        row("keep ride height", "raise the spring preload so softer springs keep the data's static ride height (off the bump stops)", input, document.createElement("span"));
      }
      for (const [label, which] of [["lab default", "lab"], ["slow weight (clip 4)", "slow_weight"], ["suspension data", "data"]] as const) {
        const b = document.createElement("button");
        b.textContent = label;
        b.title =
          which === "lab"
            ? "calmer, slower weight transfer (springs 0.9, bars 0.25, dampers 1.4 / 1.8)"
            : which === "slow_weight"
              ? "reference clip 4: roll builds over ~0.6 s to ~6°, no overshoot (soft springs, ride height kept, more travel and inertia). Breaks the spin model's knife-edge held drift. Restarts the car"
              : "no scaling: the car files as they are";
        b.onclick = () => {
          const text = JSON.stringify(this.sim.rideTuneDefaults()[which]);
          this.sim.rideTuneSet(text);
          try {
            localStorage.setItem(RIDE_STORE, text);
          } catch {
            /* ignore */
          }
          // The inertia scales apply when the car's body is built.
          this.restart(this.car);
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
        ["rack_yield_to_centre_only", "rack yields to centre only", "the compliant rack only gives way toward the centre, never past the key's angle"],
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
        ["spin + drift lock", "spin_lock", "tyre-slip step 3: spin model + 50 % less counter-lock past 20° of slip + front deep-slip falloff (holds 15 / 27 entries, as a slow 27–54° swing)"],
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
      {
        // Tyres + steering together (tyre-slip): spin model + velocity-relative steering at half gain.
        const b = document.createElement("button");
        b.textContent = "drift: spin + VR (test)";
        b.title =
          "tyre-slip test preset: spin model tyres + velocity-relative steering (gain 0.5, fade 7→17°, align 0.26 s, slide key 0.31). Holds 13 / 27 test entries in the 30–45° band; not steady (speed bleeds). Sets the steering too: use the steering defaults button to undo";
        b.onclick = () => {
          const d = this.sim.carTuneDefaults();
          saveCar(d.spin);
          this.save(d.spin_vr_plain);
          this.build();
        };
        btns.appendChild(b);
      }
      this.body.appendChild(btns);
    }
    // Grip-driving balance (wp_vehicle::grip_lab; docs/integration/grip-balance.md).
    {
      const h = document.createElement("div");
      h.textContent = "grip balance (lab override)";
      h.style.cssText = "margin:8px 0 2px;color:#9cf";
      this.body.appendChild(h);
      const gd = this.sim.gripBalanceDefaults();
      const gt = this.sim.gripBalance() ?? gd.before;
      const saveGrip = (t: Record<string, number | boolean | string>) => {
        const text = JSON.stringify(t);
        this.sim.gripBalanceSet(text);
        try {
          localStorage.setItem(GRIP_STORE, text);
        } catch {
          /* ignore */
        }
      };
      const isNew = sameGrip(gt, gd.new);
      const ab = document.createElement("button");
      ab.id = "drift-w-balance-ab";
      ab.textContent = `balance: ${isNew ? "new" : sameGrip(gt, gd.before) ? "before" : sameGrip(gt, gd.lock_only) ? "lock only" : sameGrip(gt, gd.step2) ? "step 2" : sameGrip(gt, gd.slow_weight) ? "for slow weight" : "custom"}`;
      ab.title = "A/B: before = the tyre data and the plain lock; new = the smart lock + softer-sidewall tyres on RWD cars + rear downforce + probe fix (the car rotates at the limit, stable at 100 mph)";
      ab.onclick = () => {
        // new → before; anything else (before, lock only, custom) → new.
        saveGrip(sameGrip(this.sim.gripBalance(), gd.new) ? gd.before : gd.new);
        this.build();
      };
      const lockOnly = document.createElement("button");
      lockOnly.textContent = "lock only (step 1)";
      lockOnly.title = "step 1's set: the smart lock alone, tyres as in the data";
      lockOnly.onclick = () => {
        saveGrip(gd.lock_only);
        this.build();
      };
      const step2 = document.createElement("button");
      step2.textContent = "step 2";
      step2.title = "step 2's set: smart lock (more front scrub at speed), RWD tyres × 0.75 / × 0.65, no downforce, no probe fix";
      step2.onclick = () => {
        saveGrip(gd.step2);
        this.build();
      };
      const forSlow = document.createElement("button");
      forSlow.textContent = "for slow weight";
      forSlow.title = "the balance to use with the suspension section's slow weight (clip 4): smart lock + rear downforce, tyres as in the data (new's softer tyres spin with slow weight)";
      forSlow.onclick = () => {
        saveGrip(gd.slow_weight);
        this.build();
      };
      const abRow = document.createElement("div");
      abRow.style.cssText = "display:flex;gap:6px;flex-wrap:wrap";
      abRow.append(ab, forSlow, step2, lockOnly);
      this.body.appendChild(abRow);
      const sel = document.createElement("select");
      sel.id = "drift-w-lock-mode";
      for (const m of ["plain", "smart"]) {
        const o = document.createElement("option");
        o.value = m;
        o.textContent = m === "smart" ? "smart (fronts at peak slip)" : "plain (rack curve)";
        sel.appendChild(o);
      }
      sel.value = String(gt.lock_mode ?? "plain");
      sel.onchange = () => {
        saveGrip({ ...(this.sim.gripBalance() ?? gd.before), lock_mode: sel.value });
        this.build();
      };
      row("keyboard lock at speed", "plain = the steering section's rack curve; smart = lock capped at the steady-corner angle + the front tyre's peak slip × (1 + past peak)", sel, document.createElement("span"));
      for (const [key, f] of Object.entries(GRIP_FIELDS)) {
        const input = document.createElement("input");
        input.type = "range";
        input.min = String(f.min);
        input.max = String(f.max);
        input.step = String(f.step);
        input.value = String(gt[key] ?? gd.before[key]);
        const val = document.createElement("span");
        const show = () => {
          val.textContent = `${Number((Number(input.value) * (f.scale ?? 1)).toFixed(2))}${f.unit ? ` ${f.unit}` : ""}`;
          val.style.color = Math.abs(Number(input.value) - Number(gd.new[key])) > 1e-6 ? "#ffd27a" : "";
        };
        show();
        input.oninput = () => {
          show();
          saveGrip({ ...(this.sim.gripBalance() ?? gd.before), [key]: Number(input.value) });
        };
        row(f.label, f.hint, input, val);
      }
      for (const [key, label, hint] of [
        ["probe_tilt_fix", "wheel probe tilt fix", "the ground probe no longer touches on its edge on smooth road when the body rolls: removes the tick-to-tick tyre load chatter in corners (kerbs unchanged); proposed as the default"],
        ["tyres_rwd_only", "tyre scales: RWD cars only", "the front / rear tyre scales apply to rear-drive cars only (front-drive cars keep their tyres); the lock and bars apply to all"],
        ["smart_angle_rate", "smart lock: plain wheel speed", "the wheels move at the plain rack's speed and stop at the smart lock (a 0.1 s tap turns them as far as before); off = full smart lock in the rack's 0.28 s"],
        ["smart_fade_both", "smart lock off in slides (both ways)", "the plain lock comes back with body slip for the into-turn side too (else only for countersteer)"],
        ["smart_track", "smart lock: track the travel", "experimental: cap the into-turn angle at the front axle's travel + peak slip (fronts held at peak however the car rotates; it tends to spin at speed)"],
      ] as const) {
        const input = document.createElement("input");
        input.type = "checkbox";
        input.checked = Boolean(gt[key] ?? gd.before[key]);
        input.onchange = () => saveGrip({ ...(this.sim.gripBalance() ?? gd.before), [key]: input.checked });
        row(label, hint, input, document.createElement("span"));
      }
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
    copy.onclick = () => void navigator.clipboard?.writeText(JSON.stringify({ steer: this.sim.plainSteer(), ride: this.sim.rideTune(), car: this.sim.carTune(), grip: this.sim.gripBalance() }, null, 1));
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
