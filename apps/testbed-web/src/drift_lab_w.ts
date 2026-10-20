// Drift Lab W (lead/drift-lab-w): deliberately PLAIN keyboard driving, to A/B against the
// reference (docs/reference/wreckfest-keyboard.md). Keys reach the sim raw (−1 / 0 / +1,
// throttle 0 / 1); the rack slews at a constant rate (wp_vehicle::plain: full lock in t_full,
// back to centre in t_return, lock shrinking with speed by the speed-sensitivity curve). No
// drift intent modes, no throttle shaping, TCS / ESC off, manual gears. Same course as Drift
// Lab C. The four reference-parity test cars (data/cars/parity/) are selectable here only.
import type * as THREE from "three";
import { buildCourse } from "./drift_lab_b";
import { setRawKeyboard } from "./input";
import { assistLevel, type AssistLevel, type sim as Sim } from "./wasm/sim";

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
  counter_margin_deg: { label: "countersteer limit", min: 0, max: 5, step: 0.05, unit: "°", hint: "step 10 snap-back: in a slide a held counter key points the wheels at most this far past the front axle's travel, so the lock unwinds as the slide closes (0 = off, the 'before'; default 0.25°)" },
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
// TCS / ABS levels (round12 Off / Half / Full). The old "1" / "0" TCS checkbox value still reads.
const TCS_STORE = "wp.driftLabW.tcs";
const ABS_STORE = "wp.driftLabW.abs";
// Handbrake pivot aid knobs (round12 row 109: `hb_pivot_accel` / `hb_pivot_cap` of the drift tuning).
const PIVOT_STORE = "wp.driftLabW.pivot";
const PIVOT_FIELDS: Record<string, { label: string; min: number; max: number; step: number; unit: string; hint: string }> = {
  hb_pivot_accel: { label: "handbrake pivot aid", min: 0, max: 15, step: 0.5, unit: "rad/s²", hint: "round12 row 109: yaw acceleration added while the handbrake AND a steer key are held (builds over 0.4 s, full ≤ 55 km/h, gone at 75 km/h); 0 = off, pure tyre physics" },
  hb_pivot_cap: { label: "pivot aid yaw cap", min: 0.5, max: 6, step: 0.25, unit: "rad/s", hint: "the aid fades out as the yaw rate toward the key reaches this (never adds to a fast spin)" },
};
// v2: step 10's default ride became medium weight (an older saved ride must not override it).
const RIDE_STORE = "wp.driftLabW.ride.v2";
const EVERYWHERE_STORE = "wp.driftLabW.everywhere";
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
  private handling = false;
  private readonly badge: HTMLDivElement;
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
    // Outside the lab: a small tag while the W profile drives the player's car.
    this.badge = document.createElement("div");
    this.badge.id = "drift-lab-w-badge";
    this.badge.textContent = "W handling";
    this.badge.title = `${DRIFT_LAB_W_TITLE} handling on your car ("use W handling everywhere" in the lab panel)`;
    this.badge.style.cssText =
      "position:absolute;left:12px;bottom:12px;z-index:30;font:11px system-ui,sans-serif;color:#cfe8ff;background:rgba(0,0,0,.55);border-left:3px solid #40a0e0;padding:2px 6px;border-radius:4px;pointer-events:none;display:none";
    parent.appendChild(this.badge);
  }

  /** "Use W handling everywhere" (lead): keep the W steering / tyre / suspension / pivot
   * profile on the player's car outside the lab (free drive, city chase, races). Saved per
   * viewer; default ON in the W build. */
  everywhere(): boolean {
    try {
      return localStorage.getItem(EVERYWHERE_STORE) !== "0";
    } catch {
      return true;
    }
  }

  /** Whether the W handling profile is on for this session (in the lab, or everywhere). */
  handlingOn(): boolean {
    return this.handling;
  }

  /** A session started. `on` = it is the lab session (panel, course, reference assists);
   * `otherLab` = another drift lab owns the drift tuning (no W handling there). With
   * "use W handling everywhere" on, every other session keeps the W profile on the player's
   * car only (AI, police and traffic stay stock: `wp_vehicle::lab_scope`). */
  setActive(on: boolean, otherLab = false): void {
    this.setHandling(on || (this.everywhere() && !otherLab));
    this.badge.style.display = this.handling && !on ? "block" : "none";
    if (on === this.active) {
      if (on) this.build();
      return;
    }
    this.active = on;
    this.root.style.display = on ? "block" : "none";
    if (on && !this.shown) this.world.add(this.course);
    if (!on && this.shown) this.world.remove(this.course);
    this.shown = on;
    if (on) this.prevTitle = document.title;
    document.title = on ? DRIFT_LAB_W_TITLE : this.prevTitle;
    if (on) this.build();
  }

  /** Turn the W profile on / off: the lab overrides (saved panel values), raw keys, and the
   * player's car tagged as the only car they apply to. */
  private setHandling(on: boolean): void {
    const was = this.handling;
    this.handling = on;
    if (!on) {
      // Only undo what W set (another lab may own the drift tuning now).
      if (!was) return;
      setRawKeyboard(false);
      this.sim.labScopeSetTaggedOnly(false);
      this.sim.driftTuningSet("");
      this.sim.plainSteerSet("");
      this.sim.rideTuneSet("");
      this.sim.carTuneSet("");
      return;
    }
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
      this.sim.rideTuneSet(ride || JSON.stringify(this.sim.rideTuneDefaults().medium_weight));
    } catch {
      this.sim.rideTuneSet(JSON.stringify(this.sim.rideTuneDefaults().medium_weight));
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
    this.applyPivot();
    setRawKeyboard(true);
    // Only the player's car: AI racers, police and traffic keep the stock handling.
    this.sim.labScopeSetTaggedOnly(true);
    try {
      this.sim.setLabTagged(this.player(), true);
    } catch (e) {
      // Replays / non-live sessions: the overrides then reach nobody (tagged-only, no tag).
      console.warn("W handling: cannot tag the player", e);
    }
  }

  /** TCS level (panel select), default Off (the owner's reference). The old checkbox's
   * "1" reads as Full. */
  private tcs(): AssistLevel {
    try {
      const v = localStorage.getItem(TCS_STORE);
      return v === "1" ? "Full" : assistLevel(v, "Off");
    } catch {
      return "Off";
    }
  }

  /** ABS level (panel select), default Full. */
  private abs(): AssistLevel {
    try {
      return assistLevel(localStorage.getItem(ABS_STORE), "Full");
    } catch {
      return "Full";
    }
  }

  /** Saved handbrake pivot knobs (only those two keys), or {}. */
  private pivot(): Record<string, number> {
    try {
      const v = JSON.parse(localStorage.getItem(PIVOT_STORE) ?? "{}") as Record<string, unknown>;
      const out: Record<string, number> = {};
      for (const k of Object.keys(PIVOT_FIELDS)) if (typeof v[k] === "number") out[k] = v[k] as number;
      return out;
    } catch {
      return {};
    }
  }

  /** Drift tuning = the defaults + the saved pivot knobs (plain steering bypasses the rest). */
  private applyPivot(p: Record<string, number> = this.pivot()): void {
    this.sim.driftTuningSet(JSON.stringify({ ...this.sim.driftTuningDefaults(), ...p }));
  }

  /** The lab's assists (call after any assist preset is applied): the owner's reference
   * settings — TCS Off / ABS Full (panel selects), ESC off, manual gears (automatic clutch), no pedal filters, no
   * countersteer help (it is bypassed by plain steering anyway). */
  applyAssists(): void {
    if (!this.active) return;
    const id = this.player();
    const a = this.sim.assists(id);
    this.sim.setAssists(id, {
      ...a,
      abs: this.abs(),
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
    // Step 10 A/B (lead): "before" = steps 1–9 (no countersteer limit, lab ride) vs "new" =
    // the default (countersteer limit 0.25°, medium weight). Tyres stay as set below.
    {
      const ab = document.createElement("div");
      ab.style.cssText = "display:flex;gap:6px;align-items:center;margin:2px 0 4px";
      const lbl = document.createElement("span");
      lbl.textContent = "handling A/B:";
      ab.appendChild(lbl);
      for (const [label, id, margin, ride] of [
        ["before", "drift-lab-w-before", 0, "lab"],
        ["new", "drift-lab-w-new", (defaults as Record<string, number>).counter_margin_deg, "medium_weight"],
      ] as const) {
        const b = document.createElement("button");
        b.id = id;
        b.textContent = label;
        b.title =
          label === "before"
            ? "steps 1–9: no countersteer limit (lifting off with the counter key held snaps the car back and can spin it), step-3 lab ride"
            : "step 10 default: countersteer limit 0.25° past the travel (progressive recovery) + medium weight (slower weight transfer, keeps the hold)";
        b.onclick = () => {
          this.save({ ...(this.sim.plainSteer() ?? defaults), counter_margin_deg: margin });
          const text = JSON.stringify(this.sim.rideTuneDefaults()[ride]);
          this.sim.rideTuneSet(text);
          try {
            localStorage.setItem(RIDE_STORE, text);
          } catch {
            /* ignore */
          }
          this.restart(this.car);
        };
        ab.appendChild(b);
      }
      this.body.appendChild(ab);
    }
    // "Use W handling everywhere" (lead): free drive, city chase and races keep this profile.
    {
      const input = document.createElement("input");
      input.type = "checkbox";
      input.id = "drift-lab-w-everywhere";
      input.checked = this.everywhere();
      input.onchange = () => {
        try {
          localStorage.setItem(EVERYWHERE_STORE, input.checked ? "1" : "0");
        } catch {
          /* ignore */
        }
      };
      row(
        "use W handling everywhere",
        "free drive, city chase and races use this steering / tyre / suspension / handbrake profile on your car (AI, police and traffic stay stock); takes effect on the next session",
        input,
        document.createElement("span"),
      );
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
    // TCS / ABS levels (round12 Off / Half / Full).
    for (const [name, id, store, get, hint] of [
      ["TCS", "drift-lab-w-tcs", TCS_STORE, () => this.tcs(), "traction control Off / Half / Full (round12 levels); Off = the raw throttle key, as the owner drives"],
      ["ABS", "drift-lab-w-abs", ABS_STORE, () => this.abs(), "ABS Off / Half / Full (round12 levels); default Full"],
    ] as const) {
      const sel = document.createElement("select");
      sel.id = id;
      for (const l of ["Off", "Half", "Full"]) sel.add(new Option(l, l));
      sel.value = get();
      sel.onchange = () => {
        try {
          localStorage.setItem(store, sel.value);
        } catch {
          /* ignore */
        }
        this.applyAssists();
      };
      row(name, hint, sel, document.createElement("span"));
    }
    // Handbrake row: round12's pivot aid (row 109), kept by the lead for W.
    for (const [key, f] of Object.entries(PIVOT_FIELDS)) {
      const defaults = this.sim.driftTuningDefaults() as Record<string, number>;
      const value = this.pivot()[key] ?? defaults[key];
      const input = document.createElement("input");
      input.type = "range";
      input.id = `drift-lab-w-${key}`;
      input.min = String(f.min);
      input.max = String(f.max);
      input.step = String(f.step);
      input.value = String(value);
      const val = document.createElement("span");
      val.textContent = `${Number(value).toFixed(2)} ${f.unit}`;
      input.oninput = () => {
        val.textContent = `${Number(input.value).toFixed(2)} ${f.unit}`;
        const p = { ...this.pivot(), [key]: Number(input.value) };
        try {
          localStorage.setItem(PIVOT_STORE, JSON.stringify(p));
        } catch {
          /* ignore */
        }
        this.applyPivot(p);
      };
      row(f.label, `${f.hint} (default ${defaults[key]})`, input, val);
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
          val.style.color = Math.abs(Number(input.value) - rd.medium_weight[key]) > 1e-6 ? "#ffd27a" : "";
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
      for (const [label, which] of [["medium weight (new)", "medium_weight"], ["lab default (before)", "lab"], ["slow weight (clip 4)", "slow_weight"], ["suspension data", "data"]] as const) {
        const b = document.createElement("button");
        b.textContent = label;
        b.title =
          which === "medium_weight"
            ? "step 10 default: slower weight transfer that keeps the held drift (springs 0.7, bars 0.15, dampers 1.6 / 2.1, ride height kept, travel 1.4, roll / pitch inertia 1.8 / 1.6). Restarts the car"
            : which === "lab"
            ? "step 3 default: calmer, slower weight transfer (springs 0.9, bars 0.25, dampers 1.4 / 1.8)"
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
