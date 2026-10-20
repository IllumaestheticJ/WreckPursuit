// Drift Lab C (lead/drift-lab-c, owner rows 104 / 106; built on Drift Lab B, rows 90 / 91): an
// open asphalt pad with three tight 90° corners and one long sweeper (no traffic, no police,
// daytime), plus the "tune" panel for the drift-assist constants
// (wp_vehicle::drift::DriftTuning, a runtime override via wp_wasm drift_tuning_set) and a live
// readout of the assist mode, held vs current slide angle, envelope and key vs applied throttle.
// Keyboard: countersteer key = hold, keys released = straighten, key into the turn = more angle.
// The course is paint and cones only (no physics): the car drives on the flat world's ground.
import * as THREE from "three";
import type { sim as Sim } from "./wasm/sim";

/** Scenario name of the lab (sim.scenario().name). */
export const DRIFT_LAB = "drift_lab_c";
/** Lab title / version badge. */
export const DRIFT_LAB_TITLE = "Drift Lab C";

/** Free-drive (flat world) scenario JSON turned into the lab: no traffic / parked cars /
 * police, noon with a slow clock. */
export function driftLabScenario(freeDriveJson: string): string {
  const sc = JSON.parse(freeDriveJson);
  return JSON.stringify({
    ...sc,
    name: DRIFT_LAB,
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

type Seg = { kind: "straight"; len: number } | { kind: "arc"; r: number; deg: number };

/** Clockwise loop from the spawn (0, 0) heading +Y; closes on itself (see drift-lab-b.md). */
const COURSE: Seg[] = [
  { kind: "straight", len: 160 },
  { kind: "arc", r: 22, deg: 90 },
  { kind: "straight", len: 130 },
  { kind: "arc", r: 22, deg: 90 },
  { kind: "straight", len: 160 },
  { kind: "arc", r: 70, deg: 90 }, // the long sweeper
  { kind: "straight", len: 82 },
  { kind: "arc", r: 22, deg: 90 },
  { kind: "straight", len: 48 },
];
const TRACK_W = 14;

/** Centre-line samples [x, y, heading] (heading: direction of travel, rad from +X). */
function centreLine(step = 2): [number, number, number][] {
  const pts: [number, number, number][] = [];
  let x = 0,
    y = 0,
    h = Math.PI / 2;
  pts.push([x, y, h]);
  for (const s of COURSE) {
    if (s.kind === "straight") {
      const n = Math.max(1, Math.round(s.len / step));
      for (let i = 0; i < n; i++) {
        x += (Math.cos(h) * s.len) / n;
        y += (Math.sin(h) * s.len) / n;
        pts.push([x, y, h]);
      }
    } else {
      // Right-hand (clockwise) arc.
      const a = (s.deg * Math.PI) / 180;
      const n = Math.max(4, Math.round((s.r * a) / step));
      const cx = x + Math.cos(h - Math.PI / 2) * s.r;
      const cy = y + Math.sin(h - Math.PI / 2) * s.r;
      const a0 = Math.atan2(y - cy, x - cx);
      for (let i = 1; i <= n; i++) {
        const t = a0 - (a * i) / n;
        x = cx + Math.cos(t) * s.r;
        y = cy + Math.sin(t) * s.r;
        pts.push([x, y, h - (a * i) / n]);
      }
      h -= a;
    }
  }
  return pts;
}

/** A flat strip along `pts` offset `off` m to the left of travel, `w` m wide. */
function strip(pts: [number, number, number][], off: number, w: number, z: number): THREE.BufferGeometry {
  const pos: number[] = [];
  for (const [x, y, h] of pts) {
    const lx = -Math.sin(h),
      ly = Math.cos(h);
    pos.push(x + lx * (off - w / 2), y + ly * (off - w / 2), z, x + lx * (off + w / 2), y + ly * (off + w / 2), z);
  }
  const idx: number[] = [];
  for (let i = 0; i + 1 < pts.length; i++) {
    const a = 2 * i;
    // Counter-clockwise seen from above (+Z), so the strip faces up.
    idx.push(a, a + 2, a + 1, a + 1, a + 2, a + 3);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  g.setIndex(idx);
  g.computeVertexNormals();
  return g;
}

/** The lab's paint and cones, in sim coordinates (+Z up), for `scene.world`. */
export function buildCourse(): THREE.Group {
  const g = new THREE.Group();
  g.name = "drift-lab-b";
  const pad = new THREE.Mesh(new THREE.PlaneGeometry(420, 420), new THREE.MeshStandardMaterial({ color: 0x3a3c40, roughness: 0.95 }));
  pad.position.set(60, 70, 0.012);
  g.add(pad);
  const pts = centreLine();
  const road = new THREE.Mesh(strip(pts, 0, TRACK_W, 0.016), new THREE.MeshStandardMaterial({ color: 0x2a2b2e, roughness: 0.9 }));
  g.add(road);
  const paint = new THREE.MeshStandardMaterial({ color: 0xf2f2f2, roughness: 0.6 });
  g.add(new THREE.Mesh(strip(pts, TRACK_W / 2, 0.35, 0.02), paint));
  g.add(new THREE.Mesh(strip(pts, -TRACK_W / 2, 0.35, 0.02), paint));
  const kerb = new THREE.MeshStandardMaterial({ color: 0xd03030, roughness: 0.6 });
  // Cones on the inside of every corner (apex markers), every few samples.
  const cone = new THREE.ConeGeometry(0.25, 0.7, 10);
  cone.rotateX(Math.PI / 2);
  cone.translate(0, 0, 0.35);
  for (let i = 1; i < pts.length - 1; i += 3) {
    const turning = Math.abs(pts[i + 1][2] - pts[i - 1][2]) > 1e-4;
    if (!turning) continue;
    const [x, y, h] = pts[i];
    // Clockwise loop: the inside is to the right of travel.
    const m = new THREE.Mesh(cone, kerb);
    m.position.set(x + Math.sin(h) * (TRACK_W / 2 + 0.6), y - Math.cos(h) * (TRACK_W / 2 + 0.6), 0);
    g.add(m);
  }
  // Start line.
  const start = new THREE.Mesh(new THREE.PlaneGeometry(TRACK_W, 1.2), paint);
  start.position.set(0, 6, 0.022);
  g.add(start);
  return g;
}

/** Slider spec per tuning field (unknown fields get a generic slider). */
const FIELDS: Record<string, { label: string; min: number; max: number; step: number; hint: string }> = {
  onset_slip_rad: { label: "onset (rad)", min: 0.005, max: 0.1, step: 0.005, hint: "rear-axle slip where the slide onset starts" },
  onset_full_rad: { label: "onset full (rad)", min: 0.02, max: 0.2, step: 0.005, hint: "rear-axle slip where the onset is full" },
  onset_rate_rps: { label: "onset rate (rad/s)", min: 0.05, max: 1.5, step: 0.05, hint: "rear slip growth that starts the onset on its own" },
  catch_lock_s: { label: "slew: full lock (s @60%)", min: 0.04, max: 0.4, step: 0.01, hint: "time for full lock toward the countersteer side at 60 %" },
  hold_gain: { label: "hold gain", min: 0, max: 2, step: 0.05, hint: "Hold: front slip given up per 5.7° past the held angle (× peak slip)" },
  hold_damp: { label: "hold damping (s)", min: 0, max: 0.6, step: 0.01, hint: "Hold: front slip given up per rad/s the slide angle grows" },
  hold_base: { label: "hold into-turn", min: 0, max: 2, step: 0.05, hint: "Hold: front slip angle into the turn at the held angle (× peak slip)" },
  hold_entry_deg: { label: "angle-up entry (°)", min: 0, max: 40, step: 1, hint: "key into the turn from grip: the held angle starts at least here" },
  hold_decay_dps: { label: "hold decay (°/s)", min: 0, max: 15, step: 0.5, hint: "countersteer key held: the held angle shrinks this fast (taps nudge it)" },
  angle_up_dps: { label: "angle-up rate (°/s)", min: 0, max: 40, step: 1, hint: "key into the turn: the held angle grows this fast" },
  angle_up_steer: { label: "angle-up steer", min: 0, max: 2, step: 0.05, hint: "key into the turn: extra front slip into the turn (× peak slip) — a tighter line" },
  angle_up_damp: { label: "angle-up damping (s)", min: 0, max: 1.5, step: 0.05, hint: "key into the turn: front slip given up per rad/s the slide grows (no overshoot)" },
  lift_shrink_dps: { label: "lift shrink (°/s)", min: 0, max: 30, step: 0.5, hint: "off the throttle the held angle shrinks this fast" },
  lift_authority: { label: "lift authority", min: 0, max: 1, step: 0.05, hint: "hold strength with the throttle released (lower = a lift straightens more)" },
  throttle_shape: { label: "throttle shaping (opt-in)", min: 0, max: 1, step: 0.05, hint: "opt-in, default 0 = exactly the throttle you press; >0 adds a rise limit in slides, a trim near the envelope and lift softening" },
  throttle_rise_s: { label: "throttle rise (s)", min: 0, max: 1, step: 0.05, hint: "in a slide the throttle rises 0 → 100 % no faster than this (× shaping); never a fixed partial throttle" },
  trim_margin_deg: { label: "trim zone (°)", min: 1, max: 20, step: 1, hint: "the throttle is trimmed only within this of the envelope" },
  trim_depth: { label: "trim depth", min: 0, max: 1, step: 0.05, hint: "deepest trim at the envelope edge (× shaping)" },
  envelope_deg: { label: "envelope (° @60%)", min: 10, max: 70, step: 1, hint: "slide angle the assist helps up to at 60 % (±25° per 100 % of slider)" },
  straighten_gain: { label: "straighten extra", min: 0, max: 3, step: 0.05, hint: "released keys: extra countersteer past the travel while a slide grows (× peak slip)" },
  lift_soften: { label: "lift soften", min: 0, max: 1, step: 0.05, hint: "only with throttle shaping on: how much an abrupt lift is softened (0 = raw, 1 = a full lift takes 0.5 s)" },
};

const MODES = ["off", "grip", "straighten", "hold", "angle-up", "exceeded"];
// New key for C: a remembered B override would bring back B's throttle shaping (now opt-in).
const STORE = "wp.driftLabC.tuning";

/** The "tune" button + panel and the readout. Call `frame(entity)` every rendered frame. */
export class DriftLabPanel {
  readonly root: HTMLDivElement;
  private readonly body: HTMLDivElement;
  private readonly readout: HTMLDivElement;
  private open = false;
  private active = false;
  private readonly course: THREE.Group;
  private shown = false;

  constructor(
    parent: HTMLElement,
    private readonly sim: typeof Sim,
    private readonly world: THREE.Object3D,
    private readonly player: () => number,
  ) {
    this.course = buildCourse();
    this.root = document.createElement("div");
    this.root.id = "drift-lab-b";
    this.root.style.cssText =
      "position:absolute;left:272px;top:196px;z-index:30;font:12px/1.35 system-ui,sans-serif;color:#eee;pointer-events:auto;max-width:min(320px,calc(100vw - 24px));display:none";
    const btn = document.createElement("button");
    btn.textContent = "tune";
    btn.id = "drift-tune";
    btn.title = `${DRIFT_LAB_TITLE}: tune the drift assist (runtime override, not the data files)`;
    btn.onclick = () => this.toggle();
    this.readout = document.createElement("div");
    this.readout.style.cssText = "background:rgba(0,0,0,.6);padding:6px 8px;border-radius:6px;margin-top:4px;font-variant-numeric:tabular-nums";
    this.body = document.createElement("div");
    this.body.style.cssText = "background:rgba(0,0,0,.72);padding:8px;border-radius:6px;margin-top:4px;display:none;max-height:60vh;overflow:auto";
    this.root.append(btn, this.readout, this.body);
    parent.appendChild(this.root);
    // A remembered override (this viewer only) comes back on load.
    try {
      const saved = localStorage.getItem(STORE);
      if (saved) this.sim.driftTuningSet(saved);
    } catch {
      /* storage blocked: defaults */
    }
  }

  /** Whether the lab session is running (shows the course, the readout and the button). */
  setActive(on: boolean): void {
    this.active = on;
    this.root.style.display = on ? "block" : "none";
    if (on && !this.shown) this.world.add(this.course);
    if (!on && this.shown) this.world.remove(this.course);
    this.shown = on;
    if (on) this.build();
  }

  private toggle(): void {
    this.open = !this.open;
    this.body.style.display = this.open ? "block" : "none";
    if (this.open) this.build();
  }

  private save(t: Record<string, unknown>): void {
    const text = JSON.stringify(t);
    this.sim.driftTuningSet(text);
    try {
      localStorage.setItem(STORE, text);
    } catch {
      /* ignore */
    }
  }

  private build(): void {
    const t = this.sim.driftTuning();
    const defaults = this.sim.driftTuningDefaults();
    this.body.replaceChildren();
    const row = (label: string, hint: string, input: HTMLInputElement, val: HTMLSpanElement) => {
      const l = document.createElement("label");
      l.title = hint;
      l.style.cssText = "display:grid;grid-template-columns:1fr 90px 44px;gap:6px;align-items:center;margin:2px 0";
      const s = document.createElement("span");
      s.textContent = label;
      input.style.width = "90px";
      l.append(s, input, val);
      this.body.appendChild(l);
    };
    // Countersteer % (the AssistProfile slider), same as the developer panel's.
    {
      const input = document.createElement("input");
      input.type = "range";
      input.min = "0";
      input.max = "100";
      input.step = "5";
      const pct = Math.round(this.sim.countersteer(this.player()));
      input.value = String(pct);
      const val = document.createElement("span");
      val.textContent = `${pct}%`;
      input.oninput = () => {
        this.sim.setCountersteer(this.player(), Number(input.value));
        val.textContent = `${input.value}%`;
      };
      row("countersteer %", "AssistProfile countersteer_help (the assist's overall strength)", input, val);
    }
    for (const [key, value] of Object.entries(t)) {
      const f = FIELDS[key];
      if (typeof value === "boolean") {
        const input = document.createElement("input");
        input.type = "checkbox";
        input.checked = value;
        const val = document.createElement("span");
        input.onchange = () => this.save({ ...this.sim.driftTuning(), [key]: input.checked });
        row(key === "intent" ? "intent modes" : key.replace(/_/g, " "), key === "intent" ? "off = Drift Lab B step 1 only (fast countersteer, earlier onset)" : "on / off", input, val);
        continue;
      }
      if (typeof value !== "number") continue;
      const input = document.createElement("input");
      input.type = "range";
      input.min = String(f?.min ?? 0);
      input.max = String(f?.max ?? Math.max(1, value * 3));
      input.step = String(f?.step ?? 0.01);
      input.value = String(value);
      const val = document.createElement("span");
      const show = () => (val.textContent = Number(input.value).toFixed(3).replace(/\.?0+$/, ""));
      show();
      const d = defaults[key];
      if (typeof d === "number" && Math.abs(d - value) > 1e-6) val.style.color = "#ffd27a";
      input.oninput = () => {
        show();
        val.style.color = typeof d === "number" && Math.abs(d - Number(input.value)) > 1e-6 ? "#ffd27a" : "";
        this.save({ ...this.sim.driftTuning(), [key]: Number(input.value) });
      };
      row(f?.label ?? key, f?.hint ?? key, input, val);
    }
    const actions = document.createElement("div");
    actions.style.cssText = "display:flex;gap:6px;margin-top:6px";
    const reset = document.createElement("button");
    reset.textContent = "defaults";
    reset.onclick = () => {
      this.sim.driftTuningSet("");
      try {
        localStorage.removeItem(STORE);
      } catch {
        /* ignore */
      }
      this.build();
    };
    const copy = document.createElement("button");
    copy.textContent = "copy JSON";
    copy.title = "copy the current constants (paste them into the owner's notes / drift-lab-b.md)";
    copy.onclick = () => void navigator.clipboard?.writeText(JSON.stringify(this.sim.driftTuning(), null, 1));
    actions.append(reset, copy);
    this.body.appendChild(actions);
  }

  /** Update the readout (cheap: one small typed array). */
  frame(): void {
    if (!this.active) return;
    let r: Float32Array;
    try {
      r = this.sim.driftReadout(this.player());
    } catch {
      return;
    }
    const mode = MODES[r[0] | 0] ?? "?";
    const env = r[3] > 0 ? `${r[3].toFixed(0)}°` : "–";
    const sliding = mode === "hold" || mode === "angle-up" || mode === "straighten" || mode === "exceeded";
    const held = mode === "hold" || mode === "angle-up" || mode === "exceeded" ? `${(r[8] ?? 0).toFixed(1)}°` : "–";
    const pct = (x: number) => `${Math.round((x ?? 0) * 100)}%`;
    this.readout.textContent =
      `${DRIFT_LAB_TITLE} · ${sliding ? mode.toUpperCase() : mode}\n` +
      `slide ${r[1].toFixed(1)}° · held ${held} · envelope ${env}\n` +
      `throttle key ${pct(r[6])} → engine ${pct(r[7])}` +
      (r[5] < 0.999 ? ` (trim ×${r[5].toFixed(2)})` : "") +
      `\nauthority ${pct(r[4])} · onset ${r[2].toFixed(2)}`;
    this.readout.style.whiteSpace = "pre";
    this.readout.style.borderLeft = `4px solid ${mode === "exceeded" ? "#e04040" : mode === "hold" ? "#40c070" : mode === "angle-up" ? "#e0a030" : mode === "straighten" ? "#4090e0" : "#666"}`;
  }
}
