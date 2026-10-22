// Police lights and non-sim police units (wave-2 style-police; spec research_police.md §2).
//
// - Flash patterns as data (`PATTERNS`), shared by sim police cars (vehicle_view.ts) and the
//   non-sim units drawn here. Lens meshes may strobe fast; the world-lighting point lights
//   follow a slower envelope (≤ 3 Hz with ~40 ms ramps, WCAG 2.3.1). `?flash=slow` or the
//   "reduced flashing" setting (F6, settings.ts) swaps every pattern for `P-SLOW`.
// - Wig-wag headlamps (`wigwag`, P-HEADLAMP-WW): left / right head lamps alternate at 2 Hz
//   (1 Hz with reduced flashing) while the siren is on.
// - `LightRig` drives one car's lens materials (left / right sides independently, clear
//   takedowns, amber advisors) and its two point lights (red left, blue right).
// - `PoliceUnitsView` draws police_units() (units that are not sim vehicles) with the real
//   police meshes: static merged geometry per model + livery texture + per-unit decals and
//   lens materials, chosen by the unit's model index (sim.policeModels()); fallback
//   `sentinel_cruiser`. A small fixed pool of point lights goes to the nearest pursuing units.
import * as THREE from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import { POLICE_UNIT_STRIDE, PU } from "../wasm/layout";
import { sim } from "../wasm/sim";
import type { CarMeshJson, LightbarSide, MaterialJson } from "./panels";
import { loadCarMesh } from "./panels";
import { applyUnitDecals, countySkin, hash32, liveryBaseColour, liveryTexture } from "./livery";
import { layers } from "../settings";

// ------------------------------------------------------------------------------------------
// Flash patterns
// ------------------------------------------------------------------------------------------

/** Channels: L / R = left / right lens halves (any colour), W = clear takedowns, A = amber. */
export type Channel = "L" | "R" | "W" | "A";
/** On-windows (seconds inside the period) per channel. */
export interface FlashPattern {
  period: number;
  /** Lens on-windows (fast strobing allowed). */
  ch: Partial<Record<Channel, [number, number][]>>;
  /** Scene-light envelope windows for L / R (≤ 3 Hz); defaults to `ch`. */
  env?: Partial<Record<"L" | "R", [number, number][]>>;
  /** Rotating beacons: lens / light brightness from a rotating lobe, L and R 180° apart. */
  rotate?: boolean;
}

/** research_police.md §2. */
export const PATTERNS: Record<string, FlashPattern> = {
  "P-GUMBALL": { period: 0.75, rotate: true, ch: {} },
  "P-ROTATOR": { period: 0.6, rotate: true, ch: { W: [[0, 0.15]] } },
  "P-STROBE": {
    period: 0.8,
    ch: { L: [[0, 0.04], [0.1, 0.14]], R: [[0.4, 0.44], [0.5, 0.54]], A: [[0.4, 0.44], [0.5, 0.54]] },
    env: { L: [[0, 0.18]], R: [[0.4, 0.58]] },
  },
  "P-LED-QUAD": {
    period: 0.5,
    ch: {
      L: [[0, 0.04], [0.06, 0.1], [0.12, 0.16], [0.18, 0.22]],
      R: [[0.25, 0.29], [0.31, 0.35], [0.37, 0.41], [0.43, 0.47]],
      W: [[0.22, 0.25], [0.47, 0.5]],
      A: [[0.25, 0.29], [0.31, 0.35], [0.37, 0.41], [0.43, 0.47]],
    },
    env: { L: [[0, 0.22]], R: [[0.25, 0.47]] },
  },
  "P-LED-WIGWAG": { period: 0.7, ch: { L: [[0, 0.3]], R: [[0.35, 0.65]], A: [[0, 0.3]] } },
  "P-LED-SIM": {
    period: 1.0,
    ch: { L: [[0, 0.08], [0.15, 0.23]], R: [[0, 0.08], [0.15, 0.23]], W: [[0.5, 0.58], [0.65, 0.73]], A: [[0, 0.5]] },
    env: { L: [[0, 0.23]], R: [[0, 0.23]] },
  },
  "P-SLOW": { period: 1.0, ch: { L: [[0, 0.45]], R: [[0.5, 0.95]], A: [[0, 0.45]] } },
};

/** Moving pattern per model (era): rotators for the 80s car, LED quad flash otherwise. */
export function patternFor(carId: string, parked = false): string {
  if (reducedFlashing()) return "P-SLOW";
  if (carId === "sentinel_warden") return "P-ROTATOR";
  return parked ? "P-LED-SIM" : "P-LED-QUAD";
}

let reducedUrl: boolean | null = null;
/** Reduced flashing (P-SLOW everywhere): `?flash=slow` in the URL or the settings toggle (F6). */
export function reducedFlashing(): boolean {
  if (reducedUrl === null) {
    try {
      reducedUrl = new URLSearchParams(globalThis.location?.search ?? "").get("flash") === "slow";
    } catch {
      reducedUrl = false;
    }
  }
  return reducedUrl || layers.on("reducedFlashing");
}

/**
 * Wig-wag head-lamp levels (P-HEADLAMP-WW) at time `t` (s) with phase offset `phase` (0..1):
 * left / right alternate, 2 Hz per lamp (1 Hz with reduced flashing), ~40 ms ramps.
 */
export function wigwag(t: number, phase: number): { left: number; right: number } {
  const period = reducedFlashing() ? 1.0 : 0.5;
  const ph = (((t + phase * period) % period) + period) % period;
  const half = period / 2;
  const wins = (a: number): [number, number][] => [[a + 0.02 * period, a + half - 0.02 * period]];
  return { left: windowLevel(wins(0), ph, true), right: windowLevel(wins(half), ph, true) };
}

/** Stable phase offset (s) for a unit / entity id, de-syncs a convoy. */
export function phaseFor(id: number): number {
  return (hash32(id) % 1000) / 1000;
}

const RAMP_S = 0.04;

/** 0..1: is `t` (within the period) inside a window; `soft` adds the ~40 ms ramps. */
function windowLevel(wins: [number, number][] | undefined, t: number, soft: boolean): number {
  if (!wins) return 0;
  let v = 0;
  for (const [a, b] of wins) {
    if (!soft) {
      if (t >= a && t < b) return 1;
      continue;
    }
    const e = Math.min(t - a, b - t) / RAMP_S + 0.5;
    v = Math.max(v, Math.min(1, Math.max(0, e)));
  }
  return v;
}

/** Channel levels at time `t` (s): lens (fast) and light (slow envelope). */
export function evalPattern(p: FlashPattern, t: number): { lens: Record<Channel, number>; light: { L: number; R: number } } {
  const ph = ((t % p.period) + p.period) % p.period;
  if (p.rotate) {
    const th = (2 * Math.PI * ph) / p.period;
    const lobe = (x: number) => Math.pow(Math.max(0, Math.cos(x)), 4);
    // Mirrors in the dome: two lobes per revolution, as seen from one direction.
    const l = Math.max(lobe(th), 0.6 * lobe(th + Math.PI));
    const r = Math.max(lobe(th + Math.PI), 0.6 * lobe(th));
    return {
      lens: { L: 0.3 + 0.7 * l, R: 0.3 + 0.7 * r, W: windowLevel(p.ch.W, ph, false), A: 0 },
      light: { L: 0.15 + 0.85 * l, R: 0.15 + 0.85 * r },
    };
  }
  const env = p.env ?? p.ch;
  return {
    lens: {
      L: windowLevel(p.ch.L, ph, false),
      R: windowLevel(p.ch.R, ph, false),
      W: windowLevel(p.ch.W, ph, false),
      A: windowLevel(p.ch.A, ph, false),
    },
    light: { L: windowLevel(env.L, ph, true), R: windowLevel(env.R, ph, true) },
  };
}

// ------------------------------------------------------------------------------------------
// Light rig of one car
// ------------------------------------------------------------------------------------------

const ON = {
  red: new THREE.Color(0xff2040),
  blue: new THREE.Color(0x2060ff),
  clear: new THREE.Color(0xfff6e0),
  amber: new THREE.Color(0xffa010),
};
/** Peak point-light intensity (one per side). */
const LIGHT_INTENSITY = 22;

/** Lens materials + point lights of one police car, flashed from a pattern. */
export class LightRig {
  private readonly amberIdle: THREE.Color[];
  constructor(
    readonly left: LightbarSide,
    readonly right: LightbarSide,
    readonly amber: THREE.MeshStandardMaterial[],
    readonly lights: { left: THREE.PointLight | null; right: THREE.PointLight | null },
    public pattern: string,
    readonly phase: number,
  ) {
    for (const s of [left, right]) for (const m of [s.red, s.blue, s.clear]) m.emissiveIntensity = 2.2;
    this.amberIdle = amber.map((m) => m.emissive.clone());
  }

  /** Set every lens / light for time `t` (s); `on` = siren / pursuing. */
  update(t: number, on: boolean): void {
    const p = PATTERNS[this.pattern] ?? PATTERNS["P-LED-QUAD"];
    const lv = on ? evalPattern(p, t + this.phase * p.period) : null;
    const set = (s: LightbarSide, k: number) => {
      s.red.emissive.copy(ON.red).multiplyScalar(k);
      s.blue.emissive.copy(ON.blue).multiplyScalar(k);
    };
    set(this.left, lv ? lv.lens.L : 0);
    set(this.right, lv ? lv.lens.R : 0);
    const w = lv ? lv.lens.W : 0;
    this.left.clear.emissive.copy(ON.clear).multiplyScalar(w);
    this.right.clear.emissive.copy(ON.clear).multiplyScalar(w);
    this.amber.forEach((m, i) => {
      if (lv && lv.lens.A > 0) m.emissive.copy(ON.amber).multiplyScalar(lv.lens.A);
      else m.emissive.copy(this.amberIdle[i]);
    });
    const { left, right } = this.lights;
    if (left) {
      left.color.copy(ON.red);
      left.intensity = lv ? LIGHT_INTENSITY * lv.light.L : 0;
    }
    if (right) {
      right.color.copy(ON.blue);
      right.intensity = lv ? LIGHT_INTENSITY * lv.light.R : 0;
    }
  }
}

// ------------------------------------------------------------------------------------------
// Non-sim police units
// ------------------------------------------------------------------------------------------

const FALLBACK_MODEL = "sentinel_cruiser";
/** Point lights shared by the nearest pursuing units (fixed count: no shader recompiles). */
const LIGHT_PAIRS = 2;
/** Units farther than this get no point lights (emissive only). */
const LIGHT_RANGE_M = 120;

/** Static materials of the unit models (mirrors render/panels.ts PanelBody colours). */
function staticMaterials(): Partial<Record<MaterialJson, THREE.Material>> {
  const std = (o: THREE.MeshStandardMaterialParameters) =>
    new THREE.MeshStandardMaterial({ flatShading: true, side: THREE.DoubleSide, ...o });
  return {
    paint: std({ color: 0x222428, metalness: 0.25, roughness: 0.55 }),
    livery: std({ color: 0xffffff, metalness: 0.25, roughness: 0.55 }),
    glass: std({ color: 0x8fb3c4, transparent: true, opacity: 0.45, metalness: 0.1, roughness: 0.1, depthWrite: false }),
    trim: std({ color: 0x1b1b1d, roughness: 0.9 }),
    headlight: std({ color: 0xfff6d0, emissive: 0x302a18 }),
    taillight: std({ color: 0x8a0a0a, emissive: 0x200000 }),
    rubber: std({ color: 0x141414, roughness: 0.95 }),
    metal: std({ color: 0x9da3a8, metalness: 0.7, roughness: 0.35 }),
    interior: std({ color: 0x2b2724, roughness: 1 }),
    chrome: std({ color: 0xe8ecef, metalness: 1, roughness: 0.12 }),
    carbon: std({ color: 0x1e2124, metalness: 0.3, roughness: 0.35 }),
    caliper: std({ color: 0x3a3c40, metalness: 0.2, roughness: 0.5 }),
    engine: std({ color: 0x3b3d40, metalness: 0.6, roughness: 0.6 }),
    emissive_amber: std({ color: 0xc87a10, emissive: 0x2a1600, roughness: 0.4 }),
    grille: std({ color: 0x0d0e10, roughness: 0.8 }),
    exhaust: std({ color: 0xb9b2a6, metalness: 0.9, roughness: 0.25 }),
    rim: std({ color: 0x9da3a8, metalness: 0.7, roughness: 0.35 }),
    spring: std({ color: 0x3a3c40, metalness: 0.3, roughness: 0.5 }),
    accent: std({ color: 0x8a1020, metalness: 0.25, roughness: 0.55 }),
  };
}

const LENS_SLOTS = new Set<MaterialJson>(["lightbar_red", "lightbar_blue", "lightbar_clear"]);

interface Template {
  carId: string;
  mesh: CarMeshJson;
  /** Merged geometry per static material slot (`county`: the material in the ACSO livery). */
  statics: { geom: THREE.BufferGeometry; mat: THREE.Material; county: THREE.Material }[];
  /** Merged lens geometry per side and colour. */
  lenses: { geom: THREE.BufferGeometry; side: "left" | "right"; colour: keyof LightbarSide }[];
  /** Amber parts (per-unit material so they flash). */
  amber: THREE.BufferGeometry | null;
  /** Decal carriers (cloned per unit: their UVs / material are per unit). */
  decals: THREE.Mesh[];
  anchors: { left: THREE.Vector3 | null; right: THREE.Vector3 | null };
}

interface UnitView {
  id: number;
  carId: string;
  /** Wears the Ashpeak County Sheriff livery (police_units() flag 4). */
  county: boolean;
  obj: THREE.Group;
  rig: LightRig | null;
  anchors: { left: THREE.Vector3 | null; right: THREE.Vector3 | null };
  seen: boolean;
  chasing: boolean;
  dist: number;
}

/** Geometry of one mesh-file part (chassis space, m). */
function partGeometry(pos: number[], uvs: number[], idx: number[], offset?: number[]): THREE.BufferGeometry {
  const g = new THREE.BufferGeometry();
  const p = new Float32Array(pos.length);
  for (let i = 0; i < pos.length; i++) p[i] = pos[i] * 1e-3 + (offset ? offset[i % 3] : 0);
  const uv = new Float32Array(uvs.length);
  for (let i = 0; i < uvs.length; i++) uv[i] = uvs[i] / 65535;
  g.setAttribute("position", new THREE.BufferAttribute(p, 3));
  g.setAttribute("uv", new THREE.BufferAttribute(uv, 2));
  g.setIndex(idx);
  return g;
}

/** Draws police_units() (units that are not sim vehicles) with the real police meshes. */
export class PoliceUnitsView {
  readonly group = new THREE.Group();
  private readonly templates = new Map<string, Template | "loading" | "missing">();
  private readonly units = new Map<number, UnitView>();
  private readonly mats = staticMaterials();
  private readonly heliMat = new THREE.MeshStandardMaterial({ color: 0x30343a, flatShading: true });
  private models: string[] | null = null;
  private readonly lights: THREE.PointLight[] = [];

  constructor(parent: THREE.Object3D) {
    parent.add(this.group);
    for (let i = 0; i < LIGHT_PAIRS * 2; i++) {
      const l = new THREE.PointLight(i % 2 ? 0x2060ff : 0xff2040, 0, 20, 2);
      this.lights.push(l);
      this.group.add(l);
    }
  }

  private modelList(): string[] {
    if (this.models === null) {
      try {
        this.models = sim.policeModels();
      } catch {
        this.models = []; // older wasm without police_models()
      }
    }
    return this.models;
  }

  /** Forget the model list (new session). */
  reset(): void {
    this.models = null;
    for (const u of this.units.values()) u.obj.removeFromParent();
    this.units.clear();
  }

  private template(carId: string): Template | null {
    const t = this.templates.get(carId);
    if (t === "missing" && carId !== FALLBACK_MODEL) return this.template(FALLBACK_MODEL);
    if (t === undefined) {
      this.templates.set(carId, "loading");
      void Promise.all([loadCarMesh(carId), fetch(`data/cars/${carId}.json`).then((r) => (r.ok ? r.json() : null)).catch(() => null)])
        .then(([mesh, def]) => this.templates.set(carId, mesh ? this.build(carId, mesh, def) : "missing"))
        .catch(() => this.templates.set(carId, "missing"));
      return null;
    }
    return typeof t === "object" ? t : null;
  }

  private build(carId: string, mesh: CarMeshJson, def: { wheels?: { position_m: number[] }[] } | null): Template {
    const bySlot = new Map<MaterialJson, THREE.BufferGeometry[]>();
    const lensParts = new Map<string, THREE.BufferGeometry[]>();
    const amber: THREE.BufferGeometry[] = [];
    const decals: THREE.Mesh[] = [];
    const sum = { left: new THREE.Vector3(), right: new THREE.Vector3() };
    const cnt = { left: 0, right: 0 };
    for (const p of mesh.parts) {
      let offset: number[] | undefined;
      if (p.wheel != null) {
        const w = def?.wheels?.[p.wheel];
        if (!w) continue;
        offset = w.position_m;
      }
      const g = partGeometry(p.positions_mm, p.uvs, p.indices, offset);
      if (p.name.startsWith("decal_")) {
        decals.push(Object.assign(new THREE.Mesh(g), { name: p.name }));
        continue;
      }
      if (LENS_SLOTS.has(p.material)) {
        g.computeBoundingBox();
        const c = g.boundingBox!.getCenter(new THREE.Vector3());
        const side = c.x < 0 ? "left" : "right";
        sum[side].add(c);
        cnt[side]++;
        const key = `${side}:${p.material.slice("lightbar_".length)}`;
        (lensParts.get(key) ?? lensParts.set(key, []).get(key)!).push(g);
        continue;
      }
      if (p.material === "emissive_amber") {
        amber.push(g);
        continue;
      }
      (bySlot.get(p.material) ?? bySlot.set(p.material, []).get(p.material)!).push(g);
    }
    // City (PVPD) and county (ACSO) livery + paint materials; each unit picks one set.
    const skin = (county: boolean) => {
      const tex = liveryTexture(mesh, 8, county);
      const livery = (this.mats.livery as THREE.MeshStandardMaterial).clone();
      livery.map = tex;
      const paint = (this.mats.paint as THREE.MeshStandardMaterial).clone();
      const base = liveryBaseColour(mesh, county);
      if (base) paint.color.copy(base);
      if (!tex) livery.color.copy(paint.color);
      return { livery, paint };
    };
    const city = skin(false);
    const county = skin(true);
    const tint = (slot: MaterialJson, m: THREE.Material) => {
      const rgb = mesh.tints?.[slot];
      if (!rgb) return m;
      const c = (m as THREE.MeshStandardMaterial).clone();
      c.color.setRGB(rgb[0] / 255, rgb[1] / 255, rgb[2] / 255, THREE.SRGBColorSpace);
      return c;
    };
    const statics: Template["statics"] = [];
    for (const [slot, gs] of bySlot) {
      const geom = mergeGeometries(gs);
      if (!geom) continue;
      const other = tint(slot, this.mats[slot] ?? city.paint);
      const pick = (k: { livery: THREE.Material; paint: THREE.Material }) =>
        slot === "livery" ? k.livery : slot === "paint" ? k.paint : other;
      statics.push({ geom, mat: pick(city), county: pick(county) });
    }
    const lenses: Template["lenses"] = [];
    for (const [key, gs] of lensParts) {
      const [side, colour] = key.split(":") as ["left" | "right", keyof LightbarSide];
      const geom = mergeGeometries(gs);
      if (geom) lenses.push({ geom, side, colour });
    }
    return {
      carId,
      mesh,
      statics,
      lenses,
      amber: amber.length ? mergeGeometries(amber) : null,
      decals,
      anchors: {
        left: cnt.left ? sum.left.divideScalar(cnt.left) : null,
        right: cnt.right ? sum.right.divideScalar(cnt.right) : null,
      },
    };
  }

  private makeUnit(id: number, t: Template, county: boolean): UnitView {
    const obj = new THREE.Group();
    for (const s of t.statics) {
      const m = new THREE.Mesh(s.geom, county ? s.county : s.mat);
      if ((s.mat as THREE.MeshStandardMaterial).transparent) m.renderOrder = 1;
      obj.add(m);
    }
    const lens = (color: number) =>
      new THREE.MeshStandardMaterial({ color, emissive: 0x000000, roughness: 0.3, transparent: true, opacity: 0.9, flatShading: true, side: THREE.DoubleSide });
    const sides: { left: LightbarSide; right: LightbarSide } = {
      left: { red: lens(0x661018), blue: lens(0x102266), clear: lens(0xbfc4c8) },
      right: { red: lens(0x661018), blue: lens(0x102266), clear: lens(0xbfc4c8) },
    };
    for (const l of t.lenses) {
      const m = new THREE.Mesh(l.geom, sides[l.side][l.colour]);
      m.renderOrder = 1;
      obj.add(m);
    }
    const amberMats: THREE.MeshStandardMaterial[] = [];
    if (t.amber) {
      const am = (this.mats.emissive_amber as THREE.MeshStandardMaterial).clone();
      amberMats.push(am);
      obj.add(new THREE.Mesh(t.amber, am));
    }
    const decalRoot = new THREE.Group();
    for (const d of t.decals) decalRoot.add(Object.assign(new THREE.Mesh(d.geometry.clone()), { name: d.name }));
    applyUnitDecals(decalRoot, t.mesh, id, county);
    obj.add(decalRoot);
    const rig = new LightRig(sides.left, sides.right, amberMats, { left: null, right: null }, patternFor(t.carId), phaseFor(id));
    this.group.add(obj);
    return { id, carId: t.carId, county, obj, rig, anchors: t.anchors, seen: true, chasing: false, dist: Infinity };
  }

  private makeHeli(id: number): UnitView {
    const obj = new THREE.Group();
    const body = new THREE.Mesh(new THREE.BoxGeometry(1.8, 4.2, 1.6), this.heliMat);
    body.position.z = 0.8;
    const boom = new THREE.Mesh(new THREE.BoxGeometry(0.3, 4, 0.3), this.heliMat);
    boom.position.set(0, -3.5, 1.1);
    const rotor = new THREE.Mesh(new THREE.BoxGeometry(0.2, 10, 0.05), this.heliMat);
    rotor.position.z = 1.8;
    obj.add(body, boom, rotor);
    this.group.add(obj);
    return { id, carId: "heli", county: false, obj, rig: null, anchors: { left: null, right: null }, seen: true, chasing: false, dist: Infinity };
  }

  /**
   * Draw `units` (police_units(), POLICE_UNIT_STRIDE floats each) at time `timeS`; `camSim`
   * (camera position in sim space) picks which pursuing units get the point lights.
   */
  update(units: Float32Array, timeS: number, camSim?: THREE.Vector3): void {
    const stride = POLICE_UNIT_STRIDE;
    const n = Math.floor(units.length / stride);
    let models = this.modelList();
    for (const u of this.units.values()) u.seen = false;
    for (let i = 0; i < n; i++) {
      const o = i * stride;
      const flags = units[o + PU.flags];
      const heli = (flags & 1) !== 0;
      const county = !heli && countySkin((flags & 4) !== 0);
      const id = Math.round(units[o + PU.unitId] ?? i);
      const mi = Math.round(units[o + PU.model] ?? -1);
      if (mi >= models.length) {
        // New session (or the list was not ready yet): fetch the model list again.
        this.models = null;
        models = this.modelList();
      }
      const carId = heli ? "heli" : (mi >= 0 && models[mi]) || FALLBACK_MODEL;
      const t = heli ? null : this.template(carId);
      const want = heli ? "heli" : t?.carId;
      let u = this.units.get(id);
      if (u && want && (u.carId !== want || u.county !== county)) {
        // Model (e.g. the model list arrived) or agency changed: rebuild.
        u.obj.removeFromParent();
        this.units.delete(id);
        u = undefined;
      }
      if (!u) {
        if (heli) u = this.makeHeli(id);
        else if (t) u = this.makeUnit(id, t, county);
        else continue; // mesh still loading
        this.units.set(id, u);
      }
      u.seen = true;
      u.obj.visible = true;
      u.obj.position.set(units[o + PU.x], units[o + PU.y], units[o + PU.z]);
      u.obj.rotation.z = units[o + PU.yaw];
      u.chasing = (flags & 2) !== 0;
      u.dist = camSim ? camSim.distanceTo(u.obj.position) : 0;
      if (u.rig) {
        u.rig.pattern = patternFor(u.carId, Math.abs(units[o + PU.speed]) < 0.5);
        u.rig.update(timeS, u.chasing);
      }
    }
    for (const [id, u] of this.units)
      if (!u.seen) {
        u.obj.removeFromParent();
        this.units.delete(id);
      }
    // Point lights: nearest pursuing car units within range.
    const lit = [...this.units.values()]
      .filter((u) => u.chasing && u.rig && u.dist < LIGHT_RANGE_M)
      .sort((a, b) => a.dist - b.dist)
      .slice(0, LIGHT_PAIRS);
    for (let k = 0; k < LIGHT_PAIRS; k++) {
      const u = lit[k];
      const [ll, lr] = [this.lights[2 * k], this.lights[2 * k + 1]];
      if (!u || !u.rig) {
        ll.intensity = 0;
        lr.intensity = 0;
        continue;
      }
      u.obj.updateMatrix();
      for (const [light, a] of [[ll, u.anchors.left], [lr, u.anchors.right]] as const) {
        const anchor = a ?? new THREE.Vector3(0, 0, 1.6);
        light.position.copy(anchor).add(new THREE.Vector3(0, 0, 0.25)).applyMatrix4(u.obj.matrix);
      }
      const p = PATTERNS[u.rig.pattern] ?? PATTERNS["P-LED-QUAD"];
      const lv = evalPattern(p, timeS + u.rig.phase * p.period);
      ll.color.copy(ON.red);
      lr.color.copy(ON.blue);
      ll.intensity = LIGHT_INTENSITY * lv.light.L;
      lr.intensity = LIGHT_INTENSITY * lv.light.R;
    }
  }
}
