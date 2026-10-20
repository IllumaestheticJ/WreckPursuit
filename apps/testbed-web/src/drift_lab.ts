// Drift lab (owner rows 90 / 91 / 98): flat asphalt pad, day, no traffic or police, with a
// painted course (two 90° corners and a long sweeper) and a live tuning panel for the slide
// assists. The panel writes a runtime override (wp_wasm set_drift_tuning / set_countersteer);
// it never changes the data files. Its values are kept in localStorage "wp.driftTuning.v4".
import * as THREE from "three";
import { sim } from "./wasm/sim";
import { S, STATE_STRIDE } from "./wasm/layout";
import { store } from "./settings";

// v4: handbrake pivot aid sliders (row 109); v3 and older values are not reused.
const TUNING_KEY = "wp.driftTuning.v4";

/** One tuning slider: index into the set_drift_tuning array (or -1 = countersteer %). */
interface Knob {
  idx: number;
  label: string;
  min: number;
  max: number;
  step: number;
  /** Shown value from the stored value (e.g. rad → °). */
  show: (v: number) => string;
  hint: string;
}

const DEG = 180 / Math.PI;
const KNOBS: Knob[] = [
  { idx: -1, label: "countersteer %", min: 0, max: 100, step: 5, show: (v) => `${v.toFixed(0)}%`, hint: "slider strength and speed (AssistProfile::countersteer_help)" },
  { idx: 0, label: "onset slip", min: 0.005, max: 0.1, step: 0.005, show: (v) => `${(v * DEG).toFixed(1)}°`, hint: "rear-axle slip where released keys start aligning the wheels" },
  { idx: 1, label: "full-align slip", min: 0.02, max: 0.2, step: 0.005, show: (v) => `${(v * DEG).toFixed(1)}°`, hint: "rear-axle slip where the alignment is complete" },
  { idx: 2, label: "slew boost", min: 0, max: 10, step: 0.25, show: (v) => `×(1+${v.toFixed(2)}·help)`, hint: "aligning / countersteer rack = max(lock rate, rack speed) × (1 + boost × help)" },
  { idx: 3, label: "save-lock onset", min: 0.005, max: 0.15, step: 0.005, show: (v) => `${(v * DEG).toFixed(1)}°`, hint: "rear-axle slip where the countersteer lock starts opening" },
  { idx: 4, label: "save-lock full", min: 0.02, max: 0.3, step: 0.005, show: (v) => `${(v * DEG).toFixed(1)}°`, hint: "rear-axle slip where it is fully open" },
  { idx: 5, label: "hold gain", min: 0, max: 3, step: 0.05, show: (v) => v.toFixed(2), hint: "hold mode: countersteer per rad of slide change (resists the angle growing / shrinking)" },
  { idx: 6, label: "throttle smoothing", min: 0, max: 1, step: 0.05, show: (v) => `${(v * 100).toFixed(0)}%`, hint: "keyboard throttle smoothing with TCS off (row 98): slower pedal rise in low gears in powerful cars + a small traction trim; default 0 = raw throttle (owner row 104), a lab-only option" },
  { idx: 8, label: "hold mode (row 91)", min: 0, max: 1, step: 1, show: (v) => (v > 0.5 ? "on" : "off"), hint: "key into the turn + throttle holds the drift angle; off by default (owner rows 106 / 111)" },
  { idx: 9, label: "hb pivot", min: 0, max: 15, step: 0.5, show: (v) => `${v.toFixed(1)} rad/s²`, hint: "handbrake pivot aid (row 109, owner-approved yaw torque): yaw acceleration with handbrake + key held, building up over 0.4 s, full ≤ 55 km/h, gone by 75 km/h; 0 = off" },
  { idx: 10, label: "hb pivot cap", min: 0.5, max: 6, step: 0.1, show: (v) => `${v.toFixed(1)} rad/s`, hint: "yaw rate toward the key at which the pivot aid has faded out" },
  { idx: 7, label: "envelope @100%", min: 10, max: 90, step: 1, show: (v) => `${v.toFixed(0)}°`, hint: "slide angle the assist helps within at 100 % (× 0.6 + 0.4 × help); past it the hold lets go" },
];

const MODES = ["off", "straighten", "catch", "hold", "exceeded"];

/** Painted course in the car's spawn frame (x right, y forward, m): centre line points. */
function coursePoints(): [number, number][] {
  const pts: [number, number][] = [];
  const line = (x0: number, y0: number, x1: number, y1: number) => {
    const n = Math.ceil(Math.hypot(x1 - x0, y1 - y0) / 2);
    for (let i = 0; i <= n; i++) pts.push([x0 + ((x1 - x0) * i) / n, y0 + ((y1 - y0) * i) / n]);
  };
  const arc = (cx: number, cy: number, r: number, a0: number, a1: number) => {
    const n = Math.ceil((Math.abs(a1 - a0) * r) / 2);
    for (let i = 0; i <= n; i++) {
      const a = a0 + ((a1 - a0) * i) / n;
      pts.push([cx + r * Math.cos(a), cy + r * Math.sin(a)]);
    }
  };
  // 120 m run-up, a 90° right-hander (r 22 m), 70 m, a 90° left-hander (r 22 m), 60 m, then a
  // long left sweeper (r 70 m, 180°) back the other way.
  line(0, 0, 0, 120);
  arc(22, 120, 22, Math.PI, Math.PI / 2);
  line(22, 142, 92, 142);
  arc(92, 164, 22, -Math.PI / 2, 0);
  line(114, 164, 114, 224);
  arc(44, 224, 70, 0, Math.PI);
  return pts;
}

/** The drift lab: course markers in the world, tuning panel and readout in the page. */
export class DriftLab {
  private readonly group = new THREE.Group();
  private active = false;
  private player = 0;
  private readonly panel: HTMLDivElement;
  private readonly readout: HTMLDivElement;
  private readonly inputs: HTMLInputElement[] = [];

  constructor(world: THREE.Group) {
    world.add(this.group);
    this.group.visible = false;
    this.panel = document.createElement("div");
    this.panel.id = "drift-panel";
    this.panel.style.cssText =
      "position:fixed;right:8px;top:8px;z-index:20;background:rgba(20,22,26,.92);color:#ddd;font:12px monospace;padding:8px 10px;border-radius:6px;display:none;min-width:300px";
    const title = document.createElement("div");
    title.innerHTML = `<b>drift lab tuning</b> <span style="color:#888">(F8 / tune: hide · runtime override, not saved to data)</span>`;
    this.panel.append(title);
    this.readout = document.createElement("div");
    this.readout.style.cssText = "margin:6px 0;font-size:13px";
    this.panel.append(this.readout);
    for (const k of KNOBS) {
      const row = document.createElement("label");
      row.title = k.hint;
      row.style.cssText = "display:flex;align-items:center;gap:6px;margin:2px 0";
      const name = document.createElement("span");
      name.textContent = k.label;
      name.style.cssText = "width:120px";
      const input = document.createElement("input");
      input.type = "range";
      input.min = String(k.min);
      input.max = String(k.max);
      input.step = String(k.step);
      input.style.width = "110px";
      const val = document.createElement("span");
      val.style.cssText = "width:90px;color:#9cf";
      input.oninput = () => {
        val.textContent = k.show(Number(input.value));
        this.apply();
      };
      row.append(name, input, val);
      this.panel.append(row);
      this.inputs.push(input);
    }
    const reset = document.createElement("button");
    reset.textContent = "defaults";
    reset.onclick = () => {
      store.set(TUNING_KEY, "");
      this.loadFromSim(true);
    };
    this.panel.append(reset);
    document.body.append(this.panel);
    addEventListener("keydown", (e) => {
      if (e.code === "F8" && this.active) {
        e.preventDefault();
        this.togglePanel();
      }
    });
  }

  /** Whether the drift lab session is running. */
  get running(): boolean {
    return this.active;
  }

  /** Show / hide the tuning panel. */
  togglePanel(): void {
    this.panel.style.display = this.panel.style.display === "none" ? "block" : "none";
  }

  /** Start: lay the course in front of the player's spawn pose and open the panel. */
  start(player: number): void {
    this.active = true;
    this.player = player;
    this.buildCourse();
    this.group.visible = true;
    this.panel.style.display = "block";
    this.loadFromSim(false);
  }

  /** Leave the drift lab (another session started). */
  stop(): void {
    this.active = false;
    this.group.visible = false;
    this.panel.style.display = "none";
  }

  /** Per-frame readout. */
  update(): void {
    if (!this.active || this.panel.style.display === "none") return;
    let d: number[];
    try {
      d = sim.driftState(this.player);
    } catch {
      return;
    }
    const [slip, mode, env, thr] = d;
    const m = MODES[Math.round(mode)] ?? "?";
    const colour =
      m === "catch" ? "#ff9f43" : m === "straighten" ? "#2ecc71" : m === "hold" ? "#4aa3ff" : m === "exceeded" ? "#ff4136" : "#aaa";
    this.readout.innerHTML =
      `slide <b>${Math.abs(slip).toFixed(1)}°</b> · mode <b style="color:${colour}">${m}</b> · envelope ${env.toFixed(0)}° · thr smoothing ${(thr * 100).toFixed(0)}%`;
  }

  /** Fill the sliders from the stored override (or the sim's defaults) and apply. */
  private loadFromSim(defaults: boolean): void {
    const base = sim.driftTuning(this.player);
    const cst = sim.countersteer(this.player);

    let stored: number[] | null = null;
    if (!defaults) {
      try {
        const raw = store.get(TUNING_KEY);
        if (raw) stored = JSON.parse(raw);
      } catch {
        stored = null;
      }
    }
    KNOBS.forEach((k, i) => {
      const v = stored && stored.length === KNOBS.length ? stored[i] : k.idx < 0 ? cst : base[k.idx];
      this.inputs[i].value = String(v);
      (this.inputs[i].nextElementSibling as HTMLElement).textContent = k.show(Number(this.inputs[i].value));
    });
    this.apply();
  }

  /** Send the sliders to the sim and remember them. */
  private apply(): void {
    const values = KNOBS.map((_, i) => Number(this.inputs[i].value));
    const tuning = new Float32Array(11);
    KNOBS.forEach((k, i) => {
      if (k.idx >= 0) tuning[k.idx] = values[i];
    });
    try {
      sim.setDriftTuning(this.player, tuning);
      const pct = values[KNOBS.findIndex((k) => k.idx < 0)];
      sim.setCountersteer(this.player, pct);
      // Keep the assist row's countersteer slider in step.
      const row = document.getElementById("cstr") as HTMLInputElement | null;
      if (row) row.value = String(pct);
      const label = document.getElementById("cstr-val");
      if (label) label.textContent = `${pct}%`;
    } catch {
      return;
    }
    store.set(TUNING_KEY, JSON.stringify(values));
  }

  /** Cones along both edges (12 m wide) of the course, placed in the spawn frame. */
  private buildCourse(): void {
    this.group.clear();
    const st = sim.states();
    let x0 = 0,
      y0 = 0,
      yaw = 0;
    for (let i = 0; i + STATE_STRIDE <= st.length; i += STATE_STRIDE) {
      if (st[i + S.entity] !== this.player) continue;
      x0 = st[i + S.pos];
      y0 = st[i + S.pos + 1];
      const [qx, qy, qz, qw] = [st[i + S.rot], st[i + S.rot + 1], st[i + S.rot + 2], st[i + S.rot + 3]];
      // Heading of body +Y in the world, as a yaw about +Z (0 = facing +Y).
      const fx = 2 * (qx * qy - qw * qz);
      const fy = 1 - 2 * (qx * qx + qz * qz);
      yaw = Math.atan2(-fx, fy);
    }
    const pts = coursePoints();
    const cone = new THREE.ConeGeometry(0.18, 0.5, 8);
    cone.rotateX(Math.PI / 2);
    cone.translate(0, 0, 0.25);
    const n = pts.length * 2;
    const mesh = new THREE.InstancedMesh(cone, new THREE.MeshLambertMaterial({ color: 0xff7a1a }), n);
    const c = Math.cos(yaw),
      s = Math.sin(yaw);
    const m = new THREE.Matrix4();
    let k = 0;
    for (let i = 0; i < pts.length; i++) {
      const [px, py] = pts[i];
      const [qx, qy] = pts[Math.min(i + 1, pts.length - 1)];
      const [rx, ry] = i + 1 < pts.length ? [qx - px, qy - py] : [px - pts[i - 1][0], py - pts[i - 1][1]];
      const len = Math.hypot(rx, ry) || 1;
      // Left normal of the direction (x right, y forward frame).
      const [nx, ny] = [-ry / len, rx / len];
      for (const side of [-6, 6]) {
        const lx = px + nx * side,
          ly = py + ny * side;
        // Local (x right, y forward) → world, yaw counter-clockwise from +Y.
        const wx = x0 + lx * c - ly * s;
        const wy = y0 + lx * s + ly * c;
        m.makeTranslation(wx, wy, 0);
        mesh.setMatrixAt(k++, m);
      }
    }
    mesh.instanceMatrix.needsUpdate = true;
    this.group.add(mesh);
  }
}
