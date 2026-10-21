// Instanced civilian traffic (wave-2 style-traffic). Spec: docs/design/vehicles/research_traffic.md
// §2 (body mix), §4 (visual-only variants), §5 (palette), §6 (instancing).
//
// Every traffic model's panel mesh (data/panels/meshes/<car>.json) is merged into ONE static
// geometry (no skinning: instanced agents carry no damage) and drawn with one InstancedMesh per
// model and LOD. Materials are baked into vertex attributes and resolved in one shader
// (MeshStandardMaterial + onBeforeCompile), so a model costs one draw call per LOD:
//   color   (vec3)  albedo; white where the surface takes the per-instance paint
//   aSurf   (vec4)  x paint group (0 none, 1 body, 2 hood, 3 front-left door, 4 trunk / tailgate,
//                   5 front-right fender), y lamp kind (0 none, 1 head, 2 tail, 3 amber left,
//                   4 amber right, 5 sign, 6 beacon), z roughness, w metalness
//   instanceColor   paint
//   iAlt    (vec4)  alt paint rgb (beater primer panels, delivery band), w = mask of paint groups
//                   that take it + 64 × livery kind
//   iParams (vec4)  x rust 0–1, y dirt 0–1, z lamp bits (1 brake, 2 hazard, 4 turn L, 8 turn R),
//                   w livery text row
//   iPbr    (vec2)  paint roughness, metalness
// Liveries are procedural in the fragment shader (taxi checker band, delivery band) plus one
// shared word atlas (canvas) for van side lettering. Visual-only props (taxi sign, roof rails,
// roof box, ladder racks, toolbox, beacon, tonneau cover) are separate InstancedMeshes placed on
// per-model anchors measured from the mesh (roof / bed bounds).
//
// LOD: full mesh within LOD_NEAR_M, beyond it an automatic hull (≈ 150 triangles: 10 loft
// stations from the body's slice bounds, glass band, lamp boxes, 8-sided wheels).
//
// Variety: model = the sim's per-agent model (traffic_agents() stride 7 + traffic_models()),
// else a weighted pick from the agent id (or a spawn-position seed on old builds). The variant
// and paint come from `trafficLook(carId, variant, id)` — the same function a promoted
// (physical) car should use (`trafficPaint`), so it keeps its colour.
//
// Coordinates: sim space (+Z up, +Y forward); the meshes live under scene.ts's `world` group.
import * as THREE from "three";
import * as wasm from "../wasm/pkg/wp_wasm.js";
import { AGENT_FLAG, AGENT_STRIDE } from "../wasm/layout";
import { loadCarMesh, type CarMeshJson, type MaterialJson, type MeshPartJson } from "./panels";

/** Civilian traffic models with styles in tools/assets/src/styles/traffic.rs. */
export const TRAFFIC_CARS = ["debug_sedan", "junkyard_beater", "tansy_13", "furrow_24", "hauler_1500", "drayman_cargo"];
/**
 * Body-type weights when the sim does not pick a model (research §2 "per-model physics
 * mapping"; box truck / bus share folded into the van).
 */
const MODEL_WEIGHTS: Record<string, number> = {
  debug_sedan: 29, tansy_13: 26, hauler_1500: 16, drayman_cargo: 19, junkyard_beater: 6, furrow_24: 4,
};

/**
 * Floats per agent in the loaded wasm build's traffic_agents(): layout.ts AGENT_STRIDE (7, with
 * agent id + model) when the build has traffic_models(), else the older 5.
 */
export function trafficStride(): number {
  return typeof (wasm as unknown as { traffic_models?: unknown }).traffic_models === "function" ? AGENT_STRIDE : 5;
}

/** Instances per model and LOD (= scene.ts MAX_AGENTS). */
const MAX_INSTANCES = 1024;
/** Full mesh closer than this (m), hull beyond. */
const LOD_NEAR_M = 55;
/** Props (signs, racks) are drawn closer than this (m). */
const PROP_FAR_M = 160;
/** Agents farther than this (m) are not drawn. */
const DRAW_FAR_M = 700;

// ------------------------------------------------------------------------------------------
// Looks: variant + paint (deterministic from car id, variant byte and agent id)
// ------------------------------------------------------------------------------------------

/** One paint: sRGB hex, metalness, roughness. */
interface Paint {
  hex: number;
  metal: number;
  rough: number;
}
type PaintTable = [Paint, number][];

const p = (hex: number, metal: number, rough: number): Paint => ({ hex, metal, rough });
/** Research §5 base civilian palette (weights per 100). */
const CIVILIAN: PaintTable = [
  [p(0xe9ebec, 0, 0.35), 17], [p(0xf1efe8, 0.15, 0.28), 8], [p(0x18191c, 0, 0.3), 12], [p(0x1e2024, 0.6, 0.3), 8],
  [p(0x4a4e54, 0.6, 0.35), 10], [p(0x8a8d8f, 0.05, 0.4), 7], [p(0x33363b, 0.6, 0.32), 5], [p(0xb4b8bc, 0.75, 0.3), 9],
  [p(0x1d2a48, 0.5, 0.32), 5], [p(0x2456a8, 0.5, 0.3), 3], [p(0xa3141c, 0, 0.32), 4], [p(0x5a1418, 0.5, 0.33), 3],
  [p(0x1f3a2c, 0.4, 0.35), 2], [p(0xb8a888, 0.55, 0.35), 2], [p(0x4b3628, 0.45, 0.38), 1.5], [p(0xd8741c, 0.2, 0.32), 1],
  [p(0xe0b81c, 0.2, 0.32), 0.5], [p(0x1f7a7a, 0.4, 0.3), 1], [p(0x7fae2a, 0.4, 0.3), 0.5], [p(0x4a2c6a, 0.4, 0.3), 0.5],
];
/** Superminis: accents ×2 (research §5 rule 4). */
const BRIGHT: PaintTable = CIVILIAN.map(([c, w]) => [c, c.hex === 0xa3141c || c.hex === 0x2456a8 || w <= 1 ? w * 2 : w]);
/** Pickups / SUVs: grey-scale ≈ 80 %. */
const GREYSCALE: PaintTable = CIVILIAN.map(([c, w], i) => [c, i < 8 ? w * 1.6 : w * 0.6]);
/** Research §5 faded / beater palette. */
const FADED: PaintTable = [
  [p(0x9a4a44, 0.05, 0.7), 16], [p(0x5e3434, 0.15, 0.65), 12], [p(0x9a9c9a, 0.3, 0.6), 16], [p(0xd8d2c0, 0, 0.6), 14],
  [p(0x3a4258, 0.15, 0.62), 10], [p(0x9c8660, 0.25, 0.62), 10], [p(0x3c5040, 0.15, 0.65), 8], [p(0x8aa2b8, 0.2, 0.6), 6],
  [p(0x252525, 0, 0.75), 8],
];
const PRIMER = [p(0x7d7f7c, 0, 0.9), p(0x7a3b2a, 0, 0.9)];
const WHITE = p(0xe9ebec, 0, 0.35);
/** Vans: white ≥ 75 %. */
const VAN: PaintTable = [[WHITE, 75], [p(0xb4b8bc, 0.75, 0.3), 10], [p(0x1d2a48, 0.5, 0.32), 5], [p(0x5a1418, 0.5, 0.33), 5], [p(0x8a8d8f, 0.05, 0.4), 5]];
/** Work trucks: white 60 %, silver 20 %, municipal red / yellow 20 %. */
const WORK: PaintTable = [[WHITE, 60], [p(0xb4b8bc, 0.75, 0.3), 20], [p(0xa3141c, 0, 0.32), 10], [p(0xe0b81c, 0.2, 0.32), 10]];
/** Fleet / rental: white 55, silver 30, grey 15; never heavy flake. */
const FLEET: PaintTable = [[WHITE, 55], [p(0xb4b8bc, 0.4, 0.3), 30], [p(0x8a8d8f, 0.05, 0.4), 15]];
/** Ex-fleet "is that a cop?" sedan. */
const EX_FLEET: PaintTable = [[WHITE, 35], [p(0x18191c, 0, 0.3), 45], [p(0x1d2a48, 0.3, 0.32), 20]];
const TAXI_YELLOW = p(0xe8b923, 0.1, 0.3);
/** Delivery / trade accent colours (band, lettering). */
const ACCENTS = [0xb3202a, 0x1f4fa0, 0x2b7a3a, 0x6a4a2a, 0xd8741c];

/** Visual-only props (research §4). */
export type TrafficProp = "taxi_sign" | "roof_rails" | "roof_box" | "pickup_rack" | "toolbox" | "beacon" | "tonneau" | "van_rack";
/** Livery kinds drawn by the shader. */
const LIVERY = { none: 0, taxi: 1, band: 2, text: 3 } as const;

/** Everything visual about one traffic car, reproducible from (carId, variant, id). */
export interface TrafficLook {
  /** Car id (model). */
  carId: string;
  /** Variant name (research §4 code, e.g. "S2 taxi"). */
  variant: string;
  /** Body paint, sRGB hex. */
  paint: number;
  metalness: number;
  roughness: number;
  /** Alt paint (primer panels / livery band), sRGB hex. */
  alt: number;
  /** Paint groups (bits 2..5, see aSurf.x) painted `alt`. */
  altMask: number;
  livery: number;
  /** Row of the word atlas for side lettering (-1 none). */
  textRow: number;
  rust: number;
  dirt: number;
  props: TrafficProp[];
  /** Prop colour (roof box, rack), sRGB hex. */
  propColor: number;
}

/** 32-bit integer hash (lowbias32). */
function hash32(x: number): number {
  x = (x ^ (x >>> 16)) >>> 0;
  x = Math.imul(x, 0x7feb352d) >>> 0;
  x = (x ^ (x >>> 15)) >>> 0;
  x = Math.imul(x, 0x846ca68b) >>> 0;
  return (x ^ (x >>> 16)) >>> 0;
}

/** Small deterministic stream of floats in [0, 1) from a seed. */
class Dice {
  private s: number;
  constructor(seed: number) {
    this.s = hash32(seed) || 1;
  }
  next(): number {
    this.s = hash32(this.s + 0x9e3779b9);
    return this.s / 4294967296;
  }
  pick<T>(table: [T, number][]): T {
    let total = 0;
    for (const [, w] of table) total += w;
    let r = this.next() * total;
    for (const [v, w] of table) {
      r -= w;
      if (r < 0) return v;
    }
    return table[table.length - 1][0];
  }
}

function stringHash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619) >>> 0;
  return h;
}

/** Variants per model: [name, weight]. Unknown models get the plain civilian look. */
const VARIANTS: Record<string, [string, number][]> = {
  debug_sedan: [["S1 family", 50], ["S2 taxi", 14], ["S4 fleet", 14], ["S5 roof box", 10], ["S6 ex-fleet", 12]],
  junkyard_beater: [["B1 stock", 35], ["B2 mismatched", 35], ["B3 rusty", 30]],
  tansy_13: [["H1 supermini", 55], ["H2 rails", 25], ["H3 delivery", 20]],
  furrow_24: [["P1 old pickup", 55], ["B3 rusty", 15], ["P2 work", 30]],
  hauler_1500: [["F1 crew cab", 50], ["F4 tonneau", 20], ["P2 work", 30]],
  drayman_cargo: [["V1 plain", 45], ["V2 delivery", 30], ["V3 trade", 25]],
};

/** Word atlas rows (invented generic words only; no real companies). */
const WORDS = ["PARCEL EXPRESS", "FRESH FOODS", "PLUMBING  555-0142", "ELECTRIC  555-0177", "LOCKSMITH  555-0108", "CITY COURIER", "BAKERY", "MOVING & STORAGE"];

/**
 * The look of traffic car `carId` with colour `variant` byte (traffic_agents() flags bits 8–15)
 * and stable agent `id`. Pure function: a promoted (physical) car gets the same look.
 */
export function trafficLook(carId: string, variant: number, id: number): TrafficLook {
  const d = new Dice(hash32(id * 2654435761 + variant * 40503) ^ stringHash(carId));
  const vname = d.pick(VARIANTS[carId] ?? [["base", 1]]);
  const look: TrafficLook = {
    carId, variant: vname, paint: WHITE.hex, metalness: 0, roughness: 0.35, alt: 0x7d7f7c, altMask: 0,
    livery: LIVERY.none, textRow: -1, rust: 0, dirt: 0.1 + 0.2 * d.next(), props: [], propColor: 0x2a2c30,
  };
  const set = (c: Paint) => {
    look.paint = c.hex;
    look.metalness = c.metal;
    look.roughness = c.rough;
  };
  const table: PaintTable =
    carId === "tansy_13" ? BRIGHT : carId === "hauler_1500" || carId === "furrow_24" ? GREYSCALE : carId === "drayman_cargo" ? VAN : CIVILIAN;
  set(d.pick(table));
  switch (vname) {
    case "S2 taxi":
      set(d.next() < 0.8 ? TAXI_YELLOW : WHITE);
      look.livery = LIVERY.taxi;
      look.props.push("taxi_sign");
      look.dirt = 0.15;
      break;
    case "S4 fleet":
      set(d.pick(FLEET));
      break;
    case "S5 roof box":
      look.props.push("roof_rails", "roof_box");
      look.propColor = d.next() < 0.5 ? 0x1b1c1f : 0x9da3a8;
      break;
    case "S6 ex-fleet":
      set(d.pick(EX_FLEET));
      break;
    case "B1 stock":
    case "B2 mismatched":
    case "B3 rusty":
    case "P1 old pickup": {
      const faded = carId === "junkyard_beater" || d.next() < 0.6;
      if (faded) set(d.pick(FADED));
      look.dirt = 0.5 + 0.4 * d.next();
      if (vname === "B2 mismatched") {
        look.alt = d.next() < 0.6 ? PRIMER[d.next() < 0.6 ? 0 : 1].hex : d.pick(FADED).hex;
        // 1–3 of hood, front-left door, trunk, front-right fender.
        const bits = [4, 8, 16, 32];
        look.altMask = bits[Math.floor(d.next() * 4)];
        if (d.next() < 0.6) look.altMask |= bits[Math.floor(d.next() * 4)];
        if (d.next() < 0.3) look.altMask |= bits[Math.floor(d.next() * 4)];
      }
      if (vname === "B3 rusty" || d.next() < 0.15) look.rust = 0.55 + 0.4 * d.next();
      break;
    }
    case "H2 rails":
      look.props.push("roof_rails");
      break;
    case "H3 delivery":
      set(d.next() < 0.8 ? WHITE : p(0xa3141c, 0, 0.32));
      look.props.push("roof_box");
      look.propColor = d.next() < 0.5 ? 0xb3202a : 0xe9ebec;
      break;
    case "P2 work":
      set(d.pick(WORK));
      look.props.push("pickup_rack", "toolbox", "beacon");
      look.propColor = 0x9da3a8;
      look.dirt = 0.4 + 0.3 * d.next();
      break;
    case "F4 tonneau":
      look.props.push("tonneau");
      break;
    case "V2 delivery":
      set(WHITE);
      look.alt = ACCENTS[Math.floor(d.next() * ACCENTS.length)];
      look.livery = LIVERY.band;
      look.textRow = [0, 1, 5, 6, 7][Math.floor(d.next() * 5)];
      break;
    case "V3 trade":
      set(d.next() < 0.85 ? WHITE : d.pick(VAN));
      look.alt = ACCENTS[Math.floor(d.next() * 2)];
      look.livery = LIVERY.text;
      look.textRow = 2 + Math.floor(d.next() * 3);
      look.props.push("van_rack");
      look.propColor = 0x9da3a8;
      look.dirt = 0.3 + 0.2 * d.next();
      break;
  }
  return look;
}

/**
 * Paint of traffic car `carId` with colour `variant` and agent `id` (what the instanced twin
 * shows): sRGB hex + PBR. For promoted (physical) traffic in vehicle_view / main.
 */
export function trafficPaint(carId: string, variant: number, id: number): { color: number; metalness: number; roughness: number } {
  const l = trafficLook(carId, variant, id);
  return { color: l.paint, metalness: l.metalness, roughness: l.roughness };
}

/** Pick a model for an agent when the sim does not (weighted by research §2). */
function pickModel(seed: number, ids: string[]): string {
  const d = new Dice(seed ^ 0x51ed27);
  return d.pick(ids.map((id) => [id, MODEL_WEIGHTS[id] ?? 5] as [string, number]));
}

// ------------------------------------------------------------------------------------------
// Geometry
// ------------------------------------------------------------------------------------------

/** Per-material look in the merged geometry: albedo, roughness, metalness, lamp kind, paint. */
interface Surf {
  rgb: number;
  rough: number;
  metal: number;
  lamp: number;
  paint: boolean;
}
const SURF: Record<MaterialJson, Surf | null> = {
  paint: { rgb: 0xffffff, rough: 0.4, metal: 0, lamp: 0, paint: true },
  livery: { rgb: 0xffffff, rough: 0.4, metal: 0, lamp: 0, paint: true },
  glass: { rgb: 0x1b242c, rough: 0.18, metal: 0.1, lamp: 0, paint: false },
  trim: { rgb: 0x161719, rough: 0.6, metal: 0, lamp: 0, paint: false },
  headlight: { rgb: 0xe6e4dc, rough: 0.2, metal: 0.2, lamp: 1, paint: false },
  taillight: { rgb: 0x6a0c0c, rough: 0.25, metal: 0.1, lamp: 2, paint: false },
  rubber: { rgb: 0x151515, rough: 0.9, metal: 0, lamp: 0, paint: false },
  metal: { rgb: 0x9da3a8, rough: 0.35, metal: 0.7, lamp: 0, paint: false },
  interior: null,
  chrome: { rgb: 0xd9dde0, rough: 0.12, metal: 1, lamp: 0, paint: false },
  carbon: { rgb: 0x1e2124, rough: 0.35, metal: 0.3, lamp: 0, paint: false },
  caliper: { rgb: 0x3a3c40, rough: 0.5, metal: 0.2, lamp: 0, paint: false },
  engine: null,
  lightbar_red: { rgb: 0x661018, rough: 0.3, metal: 0, lamp: 0, paint: false },
  lightbar_blue: { rgb: 0x102266, rough: 0.3, metal: 0, lamp: 0, paint: false },
  lightbar_clear: { rgb: 0xbfc4c8, rough: 0.3, metal: 0, lamp: 0, paint: false },
  emissive_amber: { rgb: 0xc87a10, rough: 0.35, metal: 0, lamp: 3, paint: false },
  grille: { rgb: 0x0d0e10, rough: 0.8, metal: 0, lamp: 0, paint: false },
  exhaust: { rgb: 0xb9b2a6, rough: 0.25, metal: 0.9, lamp: 0, paint: false },
  rim: { rgb: 0x9da3a8, rough: 0.35, metal: 0.7, lamp: 0, paint: false },
  spring: { rgb: 0x3a3c40, rough: 0.5, metal: 0.2, lamp: 0, paint: false },
  // Second body colour (mesh `tints.accent` when set).
  accent: { rgb: 0x2a2c30, rough: 0.4, metal: 0.2, lamp: 0, paint: false },
  underglow: null,
};
/** Part kinds never drawn for instanced traffic (hidden or too small to matter). */
const SKIP_KINDS = new Set(["interior", "engine", "suspension", "brake_disc", "caliper", "upright"]);
/** Paint group of a part (aSurf.x) for mismatched-panel variants. */
function paintGroup(name: string): number {
  if (name === "hood") return 2;
  if (name === "door_fl") return 3;
  if (name === "trunk") return 4;
  if (name === "fender_fr") return 5;
  return 1;
}

/** Growable vertex buffers in the traffic attribute layout. */
class GeoBuilder {
  pos: number[] = [];
  col: number[] = [];
  surf: number[] = [];
  idx: number[] = [];
  private readonly c = new THREE.Color();

  /** Add a vertex; returns its index. */
  vert(x: number, y: number, z: number, s: Surf, group: number, lamp = s.lamp): number {
    this.pos.push(x, y, z);
    this.c.setHex(s.rgb, THREE.SRGBColorSpace);
    this.col.push(this.c.r, this.c.g, this.c.b);
    this.surf.push(s.paint ? group : 0, lamp, s.rough, s.metal);
    return this.pos.length / 3 - 1;
  }

  /** Quad a-b-c-d (CCW from outside). */
  quad(a: number, b: number, c: number, d: number): void {
    this.idx.push(a, b, c, a, c, d);
  }

  /** Axis-aligned box. */
  box(lo: [number, number, number], hi: [number, number, number], s: Surf, group = 1, lamp = s.lamp): void {
    const v: number[] = [];
    for (let i = 0; i < 8; i++)
      v.push(this.vert(i & 1 ? hi[0] : lo[0], i & 2 ? hi[1] : lo[1], i & 4 ? hi[2] : lo[2], s, group, lamp));
    const f = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    for (const q of f) this.quad(v[q[0]], v[q[1]], v[q[2]], v[q[3]]);
  }

  /** Cylinder along X (wheel), `n` sides. */
  wheel(c: [number, number, number], r: number, w: number, n: number, s: Surf): void {
    const a: number[] = [];
    const b: number[] = [];
    for (let i = 0; i < n; i++) {
      const t = (i / n) * Math.PI * 2;
      a.push(this.vert(c[0] - w / 2, c[1] + r * Math.cos(t), c[2] + r * Math.sin(t), s, 0));
      b.push(this.vert(c[0] + w / 2, c[1] + r * Math.cos(t), c[2] + r * Math.sin(t), s, 0));
    }
    for (let i = 0; i < n; i++) {
      const j = (i + 1) % n;
      this.quad(a[i], a[j], b[j], b[i]);
    }
    for (let i = 1; i < n - 1; i++) {
      this.idx.push(a[0], a[i + 1], a[i]);
      this.idx.push(b[0], b[i], b[i + 1]);
    }
  }

  build(): THREE.BufferGeometry {
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(this.pos, 3));
    g.setAttribute("color", new THREE.Float32BufferAttribute(this.col, 3));
    g.setAttribute("aSurf", new THREE.Float32BufferAttribute(this.surf, 4));
    g.setIndex(this.idx.length > 65535 ? new THREE.Uint32BufferAttribute(this.idx, 1) : new THREE.Uint16BufferAttribute(this.idx, 1));
    g.computeBoundingSphere();
    return g;
  }
}

/** Min / max of a part's positions (m). */
function bounds(part: MeshPartJson | undefined): { lo: THREE.Vector3; hi: THREE.Vector3 } | null {
  if (!part) return null;
  const lo = new THREE.Vector3(Infinity, Infinity, Infinity);
  const hi = new THREE.Vector3(-Infinity, -Infinity, -Infinity);
  const q = part.positions_mm;
  for (let i = 0; i < q.length; i += 3) {
    lo.min(new THREE.Vector3(q[i] * 1e-3, q[i + 1] * 1e-3, q[i + 2] * 1e-3));
    hi.max(new THREE.Vector3(q[i] * 1e-3, q[i + 1] * 1e-3, q[i + 2] * 1e-3));
  }
  return { lo, hi };
}

/** Where props go on a model, and the shader livery frame. */
interface Anchors {
  /** Roof: centre x/y, top z, half width, length. */
  roof: { y: number; z: number; hw: number; len: number };
  /** Open bed (pickups): centre y, rail z, half width, length; null without one. */
  bed: { y: number; z: number; hw: number; len: number } | null;
  /** Body bounds. */
  lo: THREE.Vector3;
  hi: THREE.Vector3;
  /** Taxi checker band centre z, delivery band top z, text rect (y0, y1, z0, z1). */
  checkZ: number;
  bandZ: number;
  text: [number, number, number, number];
}

interface CarDefJson {
  wheels: { position_m: [number, number, number]; radius_m: number; width_m: number }[];
}

/** Merge a car mesh into the near-LOD geometry; measure anchors; build the far-LOD hull. */
function buildModel(mesh: CarMeshJson, def: CarDefJson | null): { near: THREE.BufferGeometry; far: THREE.BufferGeometry; anchors: Anchors } {
  const g = new GeoBuilder();
  const tint = (m: MaterialJson, s: Surf): Surf => {
    const t = mesh.tints?.[m];
    return t ? { ...s, rgb: (t[0] << 16) | (t[1] << 8) | t[2] } : s;
  };
  const lo = new THREE.Vector3(Infinity, Infinity, Infinity);
  const hi = new THREE.Vector3(-Infinity, -Infinity, -Infinity);
  const byName = new Map(mesh.parts.map((q) => [q.name, q]));
  for (const part of mesh.parts) {
    const s0 = SURF[part.material];
    if (!s0 || SKIP_KINDS.has(part.kind)) continue;
    const s = tint(part.material, s0);
    const group = paintGroup(part.name);
    const off = part.wheel != null && def?.wheels[part.wheel] ? def.wheels[part.wheel].position_m : [0, 0, 0];
    const base = g.pos.length / 3;
    const q = part.positions_mm;
    for (let i = 0; i < q.length; i += 3) {
      const x = q[i] * 1e-3 + off[0];
      const y = q[i + 1] * 1e-3 + off[1];
      const z = q[i + 2] * 1e-3 + off[2];
      let lamp = s.lamp;
      if (lamp === 3 && x > 0) lamp = 4; // amber lamps: left / right indicator
      g.vert(x, y, z, s, group, lamp);
      if (part.wheel == null) {
        lo.min(new THREE.Vector3(x, y, z));
        hi.max(new THREE.Vector3(x, y, z));
      }
    }
    for (const i of part.indices) g.idx.push(base + i);
  }

  // Anchors.
  const roofB = bounds(byName.get("roof"));
  const roof = roofB
    ? { y: (roofB.lo.y + roofB.hi.y) / 2, z: roofB.hi.z, hw: (roofB.hi.x - roofB.lo.x) / 2, len: roofB.hi.y - roofB.lo.y }
    : { y: 0, z: hi.z, hw: 0.6, len: 1.2 };
  const bedB = bounds(byName.get("bed_floor"));
  const quarter = bounds(byName.get("quarter_rl"));
  const bed = bedB
    ? { y: (bedB.lo.y + bedB.hi.y) / 2, z: quarter ? quarter.hi.z : bedB.hi.z + 0.45, hw: (bedB.hi.x - bedB.lo.x) / 2, len: bedB.hi.y - bedB.lo.y }
    : null;
  const doorF = bounds(byName.get("door_fl"));
  const zBelt = doorF ? doorF.hi.z : 0.5 * hi.z;
  const zDoor = doorF ? doorF.lo.z : 0.3 * hi.z;
  const cabRear = doorF ? doorF.lo.y : 0;
  const anchors: Anchors = {
    roof, bed, lo, hi,
    checkZ: zDoor + 0.62 * (zBelt - zDoor),
    bandZ: zDoor + 0.45 * (zBelt - zDoor),
    text: [lo.y + 0.35, cabRear - 0.25, zBelt + 0.05, zBelt + 0.45],
  };
  return { near: g.build(), far: buildHull(mesh, def, lo, hi), anchors };
}

/**
 * Far LOD: 10 loft stations from the body's slice bounds (paint below the glass base, a dark
 * glass band, roof), head / tail lamp boxes and 8-sided wheels. ≈ 150–200 triangles.
 */
function buildHull(mesh: CarMeshJson, def: CarDefJson | null, lo: THREE.Vector3, hi: THREE.Vector3): THREE.BufferGeometry {
  const N = 10;
  const len = hi.y - lo.y;
  interface Slice { zLo: number; zTop: number; hw: number; rw: number; glass: number }
  const sl: Slice[] = Array.from({ length: N }, () => ({ zLo: Infinity, zTop: -Infinity, hw: 0, rw: 0, glass: Infinity }));
  const body = mesh.parts.filter((q) => q.wheel == null && (q.kind === "panel" || q.kind === "body" || q.kind === "glass"));
  for (const part of body) {
    const q = part.positions_mm;
    for (let i = 0; i < q.length; i += 3) {
      const x = Math.abs(q[i] * 1e-3);
      const y = q[i + 1] * 1e-3;
      const z = q[i + 2] * 1e-3;
      const k = Math.min(N - 1, Math.max(0, Math.floor(((y - lo.y) / len) * N)));
      const s = sl[k];
      s.zLo = Math.min(s.zLo, z);
      s.zTop = Math.max(s.zTop, z);
      if (part.kind === "glass") s.glass = Math.min(s.glass, z);
      else if (part.name !== "bed_floor") s.hw = Math.max(s.hw, x);
    }
  }
  for (const part of body) {
    const q = part.positions_mm;
    for (let i = 0; i < q.length; i += 3) {
      const k = Math.min(N - 1, Math.max(0, Math.floor(((q[i + 1] * 1e-3 - lo.y) / len) * N)));
      if (q[i + 2] * 1e-3 > sl[k].zTop - 0.06) sl[k].rw = Math.max(sl[k].rw, Math.abs(q[i] * 1e-3));
    }
  }
  const g = new GeoBuilder();
  const paint = SURF.paint!;
  const glass = SURF.glass!;
  // Ring (left → right over the top) per station; glass band where the slice has glass.
  const rings: number[][] = [];
  const ringY = (k: number) => (k === 0 ? lo.y : k === N ? hi.y : lo.y + (len * k) / N);
  const sliceAt = (k: number) => sl[Math.min(N - 1, Math.max(0, k === N ? N - 1 : k === 0 ? 0 : k))];
  for (let k = 0; k <= N; k++) {
    const a = k === 0 || k === N ? sliceAt(k) : mixSlice(sl[k - 1], sl[k]);
    const y = ringY(k);
    const hasGlass = a.glass < a.zTop - 0.12;
    const zb = hasGlass ? a.glass : a.zTop - 0.03;
    const rw = hasGlass ? Math.min(a.rw, a.hw - 0.05) : a.hw - 0.04;
    const pts: [number, number, Surf][] = [
      [-a.hw, a.zLo, paint], [-a.hw, zb, hasGlass ? glass : paint], [-rw, a.zTop, paint],
      [rw, a.zTop, paint], [a.hw, zb, paint], [a.hw, a.zLo, paint],
    ];
    rings.push(pts.map(([x, z, s]) => g.vert(x, y, z, s, 1)));
  }
  for (let k = 0; k < N; k++) {
    const r0 = rings[k];
    const r1 = rings[k + 1];
    for (let j = 0; j < 5; j++) g.quad(r0[j], r0[j + 1], r1[j + 1], r1[j]);
    g.quad(r0[5], r0[0], r1[0], r1[5]); // underside
  }
  // End caps (fan).
  const capF = rings[N];
  const capR = rings[0];
  for (let j = 1; j < 5; j++) {
    g.idx.push(capF[0], capF[j], capF[j + 1]);
    g.idx.push(capR[0], capR[j + 1], capR[j]);
  }
  // Glass sides: recolour the band between belt and roof edge (separate verts, flat).
  for (let k = 0; k < N; k++) {
    const a = sl[k];
    if (!(a.glass < a.zTop - 0.12)) continue;
    const y0 = ringY(k) + 0.02;
    const y1 = ringY(k + 1) - 0.02;
    for (const sx of [-1, 1]) {
      const x0 = sx * (a.hw + 0.004);
      const x1 = sx * (Math.min(a.rw, a.hw - 0.05) + 0.004);
      const v = [g.vert(x0, y0, a.glass + 0.03, glass, 0), g.vert(x0, y1, a.glass + 0.03, glass, 0), g.vert(x1, y1, a.zTop - 0.04, glass, 0), g.vert(x1, y0, a.zTop - 0.04, glass, 0)];
      if (sx < 0) g.quad(v[0], v[3], v[2], v[1]);
      else g.quad(v[0], v[1], v[2], v[3]);
    }
  }
  // Lamps from the near mesh.
  for (const name of ["headlight_l", "headlight_r", "taillight_l", "taillight_r"]) {
    const b = bounds(mesh.parts.find((q) => q.name === name));
    if (!b) continue;
    const s = name.startsWith("head") ? SURF.headlight! : SURF.taillight!;
    const pad = 0.02;
    g.box([b.lo.x, b.lo.y - (name.startsWith("tail") ? pad : 0), b.lo.z], [b.hi.x, b.hi.y + (name.startsWith("head") ? pad : 0), b.hi.z], s, 0);
  }
  for (const w of def?.wheels ?? []) g.wheel(w.position_m, w.radius_m, w.width_m, 8, SURF.rubber!);
  return g.build();
}

function mixSlice(a: { zLo: number; zTop: number; hw: number; rw: number; glass: number }, b: typeof a): typeof a {
  return {
    zLo: Math.min(a.zLo, b.zLo),
    zTop: 0.5 * (a.zTop + b.zTop),
    hw: Math.max(a.hw, b.hw),
    rw: 0.5 * (a.rw + b.rw),
    glass: Math.max(a.glass, b.glass) === Infinity ? Infinity : 0.5 * (Math.min(a.glass, a.zTop) + Math.min(b.glass, b.zTop)),
  };
}

/** A plain car-shaped box for models whose mesh has not loaded (yet). */
function boxGeometry(): THREE.BufferGeometry {
  const g = new GeoBuilder();
  g.box([-0.9, -2.2, 0.2], [0.9, 2.2, 0.85], SURF.paint!);
  g.box([-0.75, -1.1, 0.85], [0.75, 0.9, 1.4], SURF.glass!, 0);
  g.box([-0.7, -1.0, 1.38], [0.7, 0.8, 1.42], SURF.paint!);
  for (const [x, y] of [[-0.8, 1.35], [0.8, 1.35], [-0.8, -1.35], [0.8, -1.35]]) g.wheel([x, y, 0.32], 0.32, 0.2, 8, SURF.rubber!);
  return g.build();
}

/** Prop geometries, in "unit" anchor space (scaled per model by the instance matrix). */
function propGeometry(kind: TrafficProp): THREE.BufferGeometry {
  const g = new GeoBuilder();
  const prop: Surf = { rgb: 0xffffff, rough: 0.45, metal: 0.1, lamp: 0, paint: true };
  const alu: Surf = { rgb: 0xb8bcc0, rough: 0.35, metal: 0.8, lamp: 0, paint: false };
  const black = SURF.trim!;
  const sign: Surf = { rgb: 0xf2e6b0, rough: 0.3, metal: 0, lamp: 5, paint: false };
  const amber: Surf = { rgb: 0xd88a18, rough: 0.3, metal: 0, lamp: 6, paint: false };
  switch (kind) {
    // Roof-anchored props: x in roof half widths, y in roof lengths (centre 0), z metres above the roof.
    case "taxi_sign":
      g.box([-0.08, -0.18, 0], [0.08, -0.12, 0.04], black, 0);
      g.box([-0.08, 0.12, 0], [0.08, 0.18, 0.04], black, 0);
      g.box([-0.55, -0.12, 0.03], [0.55, 0.12, 0.24], sign, 0);
      g.box([-0.57, -0.13, 0.22], [0.57, 0.13, 0.25], black, 0);
      break;
    case "roof_rails":
      for (const x of [-0.85, 0.85]) {
        g.box([x - 0.03, -0.42, 0.04], [x + 0.03, 0.42, 0.075], black, 0);
        for (const y of [-0.4, 0.4]) g.box([x - 0.03, y - 0.04, 0], [x + 0.03, y + 0.04, 0.06], black, 0);
      }
      break;
    case "roof_box":
      g.box([-0.62, -0.32, 0.075], [0.62, 0.36, 0.42], prop);
      g.box([-0.64, -0.33, 0.06], [0.64, 0.37, 0.08], black, 0);
      break;
    case "van_rack": {
      for (const x of [-0.92, 0.92]) g.box([x - 0.025, -0.45, 0.06], [x + 0.025, 0.45, 0.1], alu, 0);
      for (const y of [-0.42, 0, 0.42]) g.box([-0.95, y - 0.02, 0.04], [0.95, y + 0.02, 0.08], alu, 0);
      // Ladder.
      for (const x of [-0.3, -0.05]) g.box([x - 0.02, -0.46, 0.1], [x + 0.02, 0.4, 0.15], alu, 0);
      for (let y = -0.44; y < 0.4; y += 0.08) g.box([-0.3, y - 0.008, 0.11], [-0.05, y + 0.008, 0.14], alu, 0);
      break;
    }
    // Bed-anchored props: x in bed half widths, y in bed lengths (centre 0), z metres above the rail.
    case "pickup_rack":
      for (const y of [-0.45, 0.42]) {
        for (const x of [-1.02, 1.02]) g.box([x - 0.03, y - 0.03, -0.02], [x + 0.03, y + 0.03, 0.5], prop);
        g.box([-1.05, y - 0.03, 0.47], [1.05, y + 0.03, 0.53], prop);
      }
      for (const x of [-0.95, 0.95]) g.box([x - 0.03, -0.48, 0.5], [x + 0.03, 0.75, 0.55], prop);
      for (const x of [-0.45, -0.1]) g.box([x - 0.025, -0.5, 0.55], [x + 0.025, 0.95, 0.6], alu, 0);
      for (let y = -0.46; y < 0.95; y += 0.1) g.box([-0.45, y - 0.01, 0.56], [-0.1, y + 0.01, 0.595], alu, 0);
      break;
    case "toolbox":
      g.box([-1.04, 0.38, -0.02], [1.04, 0.47, 0.22], { ...alu, rough: 0.25, metal: 0.9 }, 0);
      g.box([-1.05, 0.37, 0.2], [1.05, 0.48, 0.23], alu, 0);
      break;
    case "beacon":
      g.box([-0.07, -0.05, 0], [0.07, 0.05, 0.03], black, 0);
      g.box([-0.05, -0.04, 0.03], [0.05, 0.04, 0.12], amber, 0);
      break;
    case "tonneau":
      g.box([-1.0, -0.5, -0.01], [1.0, 0.5, 0.025], black, 0);
      break;
  }
  return g.build();
}
const ROOF_PROPS = new Set<TrafficProp>(["taxi_sign", "roof_rails", "roof_box", "van_rack", "beacon"]);

// ------------------------------------------------------------------------------------------
// Material
// ------------------------------------------------------------------------------------------

/** Shared shader uniforms (one object for every traffic material). */
const shared = {
  uNight: { value: 0 },
  uBlink: { value: 0 },
  uWords: { value: null as THREE.Texture | null },
};

function wordAtlas(): THREE.CanvasTexture {
  const c = document.createElement("canvas");
  c.width = 1024;
  c.height = 512;
  const g = c.getContext("2d")!;
  g.clearRect(0, 0, c.width, c.height);
  g.fillStyle = "#fff";
  g.textAlign = "center";
  g.textBaseline = "middle";
  WORDS.forEach((w, i) => {
    let size = 50;
    g.font = `bold ${size}px sans-serif`;
    while (g.measureText(w).width > 980 && size > 20) g.font = `bold ${(size -= 2)}px sans-serif`;
    g.fillText(w, 512, i * 64 + 34);
  });
  const t = new THREE.CanvasTexture(c);
  t.anisotropy = 4;
  return t;
}

/** Traffic material: standard PBR with the attribute layout documented at the top. */
function trafficMaterial(a: Anchors | null): THREE.MeshStandardMaterial {
  const m = new THREE.MeshStandardMaterial({ vertexColors: true, flatShading: true, side: THREE.DoubleSide, roughness: 0.5, metalness: 0 });
  const local = {
    uCheckZ: { value: a?.checkZ ?? -10 },
    uBandZ: { value: a?.bandZ ?? -10 },
    uText: { value: new THREE.Vector4(...(a?.text ?? [0, 0, -10, -10])) },
    uHeight: { value: a ? a.hi.z : 1.5 },
  };
  m.onBeforeCompile = (sh) => {
    Object.assign(sh.uniforms, shared, local);
    sh.vertexShader = sh.vertexShader
      .replace(
        "#include <common>",
        `#include <common>
attribute vec4 aSurf;
attribute vec4 iAlt;
attribute vec4 iParams;
attribute vec2 iPbr;
varying vec4 vSurf;
varying vec4 vAlt;
varying vec4 vParams;
varying vec2 vPbr;
varying vec3 vLocal;
varying vec3 vPaint;`,
      )
      .replace(
        "#include <color_vertex>",
        `#include <color_vertex>
vSurf = aSurf;
vAlt = iAlt;
vParams = iParams;
vPbr = iPbr;
vLocal = position;
#ifdef USE_INSTANCING_COLOR
vPaint = instanceColor.rgb;
#else
vPaint = vec3(1.0);
#endif
vColor = color;`,
      );
    sh.fragmentShader = sh.fragmentShader
      .replace(
        "#include <common>",
        `#include <common>
uniform float uNight;
uniform float uBlink;
uniform float uCheckZ;
uniform float uBandZ;
uniform vec4 uText;
uniform float uHeight;
uniform sampler2D uWords;
varying vec4 vSurf;
varying vec4 vAlt;
varying vec4 vParams;
varying vec2 vPbr;
varying vec3 vLocal;
varying vec3 vPaint;
float tHash(vec3 p) { return fract(sin(dot(floor(p), vec3(12.9898, 78.233, 37.719))) * 43758.5453); }
float tNoise(vec3 p) {
  vec3 i = floor(p); vec3 f = fract(p); f = f * f * (3.0 - 2.0 * f);
  float a = mix(mix(tHash(i), tHash(i + vec3(1,0,0)), f.x), mix(tHash(i + vec3(0,1,0)), tHash(i + vec3(1,1,0)), f.x), f.y);
  float b = mix(mix(tHash(i + vec3(0,0,1)), tHash(i + vec3(1,0,1)), f.x), mix(tHash(i + vec3(0,1,1)), tHash(i + vec3(1,1,1)), f.x), f.y);
  return mix(a, b, f.z);
}`,
      )
      .replace(
        "#include <color_fragment>",
        `#include <color_fragment>
float tGroup = floor(vSurf.x + 0.5);
float tRough = vSurf.z;
float tMetal = vSurf.w;
vec3 tLn = normalize(cross(dFdx(vLocal), dFdy(vLocal)));
if (tGroup > 0.5) {
  int mask = int(vAlt.w + 0.5) & 63;
  int livery = int(vAlt.w + 0.5) >> 6;
  bool alt = tGroup > 1.5 && ((mask >> int(tGroup)) & 1) == 1;
  vec3 paint = alt ? vAlt.rgb : vPaint;
  tRough = alt ? 0.85 : vPbr.x;
  tMetal = alt ? 0.0 : vPbr.y;
  bool side = abs(tLn.x) > 0.55;
  if (livery == 1 && side && abs(vLocal.z - uCheckZ) < 0.05) {
    float k = mod(floor(vLocal.y / 0.1) + floor((vLocal.z - uCheckZ + 0.05) / 0.05), 2.0);
    paint = mix(vec3(0.02), vec3(0.85), k);
    tMetal = 0.0;
  }
  if (livery == 2 && side && vLocal.z < uBandZ && vLocal.y < uText.y + 0.1) { paint = vAlt.rgb; tMetal = 0.0; }
  if ((livery == 2 || livery == 3) && side && vParams.w >= 0.0 && vLocal.y > uText.x && vLocal.y < uText.y && vLocal.z > uText.z && vLocal.z < uText.w) {
    float u = (vLocal.y - uText.x) / (uText.y - uText.x);
    if (vLocal.x < 0.0) u = 1.0 - u;
    float t = (vLocal.z - uText.z) / (uText.w - uText.z);
    float a = texture2D(uWords, vec2(u, 1.0 - (vParams.w + 1.0) / 8.0 + t / 8.0)).a;
    paint = mix(paint, livery == 2 ? vec3(0.03, 0.04, 0.08) : vAlt.rgb, a);
  }
  // Rust: blotches low on the body and round the arches, scaled by vParams.x.
  if (vParams.x > 0.0) {
    float low = 1.0 - smoothstep(0.2, 0.45, vLocal.z / uHeight);
    float n = tNoise(vLocal * 7.0) * 0.7 + tNoise(vLocal * 19.0) * 0.3;
    float r = smoothstep(0.55, 0.65, n * (0.55 + 0.65 * low) * (0.6 + 0.5 * vParams.x));
    paint = mix(paint, vec3(0.30, 0.13, 0.06) * (0.7 + 0.5 * tNoise(vLocal * 40.0)), r);
    tRough = mix(tRough, 0.95, r);
    tMetal = mix(tMetal, 0.0, r);
  }
  diffuseColor.rgb *= paint;
}
// Dirt / grime toward the sills.
float tDirt = vParams.y * (1.0 - smoothstep(0.05, 0.55, vLocal.z / uHeight));
diffuseColor.rgb *= 1.0 - 0.45 * tDirt;
tRough = mix(tRough, 0.9, tDirt);`,
      )
      .replace(
        "#include <roughnessmap_fragment>",
        `#include <roughnessmap_fragment>
roughnessFactor = tRough;`,
      )
      .replace(
        "#include <metalnessmap_fragment>",
        `#include <metalnessmap_fragment>
metalnessFactor = tMetal;`,
      )
      .replace(
        "#include <emissivemap_fragment>",
        `#include <emissivemap_fragment>
{
  int lamp = int(vSurf.y + 0.5);
  int bits = int(vParams.z + 0.5);
  bool hazard = (bits & 2) != 0;
  if (lamp == 1) totalEmissiveRadiance += vec3(1.0, 0.95, 0.82) * 2.2 * uNight;
  else if (lamp == 2) totalEmissiveRadiance += vec3(1.0, 0.05, 0.03) * (0.6 * uNight + ((bits & 1) != 0 ? 2.0 : 0.0));
  else if (lamp == 3 || lamp == 4) {
    bool on = hazard || (lamp == 3 ? (bits & 4) != 0 : (bits & 8) != 0);
    totalEmissiveRadiance += vec3(1.0, 0.5, 0.05) * (on ? 2.5 * uBlink : 0.0);
  } else if (lamp == 5) totalEmissiveRadiance += vec3(1.0, 0.85, 0.45) * (0.25 + 1.4 * uNight);
  else if (lamp == 6) totalEmissiveRadiance += vec3(1.0, 0.5, 0.05) * (hazard ? 3.0 * uBlink : 0.0);
}`,
      );
  };
  // Distinct program per anchors set (uniform values differ, code is shared).
  m.customProgramCacheKey = () => "traffic";
  return m;
}

// ------------------------------------------------------------------------------------------
// Buckets
// ------------------------------------------------------------------------------------------

/** One InstancedMesh with the traffic per-instance attributes. */
class Bucket {
  readonly mesh: THREE.InstancedMesh;
  readonly alt: THREE.InstancedBufferAttribute;
  readonly params: THREE.InstancedBufferAttribute;
  readonly pbr: THREE.InstancedBufferAttribute;
  readonly color: THREE.InstancedBufferAttribute;
  count = 0;

  constructor(geometry: THREE.BufferGeometry, material: THREE.Material, name: string) {
    this.mesh = new THREE.InstancedMesh(geometry, material, MAX_INSTANCES);
    this.mesh.name = name;
    this.mesh.frustumCulled = false;
    this.mesh.count = 0;
    this.color = new THREE.InstancedBufferAttribute(new Float32Array(MAX_INSTANCES * 3), 3);
    this.mesh.instanceColor = this.color;
    this.alt = new THREE.InstancedBufferAttribute(new Float32Array(MAX_INSTANCES * 4), 4);
    this.params = new THREE.InstancedBufferAttribute(new Float32Array(MAX_INSTANCES * 4), 4);
    this.pbr = new THREE.InstancedBufferAttribute(new Float32Array(MAX_INSTANCES * 2), 2);
    for (const a of [this.mesh.instanceMatrix, this.color, this.alt, this.params, this.pbr]) a.setUsage(THREE.DynamicDrawUsage);
    geometry.setAttribute("iAlt", this.alt);
    geometry.setAttribute("iParams", this.params);
    geometry.setAttribute("iPbr", this.pbr);
  }

  /** Append one instance: yaw-only rotation + translation (+ per-axis scale and local offset). */
  push(look: Resolved, x: number, y: number, z: number, c: number, s: number, lamps: number, off?: PropXform): void {
    if (this.count >= MAX_INSTANCES) return;
    const i = this.count++;
    const m = this.mesh.instanceMatrix.array as Float32Array;
    const o = i * 16;
    const sx = off ? off.sx : 1;
    const sy = off ? off.sy : 1;
    const sz = off ? off.sz : 1;
    const tx = off ? off.tx : 0;
    const ty = off ? off.ty : 0;
    const tz = off ? off.tz : 0;
    m[o] = c * sx; m[o + 1] = s * sx; m[o + 2] = 0; m[o + 3] = 0;
    m[o + 4] = -s * sy; m[o + 5] = c * sy; m[o + 6] = 0; m[o + 7] = 0;
    m[o + 8] = 0; m[o + 9] = 0; m[o + 10] = sz; m[o + 11] = 0;
    m[o + 12] = x + c * tx - s * ty; m[o + 13] = y + s * tx + c * ty; m[o + 14] = z + tz; m[o + 15] = 1;
    const col = this.color.array as Float32Array;
    const rgb = off ? look.propRgb : look.paintRgb;
    col[i * 3] = rgb[0]; col[i * 3 + 1] = rgb[1]; col[i * 3 + 2] = rgb[2];
    const al = this.alt.array as Float32Array;
    al[i * 4] = look.altRgb[0]; al[i * 4 + 1] = look.altRgb[1]; al[i * 4 + 2] = look.altRgb[2]; al[i * 4 + 3] = look.altW;
    const pa = this.params.array as Float32Array;
    pa[i * 4] = off ? 0 : look.look.rust; pa[i * 4 + 1] = look.look.dirt; pa[i * 4 + 2] = lamps; pa[i * 4 + 3] = look.look.textRow;
    const pb = this.pbr.array as Float32Array;
    pb[i * 2] = off ? 0.45 : look.look.roughness; pb[i * 2 + 1] = off ? 0.1 : look.look.metalness;
  }

  /** Publish this frame's instances. */
  commit(): void {
    this.mesh.count = this.count;
    if (this.count === 0) return;
    for (const a of [this.mesh.instanceMatrix, this.color, this.alt, this.params, this.pbr]) {
      a.clearUpdateRanges();
      a.addUpdateRange(0, this.count * a.itemSize);
      a.needsUpdate = true;
    }
  }
}

/** Prop placement relative to the car: translation + scale. */
interface PropXform {
  tx: number; ty: number; tz: number;
  sx: number; sy: number; sz: number;
}

/** A look plus the values the buckets write (linear colours). */
interface Resolved {
  look: TrafficLook;
  model: string;
  paintRgb: [number, number, number];
  altRgb: [number, number, number];
  propRgb: [number, number, number];
  altW: number;
}

function linear(hex: number, jitter = 0): [number, number, number] {
  const c = new THREE.Color().setHex(hex, THREE.SRGBColorSpace);
  if (jitter) c.offsetHSL(0, 0, jitter);
  return [c.r, c.g, c.b];
}

interface Model {
  near: Bucket;
  far: Bucket;
  anchors: Anchors;
}

// ------------------------------------------------------------------------------------------
// Renderer
// ------------------------------------------------------------------------------------------

/** Draws civilian traffic as instanced, styled car meshes. */
export class TrafficRenderer {
  /** Parent this under the sim-space `world` group. */
  readonly group = new THREE.Group();
  /** True once at least the default traffic models have loaded (scene.ts keeps its box path until then). */
  ready = false;
  /** Last frame's CPU cost of update(), ms (perf / tests). */
  lastMs = 0;
  /** Instances drawn last frame: near, far, props. */
  readonly drawn = { near: 0, far: 0, props: 0 };
  private readonly models = new Map<string, Model>();
  private readonly loading = new Set<string>();
  private readonly box: Model;
  private readonly props = new Map<TrafficProp, Bucket>();
  private readonly looks = new Map<number, Resolved>();
  private simModels: string[] = [];
  private modelsCheckedAt = -Infinity;
  /** Fallback tracking (builds without agent ids): previous positions and seeds by index. */
  private prevPos = new Float32Array(0);
  private prevSeed = new Uint32Array(0);
  private nextSeed = 1;
  private readonly cam = new THREE.Vector3();
  private readonly camDir = new THREE.Vector3(0, 1, 0);

  constructor() {
    shared.uWords.value = wordAtlas();
    const g = boxGeometry();
    const mat = trafficMaterial(null);
    this.box = { near: new Bucket(g, mat, "traffic_box"), far: new Bucket(g, mat, "traffic_box_far"), anchors: null as unknown as Anchors };
    this.group.add(this.box.near.mesh);
    const propMat = trafficMaterial(null);
    for (const k of ["taxi_sign", "roof_rails", "roof_box", "pickup_rack", "toolbox", "beacon", "tonneau", "van_rack"] as TrafficProp[]) {
      const b = new Bucket(propGeometry(k), propMat, `traffic_prop_${k}`);
      this.props.set(k, b);
      this.group.add(b.mesh);
    }
    void Promise.all(TRAFFIC_CARS.map((id) => this.load(id))).then(() => {
      this.ready = this.models.size > 0;
    });
  }

  /** Load (once) and add the buckets of model `carId`. */
  private async load(carId: string): Promise<void> {
    if (this.models.has(carId) || this.loading.has(carId)) return;
    this.loading.add(carId);
    try {
      const [mesh, def] = await Promise.all([
        loadCarMesh(carId),
        fetch(`data/cars/${carId}.json`).then((r) => (r.ok ? (r.json() as Promise<CarDefJson>) : null)).catch(() => null),
      ]);
      if (!mesh) return;
      const { near, far, anchors } = buildModel(mesh, def);
      const mat = trafficMaterial(anchors);
      const model = { near: new Bucket(near, mat, `traffic_${carId}`), far: new Bucket(far, mat, `traffic_${carId}_far`), anchors };
      this.group.add(model.near.mesh, model.far.mesh);
      this.models.set(carId, model);
    } catch (e) {
      console.warn("traffic model failed to load", carId, e);
    }
  }

  /** Camera position + forward in sim space (from the Three camera; one frame late is fine). */
  setCamera(camera: THREE.Camera): void {
    // Three is +Y up; sim is +Z up: sim (x, y, z) = three (x, -z, y).
    const p = camera.getWorldPosition(new THREE.Vector3());
    this.cam.set(p.x, -p.z, p.y);
    const d = camera.getWorldDirection(new THREE.Vector3());
    this.camDir.set(d.x, -d.z, d.y);
  }

  /** Night factor 0 (noon) … 1 (midnight): lamps glow. */
  setNight(night: number): void {
    shared.uNight.value = Math.min(Math.max((night - 0.3) / 0.5, 0), 1);
  }

  /** Car ids by model index from wasm `traffic_models()` (absent on older builds). */
  private refreshSimModels(now: number): void {
    if (now - this.modelsCheckedAt < 3000) return;
    this.modelsCheckedAt = now;
    const fn = (wasm as unknown as { traffic_models?: () => string }).traffic_models;
    if (typeof fn !== "function") return;
    try {
      const ids = JSON.parse(fn()) as string[];
      if (ids.join() !== this.simModels.join()) {
        this.simModels = ids;
        this.looks.clear();
        for (const id of ids) void this.load(id);
      }
    } catch {
      /* no session yet */
    }
  }

  /** Look for an agent (cached per stable key). */
  private resolve(key: number, id: number, model: number, variant: number): Resolved {
    let r = this.looks.get(key);
    if (r) return r;
    let carId: string;
    // With several physical models the sim's pick wins (a promoted car keeps its body). With one
    // (or none) every agent would look the same, so pick a body here; a promoted agent then
    // changes body (not colour) when it becomes the session's single physical model.
    if (this.simModels.length > 1 && model >= 0) carId = this.simModels[Math.min(model, this.simModels.length - 1)];
    else carId = pickModel(id, TRAFFIC_CARS);
    const look = trafficLook(carId, variant, id);
    // Per-instance jitter: value ±4 % (research §5 rule 6).
    const j = ((hash32(id ^ 0xabcdef) & 255) / 255 - 0.5) * 0.04;
    r = {
      look,
      model: carId,
      paintRgb: linear(look.paint, j),
      altRgb: linear(look.alt),
      propRgb: linear(look.propColor),
      altW: look.altMask + 64 * look.livery,
    };
    if (this.looks.size > 8192) this.looks.clear();
    this.looks.set(key, r);
    return r;
  }

  /**
   * Draw `agents` (traffic_agents(): `stride` floats each, [x, y, z, yaw, flags(, id, model)]).
   * Physical agents are skipped when `skipPhysical` (vehicle_view draws them). `timeMs` drives
   * the indicator blink.
   */
  update(agents: Float32Array, stride: number, skipPhysical: boolean, timeMs: number): void {
    const t0 = performance.now();
    this.refreshSimModels(timeMs);
    shared.uBlink.value = (timeMs % 700) < 380 ? 1 : 0;
    const hasId = stride >= 7;
    const total = Math.floor(agents.length / stride);
    for (const m of this.models.values()) {
      m.near.count = 0;
      m.far.count = 0;
    }
    this.box.near.count = 0;
    for (const b of this.props.values()) b.count = 0;
    if (!hasId) this.track(agents, stride, total);
    const cx = this.cam.x;
    const cy = this.cam.y;
    const cz = this.cam.z;
    const dx = this.camDir.x;
    const dy = this.camDir.y;
    const dz = this.camDir.z;
    let near = 0;
    let far = 0;
    let props = 0;
    for (let a = 0; a < total; a++) {
      const o = a * stride;
      const flags = agents[o + 4];
      if (skipPhysical && flags & AGENT_FLAG.physical) continue;
      const x = agents[o];
      const y = agents[o + 1];
      const z = agents[o + 2];
      const ex = x - cx;
      const ey = y - cy;
      const ez = z - cz;
      const d2 = ex * ex + ey * ey + ez * ez;
      if (d2 > DRAW_FAR_M * DRAW_FAR_M) continue;
      // Behind the camera (with a margin for the car's own size).
      if (d2 > 100 && ex * dx + ey * dy + ez * dz < -8) continue;
      const variant = (flags >> 8) & 0xff;
      const id = hasId ? agents[o + 5] : this.prevSeed[a];
      const look = this.resolve(hasId ? id : -1 - id, id, hasId ? agents[o + 6] : -1, variant);
      const yaw = agents[o + 3];
      const c = Math.cos(yaw);
      const s = Math.sin(yaw);
      const lamps = (flags & AGENT_FLAG.brake ? 1 : 0) | (flags & AGENT_FLAG.hazard ? 2 : 0) |
        (flags & AGENT_FLAG.turnLeft ? 4 : 0) | (flags & AGENT_FLAG.turnRight ? 8 : 0);
      const model = this.models.get(look.model);
      if (!model) {
        if (!this.loading.has(look.model)) void this.load(look.model);
        this.box.near.push(look, x, y, z, c, s, lamps);
        near++;
        continue;
      }
      const isNear = d2 < LOD_NEAR_M * LOD_NEAR_M;
      (isNear ? model.near : model.far).push(look, x, y, z, c, s, lamps);
      if (isNear) near++;
      else far++;
      if (look.look.props.length && d2 < PROP_FAR_M * PROP_FAR_M) {
        for (const k of look.look.props) {
          const xf = propXform(k, model.anchors);
          if (!xf) continue;
          this.props.get(k)!.push(look, x, y, z, c, s, lamps, xf);
          props++;
        }
      }
    }
    for (const m of this.models.values()) {
      m.near.commit();
      m.far.commit();
    }
    this.box.near.commit();
    for (const b of this.props.values()) b.commit();
    this.drawn.near = near;
    this.drawn.far = far;
    this.drawn.props = props;
    this.lastMs = performance.now() - t0;
  }

  /**
   * Builds without agent ids: keep a per-agent seed by matching each agent to the nearest
   * previous agent (same index first, then neighbours, then a full scan); new agents get a
   * fresh seed. Agents move ≤ ~1 m per frame.
   */
  private track(agents: Float32Array, stride: number, total: number): void {
    const prevN = this.prevPos.length / 2;
    const pos = new Float32Array(total * 2);
    const seed = new Uint32Array(total);
    const near = (a: number, b: number) => {
      const ex = agents[a * stride] - this.prevPos[b * 2];
      const ey = agents[a * stride + 1] - this.prevPos[b * 2 + 1];
      return ex * ex + ey * ey < 9;
    };
    for (let a = 0; a < total; a++) {
      pos[a * 2] = agents[a * stride];
      pos[a * 2 + 1] = agents[a * stride + 1];
      let found = -1;
      for (const b of [a, a - 1, a + 1, a - 2, a + 2]) {
        if (b >= 0 && b < prevN && near(a, b)) {
          found = b;
          break;
        }
      }
      if (found < 0) for (let b = 0; b < prevN; b++) if (near(a, b)) { found = b; break; }
      seed[a] = found >= 0 ? this.prevSeed[found] : hash32(this.nextSeed++ * 7919 + Math.round(pos[a * 2]) * 31 + Math.round(pos[a * 2 + 1]));
    }
    this.prevPos = pos;
    this.prevSeed = seed;
  }
}

/** Where prop `k` sits on a model (null if the model has no anchor for it). */
function propXform(k: TrafficProp, a: Anchors): PropXform | null {
  if (ROOF_PROPS.has(k)) {
    const r = a.roof;
    if (k === "beacon") {
      // Work pickups: beacon on the cab roof, slightly back of centre.
      return { tx: -0.25 * r.hw, ty: r.y - 0.15 * r.len, tz: r.z, sx: 1, sy: 1, sz: 1 };
    }
    if (k === "taxi_sign") return { tx: 0, ty: r.y - 0.05 * r.len, tz: r.z - 0.01, sx: 1, sy: 1, sz: 1 };
    // Unit anchor: x in half widths, y in roof lengths.
    return { tx: 0, ty: r.y, tz: r.z - 0.01, sx: r.hw, sy: r.len, sz: 1 };
  }
  const b = a.bed;
  if (!b) return null;
  return { tx: 0, ty: b.y, tz: b.z, sx: b.hw, sy: b.len, sz: 1 };
}
