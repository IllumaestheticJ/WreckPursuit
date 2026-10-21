// Instanced cars for everything that is not drawn by a VehicleView (lead helper, playtest row 25
// "LOD pop-in"): kinematic traffic agents, police units without a sim vehicle, and sim vehicles
// whose own view is not ready yet. One car model = a few InstancedMeshes, so 400 agents cost a
// handful of draw calls.
//
// Near cars (< FULL_LOD_M from the camera, the "full" instance set) use the model's mid level
// of detail (data/panels/meshes/mid/<car>.json, tools/assets `Lod::Mid`: the same style, livery
// atlas UVs and tints as the full panel mesh a promoted car's VehicleView shows, ~3–6k
// triangles instead of 8–20k, merged and undeformed; the full mesh if the mid file is missing),
// with the same livery colour as the car will have once promoted, so the swap to a full
// VehicleView (render/lod.ts `viewDrawM`, ~30–40 m) does not pop. Far cars, and cars whose
// mesh has not arrived yet, use a low-poly proxy of the model's body size. Coordinates: sim
// space (+Z up), parented under SceneView.world.
//
// Every model in the session has its own instance sets (render/lod.ts picks the model from the
// draw record's `model` index), so mixed traffic and a mixed police fleet cost a few draw calls
// per model. Police models with a livery scheme (render/livery.ts) draw their body panels with
// the scheme's shared texture and their light-bar lenses as instanced lens meshes flashed per
// unit (render/lights.ts patterns); per-unit numbers only appear once the car is promoted.
import * as THREE from "three";
import type { CarMeshJson, MaterialJson, PartMaterial } from "./panels";
import { applyCarEnv, carMidMeshNow, indicatorRight, isIndicatorPart, loadCarMidMesh, rgbColour } from "./panels";
import { hasPoliceLivery, liveryBaseColour, liveryTexture } from "./livery";
import { LENS_ON } from "./lights";
import { sim, type CarModelInfo } from "../wasm/sim";
import { PALETTE, type Hex } from "./palette";
import { artStyle } from "./style";

/** Cars nearer than this (to the camera) get the mid-LOD instanced mesh, m (default; the
 * graphics profiles set `Fleet.fullLodM`, render/quality.ts). */
export const FULL_LOD_M = 120;
/** Full-mesh instances per model per frame (the nearest get them; the rest use the proxy). */
const MAX_FULL = 192;
/** Proxy instances per model per frame. */
const MAX_PROXY = 1024;

/** Civilian livery per traffic `variant` (0..7): shared by instanced agents and promoted cars. */
export const TRAFFIC_PALETTE = [0x8a9099, 0x2b3a55, 0x7a1f1f, 0xd8d8d2, 0x1e1e22, 0x3f5a3a, 0xb39a5c, 0x4a6f8a];
/** Police livery (same as the police VehicleView). */
export const POLICE_PAINT = 0xe9ebf0;
/** Tail lamp colours (unlit). */
export const TAIL = { off: 0x4a0808, brake: 0xff2a1a, hazard: 0xff9a10 } as const;
/** Head lamp colour (unlit). */
export const HEAD = 0xfff2c0;
/** Light bar colours (unlit). */
export const BAR = { off: 0x1a1a24, red: 0xff2040, blue: 0x2060ff } as const;

/** Indicator lamp bits (`ind` masks below): left-side lamps, right-side lamps. Hazards = both. */
export const IND = { left: 1, right: 2 } as const;
/** Indicator blink rate, Hz (50 % duty; every car blinks in phase). */
export const INDICATOR_HZ = 1.5;
/** Whether indicators are in the lit half of their blink at time `t` (s). */
export const indicatorPhaseOn = (t: number): boolean => (t * INDICATOR_HZ) % 1 < 0.5;
/** Unlit indicator lens brightness over its hue (a dark lens, well below the bloom pass). */
const IND_UNLIT = 0.12;
/** Lit indicator glow over the hue (greybox; low-poly dark uses `PALETTE.brakeGlow`): above 1
 * so it reaches the bloom pass and reads in daylight. */
const IND_GLOW = 1.0;
/** Turn-signal amber: deeper than the hazard-flash palette amber, so a lit lens reads as amber
 * rather than tone-mapping to cream (playtest row 136). */
const IND_AMBER: Hex = 0xff6400;

/** Indicator lens hue: amber, or red for red-on-red rear lamps (mesh parts in `taillight`;
 * the saturated brake red, which stays red rather than pink once it glows over 1). */
export function indicatorHue(red: boolean): Hex {
  return red ? TAIL.brake : IND_AMBER;
}

/** Indicator brightness over its hue: lit (HDR, blooms) or the dark lens. The fleet's indicator
 * geometry carries the hue as vertex colour and this as its instance colour (grey). */
export function indicatorLevel(lit: boolean): number {
  return lit ? (artStyle.lowpoly ? PALETTE.brakeGlow : IND_GLOW) : IND_UNLIT;
}

/** Indicator lamp colour into `out`: lit (HDR, blooms) or the dark lens; amber unless `red`. */
export function indicatorColour(out: THREE.Color, lit: boolean, red = false): THREE.Color {
  return out.setHex(indicatorHue(red)).multiplyScalar(indicatorLevel(lit));
}

/** Which corner lamps (front / rear) a mesh has as indicator lens parts (side repeaters do not
 * count); the box markers stand in for the rest. Once per mesh. */
export function meshIndicatorEnds(mesh: CarMeshJson | null | undefined): { front: boolean; rear: boolean } {
  if (!mesh) return NO_IND_ENDS;
  let hit = indEndsCache.get(mesh);
  if (!hit) {
    hit = { front: false, rear: false };
    for (const p of mesh.parts) {
      if (!isIndicatorPart(p.name)) continue;
      if (p.name.startsWith("indicator_front")) hit.front = true;
      else if (p.name.startsWith("indicator_rear")) hit.rear = true;
    }
    indEndsCache.set(mesh, hit);
  }
  return hit;
}
const NO_IND_ENDS = { front: false, rear: false } as const;
const indEndsCache = new WeakMap<CarMeshJson, { front: boolean; rear: boolean }>();


/**
 * Livery in the current art style. Low-poly dark maps the greybox traffic liveries one to one
 * onto `PALETTE.trafficPaints` (same variant → same slot) and police white onto
 * `PALETTE.policePaint`, so the fleet, promoted VehicleViews and parked props agree.
 * Any other colour passes through.
 */
export function styledPaint(hex: Hex, police = false): Hex {
  if (!artStyle.lowpoly) return hex;
  if (police || hex === POLICE_PAINT) return PALETTE.policePaint;
  const i = TRAFFIC_PALETTE.indexOf(hex);
  return i < 0 ? hex : PALETTE.trafficPaints[i % PALETTE.trafficPaints.length];
}

/**
 * Lamp colour in the current art style, into `out` (linear, may exceed 1 so lit lamps reach
 * the bloom pass). Takes the greybox TAIL / BAR / HEAD hexes; other colours pass through.
 */
export function styledLamp(out: THREE.Color, hex: Hex): THREE.Color {
  out.setHex(hex);
  if (!artStyle.lowpoly) return out;
  const night = artStyle.night;
  switch (hex) {
    case TAIL.off:
      // Unlit lens by day; running lights (a quarter of the brake glow) at night.
      out.setHex(night > 0.2 ? PALETTE.taillight : UNLIT_TAIL);
      return night > 0.2 ? out.multiplyScalar(PALETTE.brakeGlow * 0.25) : out;
    case TAIL.brake:
      return out.setHex(PALETTE.taillight).multiplyScalar(PALETTE.brakeGlow);
    case TAIL.hazard:
      return out.setHex(PALETTE.hazard).multiplyScalar(PALETTE.brakeGlow);
    case HEAD:
      return out.setHex(PALETTE.headlight).multiplyScalar(night > 0.2 ? PALETTE.headlightGlow : 1);
    case BAR.off:
      return out.setHex(PALETTE.lightbarBase);
    case BAR.red:
      return out.setHex(PALETTE.policeRed).multiplyScalar(PALETTE.policeRedGlow);
    case BAR.blue:
      return out.setHex(PALETTE.policeBlue).multiplyScalar(PALETTE.policeBlueGlow);
  }
  return out;
}

/** How a car was drawn this frame. */
export type DrawMode = "full" | "proxy";

export interface Geoms {
  /** Painted surfaces (white vertex colour; instance colour = livery). */
  paint: THREE.BufferGeometry;
  /** Body panels under a police livery texture (uv; no instance colour), null without one. */
  liv?: THREE.BufferGeometry | null;
  /** Light-bar lenses: left / right side (red and blue lens parts, vertex colour = lit hue),
   * clear takedowns, amber advisors; instance colour = brightness. Null when the mesh has none. */
  lensL?: THREE.BufferGeometry | null;
  lensR?: THREE.BufferGeometry | null;
  lensW?: THREE.BufferGeometry | null;
  lensA?: THREE.BufferGeometry | null;
  /** Glass, trim, rubber, metal, head lamps (own vertex colours). */
  detail: THREE.BufferGeometry;
  /** Tail lamps (instance colour = lamp state). */
  tail: THREE.BufferGeometry;
  /** Head lamps (instance colour). */
  head: THREE.BufferGeometry;
  /** Left / right indicator lamps: the mesh's indicator lens parts of that side (front, rear,
   * side repeaters; vertex colour = hue), box markers on the corners the mesh has none for;
   * instance colour = brightness (`indicatorLevel`). */
  indL: THREE.BufferGeometry;
  indR: THREE.BufferGeometry;
}

/** Base colour per mesh material (glass is drawn opaque at LOD distances); render/panels.ts
 * greybox colours. */
const MATERIAL_RGB: Record<string, number> = {
  glass: 0x5c7480,
  trim: 0x1b1b1d,
  rubber: 0x141414,
  metal: 0x9da3a8,
  headlight: 0xf2ecd2,
  interior: 0x2b2724,
  chrome: 0xd8dde2,
  carbon: 0x1c1d20,
  caliper: 0x3a3c40,
  engine: 0x3b3d40,
  grille: 0x0d0e10,
  exhaust: 0xb9b2a6,
  rim: 0x9da3a8,
  spring: 0x3a3c40,
  accent: 0x8a1020,
  underglow: 0x101012,
  emissive_amber: 0xc87a10,
  lightbar_red: 0x661018,
  lightbar_blue: 0x102266,
  lightbar_clear: 0xbfc4c8,
};
/** The same in low-poly dark (render/palette.ts; as R's panel meshes are recoloured). */
const MATERIAL_RGB_LOWPOLY: Record<string, number> = {
  ...MATERIAL_RGB,
  glass: PALETTE.glass,
  trim: PALETTE.trim,
  rubber: PALETTE.tire,
  metal: PALETTE.rim,
  headlight: PALETTE.headlight,
  interior: PALETTE.interior,
};
/** Mesh-file slots `tints` recolour in the panel meshes too (render/panels.ts TINTABLE). */
const TINTED: ReadonlySet<string> = new Set(["caliper", "rim", "spring", "accent", "engine", "grille", "exhaust", "chrome", "carbon", "metal", "trim", "rubber", "glass", "interior"]);
/** Detail colour of a mesh material in the current art style (read when geometry is built);
 * the mesh's `tints` win in greybox, as on the panel meshes. Linear RGB into `out`. */
function detailColour(material: PartMaterial, mesh: CarMeshJson | null, out: THREE.Color): THREE.Color {
  if (artStyle.lowpoly && LOWPOLY_MAPPED.has(material)) return out.setHex(MATERIAL_RGB_LOWPOLY[material]);
  const tint = TINTED.has(material) ? mesh?.tints?.[material as MaterialJson] : undefined;
  if (tint) return rgbColour(tint, out);
  return out.setHex(MATERIAL_RGB[material] ?? 0x808080);
}
/** Materials low-poly dark takes from its palette (render/panels.ts PanelBody.applyStyle). */
const LOWPOLY_MAPPED: ReadonlySet<string> = new Set(["glass", "trim", "rubber", "metal", "headlight", "interior"]);
/** Detail colour as a hex (proxy geometry, no mesh). */
function detailRgb(material: string): number {
  return (artStyle.lowpoly ? MATERIAL_RGB_LOWPOLY : MATERIAL_RGB)[material] ?? 0x808080;
}

/** One part's triangles for `merge`: positions, index, flat colour (hex or linear Color), UVs. */
interface Chunk {
  pos: Float32Array;
  idx: ArrayLike<number>;
  rgb: number | THREE.Color;
  uv?: ArrayLike<number>;
}

/** Merge parts into one non-indexed geometry with a flat vertex colour per part (and UVs when
 * `withUv`: 0..1 from the mesh file's 0..65535). */
function merge(chunks: Chunk[], withUv = false): THREE.BufferGeometry {
  let n = 0;
  for (const c of chunks) n += c.idx.length;
  const pos = new Float32Array(n * 3);
  const col = new Float32Array(n * 3);
  const uv = withUv ? new Float32Array(n * 2) : null;
  const tmp = new THREE.Color();
  let o = 0;
  for (const c of chunks) {
    if (typeof c.rgb === "number") tmp.setHex(c.rgb);
    else tmp.copy(c.rgb);
    for (let i = 0; i < c.idx.length; i++, o++) {
      const v = c.idx[i] * 3;
      pos[o * 3] = c.pos[v];
      pos[o * 3 + 1] = c.pos[v + 1];
      pos[o * 3 + 2] = c.pos[v + 2];
      col[o * 3] = tmp.r;
      col[o * 3 + 1] = tmp.g;
      col[o * 3 + 2] = tmp.b;
      if (uv && c.uv) {
        uv[o * 2] = (c.uv[c.idx[i] * 2] ?? 0) / 65535;
        uv[o * 2 + 1] = (c.uv[c.idx[i] * 2 + 1] ?? 0) / 65535;
      }
    }
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  g.setAttribute("color", new THREE.BufferAttribute(col, 3));
  if (uv) g.setAttribute("uv", new THREE.BufferAttribute(uv, 2));
  g.computeVertexNormals();
  return g;
}

/** A box as non-indexed triangles with a flat vertex colour. */
function colourBox(sx: number, sy: number, sz: number, x: number, y: number, z: number, rgb: number): THREE.BufferGeometry {
  const g = new THREE.BoxGeometry(sx, sy, sz).toNonIndexed();
  g.translate(x, y, z);
  const c = new THREE.Color(rgb);
  const n = g.getAttribute("position").count;
  const a = new Float32Array(n * 3);
  for (let i = 0; i < n; i++) c.toArray(a, i * 3);
  g.setAttribute("color", new THREE.BufferAttribute(a, 3));
  g.deleteAttribute("uv");
  return g;
}

/** Concatenate non-indexed geometries (position + color), then flat normals. */
function join(gs: THREE.BufferGeometry[]): THREE.BufferGeometry {
  const n = gs.reduce((s, g) => s + g.getAttribute("position").count * 3, 0);
  const pos = new Float32Array(n);
  const col = new Float32Array(n);
  let o = 0;
  for (const g of gs) {
    const p = g.getAttribute("position").array as Float32Array;
    pos.set(p, o);
    col.set(g.getAttribute("color").array as Float32Array, o);
    o += p.length;
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  g.setAttribute("color", new THREE.BufferAttribute(col, 3));
  g.computeVertexNormals();
  return g;
}

/**
 * Head / tail lamp markers where VehicleView puts them (its lamp boxes, scaled 1.3 × 1 × 1.4,
 * moved onto the body's nose `front` and tail `rear` y), so lights look the same either side
 * of a promotion.
 */
function lampMarkers(info: CarModelInfo, front: number, rear: number): { head: THREE.BufferGeometry; tail: THREE.BufferGeometry } {
  const [w, , h] = info.body_size_m;
  const head: THREE.BufferGeometry[] = [];
  const tail: THREE.BufferGeometry[] = [];
  for (const sx of [-1, 1]) {
    head.push(colourBox(0.3 * 1.3, 0.05, 0.14 * 1.4, sx * (w / 2 - 0.25), front + 0.01, h * 0.45, 0xffffff));
    tail.push(colourBox(0.3 * 1.3, 0.05, 0.12 * 1.4, sx * (w / 2 - 0.25), rear - 0.01, h * 0.5, 0xffffff));
  }
  return { head: join(head), tail: join(tail) };
}

/**
 * Indicator box markers on the corners (just below the head / tail lamps, wrapping round the
 * corner so they show from the front or rear and the side): `left` (x < 0) and `right`
 * geometries, each the front and the rear lamp (vertex colour = amber hue). `front` / `rear`
 * are the body's nose / tail y. `skip`: corners the panel mesh has indicator lenses for (no
 * box there; a side can come out empty). The fallback for meshes without indicator parts and
 * the greybox proxy; shared by the fleet and VehicleView so lamps match either side of a
 * promotion.
 */
export function indicatorMarkers(
  info: { body_size_m: [number, number, number] | number[] },
  front: number,
  rear: number,
  skip: { front: boolean; rear: boolean } = NO_IND_ENDS,
): { left: THREE.BufferGeometry; right: THREE.BufferGeometry } {
  const [w, , h] = info.body_size_m;
  const amber = indicatorHue(false);
  const side = (sx: number) => {
    const boxes: THREE.BufferGeometry[] = [];
    if (!skip.front) boxes.push(colourBox(0.16, 0.2, 0.09, sx * (w / 2 - 0.06), front - 0.08, h * 0.45 - 0.15, amber));
    if (!skip.rear) boxes.push(colourBox(0.16, 0.2, 0.09, sx * (w / 2 - 0.06), rear + 0.08, h * 0.5 - 0.14, amber));
    return join(boxes);
  };
  return { left: side(-1), right: side(1) };
}

/** Nose and tail y of a car mesh (non-wheel parts, chassis space), m; null if it has none. */
export function meshFrontRear(mesh: CarMeshJson): { front: number; rear: number } | null {
  // Once per mesh (VehicleView asks for every promoted car).
  let hit = frontRearCache.get(mesh);
  if (hit === undefined) frontRearCache.set(mesh, (hit = computeFrontRear(mesh)));
  return hit;
}
const frontRearCache = new WeakMap<CarMeshJson, { front: number; rear: number } | null>();
function computeFrontRear(mesh: CarMeshJson): { front: number; rear: number } | null {
  let front = -Infinity;
  let rear = Infinity;
  for (const p of mesh.parts) {
    if (p.kind === "interior" || p.wheel != null) continue;
    const a = p.positions_mm;
    for (let i = 1; i < a.length; i += 3) {
      front = Math.max(front, a[i] * 1e-3);
      rear = Math.min(rear, a[i] * 1e-3);
    }
  }
  return Number.isFinite(front) && Number.isFinite(rear) ? { front, rear } : null;
}

/**
 * The car's panel mesh at rest (chassis space), wheels placed on their hubs. `police`: a police
 * model draws its livery scheme's body panels apart (`liv`, textured) and its light-bar lenses as
 * lens geometry (`lensL` …); otherwise livery panels are paint and lenses are detail.
 * `indicators`: the mesh's indicator lens parts go to `indL` / `indR` (blinked per instance);
 * otherwise (parked props, the police fleet: never blinking) they stay in their material's
 * geometry (amber detail / tail lamps), and `indL` / `indR` are the box markers.
 */
export function fullGeoms(mesh: CarMeshJson, info: CarModelInfo, police = false, indicators = false): Geoms {
  const paint: Chunk[] = [];
  const detail: Chunk[] = [];
  const tail: Chunk[] = [];
  const liv: Chunk[] = [];
  const indL: Chunk[] = [];
  const indR: Chunk[] = [];
  const lens: Record<"L" | "R" | "W" | "A", Chunk[]> = { L: [], R: [], W: [], A: [] };
  const textured = police && hasPoliceLivery(mesh);
  let front = -Infinity;
  let rear = Infinity;
  for (const p of mesh.parts) {
    if (p.kind === "interior") continue; // hidden behind glass that is opaque here
    const pos = new Float32Array(p.positions_mm.length);
    for (let i = 0; i < pos.length; i++) pos[i] = p.positions_mm[i] * 1e-3;
    if (p.wheel != null) {
      // Wheel parts are in hub space; VehicleView puts the hub at the wheel position (mid travel).
      const w = info.wheels[p.wheel];
      if (!w) continue;
      for (let i = 0; i < pos.length; i += 3) {
        pos[i] += w.position_m[0];
        pos[i + 1] += w.position_m[1];
        pos[i + 2] += w.position_m[2];
      }
    }
    if (p.wheel == null) {
      for (let i = 1; i < pos.length; i += 3) {
        front = Math.max(front, pos[i]);
        rear = Math.min(rear, pos[i]);
      }
    }
    const chunk: Chunk = { pos, idx: p.indices, rgb: 0xffffff, uv: p.uvs };
    const m = p.material;
    if (p.wheel == null && isIndicatorPart(p.name)) {
      // Indicator lenses: hue in the vertex colour, the instance colour is the blink brightness.
      // Never a light-bar channel (a police car's amber indicators are not advisors).
      if (indicators) (indicatorRight(p.name, p.positions_mm) ? indR : indL).push({ ...chunk, rgb: indicatorHue(m === "taillight") });
      else if (m === "taillight") tail.push(chunk);
      else detail.push({ ...chunk, rgb: detailColour(m, mesh, new THREE.Color()) });
      continue;
    }
    if (m === "livery" && textured) liv.push(chunk);
    else if (m === "paint" || m === "livery") paint.push(chunk);
    else if (m === "taillight") tail.push(chunk);
    else if (police && (m.startsWith("lightbar_") || m === "emissive_amber")) {
      // Lit hue in the vertex colour; the instance colour is the pattern's brightness.
      let cx = 0;
      for (let i = 0; i < pos.length; i += 3) cx += pos[i];
      const ch = m === "lightbar_clear" ? "W" : m === "emissive_amber" ? "A" : cx > 0 ? "R" : "L";
      const hue = m === "lightbar_red" ? LENS_ON.red : m === "lightbar_blue" ? LENS_ON.blue : m === "lightbar_clear" ? LENS_ON.clear : LENS_ON.amber;
      lens[ch].push({ ...chunk, rgb: hue });
    } else detail.push({ ...chunk, rgb: detailColour(m, mesh, new THREE.Color()) });
  }
  const l = info.body_size_m[1];
  const fy = Number.isFinite(front) ? front : l / 2;
  const ry = Number.isFinite(rear) ? rear : -l / 2;
  const lamps = lampMarkers(info, fy, ry);
  // Box markers only on corners without mesh lenses (all four when the lenses are not apart).
  const ind = indicatorMarkers(info, fy, ry, indicators ? meshIndicatorEnds(mesh) : NO_IND_ENDS);
  const lensGeo = (c: Chunk[]) => (c.length ? merge(c) : null);
  const hasLens = lens.L.length + lens.R.length > 0;
  // The mesh's own tail-light parts sit inside the shell; the markers are what shows.
  return {
    paint: merge(paint),
    liv: liv.length ? merge(liv, true) : null,
    detail: merge(detail),
    tail: join([merge(tail), lamps.tail]),
    head: lamps.head,
    indL: indL.length ? join([merge(indL), ind.left]) : ind.left,
    indR: indR.length ? join([merge(indR), ind.right]) : ind.right,
    lensL: hasLens ? lensGeo(lens.L) : null,
    lensR: hasLens ? lensGeo(lens.R) : null,
    lensW: hasLens ? lensGeo(lens.W) : null,
    lensA: hasLens ? lensGeo(lens.A) : null,
  };
}

/** Low-poly stand-in from the body size: lower body, cabin, glass band, wheels, lamps. */
export function proxyGeoms(info: CarModelInfo): Geoms {
  const [w, l, h] = info.body_size_m;
  const r = info.wheels[0]?.radius_m ?? 0.32;
  const bodyLo = r * 0.55;
  const bodyHi = h * 0.6;
  const roof = (h - bodyHi) * 0.25;
  const paint = join([
    colourBox(w, l, bodyHi - bodyLo, 0, 0, (bodyLo + bodyHi) / 2, 0xffffff),
    colourBox(w * 0.86, l * 0.42, roof, 0, -l * 0.04, h - roof / 2, 0xffffff),
  ]);
  const detail = join([
    colourBox(w * 0.88, l * 0.5, (h - bodyHi) * 0.75, 0, -l * 0.04, bodyHi + (h - bodyHi) * 0.375, detailRgb("glass")),
    ...info.wheels.map((wh) =>
      colourBox(wh.width_m, wh.radius_m * 1.9, wh.radius_m * 1.9, wh.position_m[0], wh.position_m[1], wh.position_m[2], detailRgb("rubber")),
    ),
  ]);
  const lamps = lampMarkers(info, l / 2, -l / 2);
  const ind = indicatorMarkers(info, l / 2, -l / 2);
  return { paint, detail, tail: lamps.tail, head: lamps.head, indL: ind.left, indR: ind.right };
}

/** Instance brightness of an unlit light-bar lens, and the extra of a fully lit one. */
const LENS_IDLE = 0.3;
const LENS_LIT = 1.9;

/** One set of instanced meshes (paint / detail / tail, optional livery, light bar or lenses,
 * and indicators). */
export class InstancedCar {
  readonly paint: THREE.InstancedMesh;
  /** Textured livery body (police schemes; no instance colour). */
  readonly liv: THREE.InstancedMesh | null;
  /** Light-bar lens channels L, R, W (clear), A (amber); null without mesh lenses. */
  readonly lens: (THREE.InstancedMesh | null)[] | null;
  readonly detail: THREE.InstancedMesh;
  readonly tail: THREE.InstancedMesh;
  readonly head: THREE.InstancedMesh;
  readonly bar: THREE.InstancedMesh | null;
  /** Left / right indicator lamps (two draw calls; only for moving traffic, `indicators`):
   * the mesh's indicator lenses (`fullGeoms`) or the box markers. */
  readonly indL: THREE.InstancedMesh | null;
  readonly indR: THREE.InstancedMesh | null;
  /** Every instanced mesh of the car (built once: `put` runs per instance per frame). */
  readonly meshes: readonly THREE.InstancedMesh[];
  count = 0;

  /** `indicators` adds the indicator lamp meshes (the fleet; parked props have none);
   * `livMat` draws `g.liv` (police livery texture). */
  constructor(g: Geoms, capacity: number, mats: FleetMaterials, barAt: [number, number] | null, indicators = false, livMat: THREE.Material | null = null) {
    const mk = (geo: THREE.BufferGeometry, mat: THREE.Material) => {
      const m = new THREE.InstancedMesh(geo, mat, capacity);
      m.count = 0;
      m.frustumCulled = false; // instances span the city; per-instance culling is not worth it
      return m;
    };
    this.paint = mk(g.paint, mats.paint);
    this.detail = mk(g.detail, mats.detail);
    this.tail = mk(g.tail, mats.lamp);
    this.head = mk(g.head, mats.lamp);
    if (barAt) {
      const bg = new THREE.BoxGeometry(barAt[0], 0.25, 0.12);
      bg.translate(0, -0.1, barAt[1] + 0.08);
      this.bar = mk(bg, mats.lamp);
    } else {
      this.bar = null;
    }
    // Vertex colour (hue) × instance colour (brightness), as the light-bar lenses.
    this.indL = indicators ? mk(g.indL, mats.lens) : null;
    this.indR = indicators ? mk(g.indR, mats.lens) : null;
    this.liv = g.liv && livMat ? mk(g.liv, livMat) : null;
    const lensMesh = (geo: THREE.BufferGeometry | null | undefined) => (geo ? mk(geo, mats.lens) : null);
    this.lens = g.lensL || g.lensR ? [lensMesh(g.lensL), lensMesh(g.lensR), lensMesh(g.lensW), lensMesh(g.lensA)] : null;
    const extra: THREE.InstancedMesh[] = [];
    if (this.liv) extra.push(this.liv);
    for (const m of this.lens ?? []) if (m) extra.push(m);
    this.meshes = [this.paint, this.detail, this.tail, this.head, ...(this.bar ? [this.bar] : []), ...(this.indL && this.indR ? [this.indL, this.indR] : []), ...extra];
  }

  /** Light-bar lens brightness of instance `i` (channels L, R, W, A in 0..1; null = all off).
   * No-op without mesh lenses. */
  putLens(i: number, levels: ArrayLike<number> | null): void {
    const lens = this.lens;
    if (!lens) return;
    for (let k = 0; k < 4; k++) {
      const m = lens[k];
      if (!m) continue;
      const v = LENS_IDLE + LENS_LIT * (levels ? levels[k] : 0);
      if (m.instanceColor === null) m.setColorAt(i, tmpLens.setRGB(v, v, v));
      else {
        const a = m.instanceColor.array as Float32Array;
        a[i * 3] = a[i * 3 + 1] = a[i * 3 + 2] = v;
      }
    }
  }

  add(m: THREE.Matrix4, paint: THREE.Color, tail: THREE.Color, bar: THREE.Color | null): void {
    this.put(this.count++, m, paint, tail, bar);
  }

  /** `add` with greybox hex colours (no Color round trip; the fleet's per-frame path). `ind` is
   * the lit indicator lamps (`IND` bits, already gated by the blink phase). */
  addHex(m: THREE.Matrix4, paint: Hex, tail: Hex, bar: Hex | null, ind = 0): void {
    this.putHex(this.count++, m, paint, tail, bar, true, ind);
  }

  /**
   * Write instance `i` (matrix and colours) without touching the others. Colours are the
   * greybox livery / TAIL / BAR values; the art style maps them here (styledPaint, styledLamp),
   * so every user of InstancedCar (fleet, parked props) gets the palette. `lit = false` draws
   * dark, unlit lenses (parked cars: no head or tail lights, playtest row 83).
   */
  put(i: number, m: THREE.Matrix4, paint: THREE.Color, tail: THREE.Color, bar: THREE.Color | null, lit = true): void {
    this.putHex(i, m, paint.getHex(), tail.getHex(), bar ? bar.getHex() : null, lit);
  }

  /** `put` with greybox hex colours (runs per car per frame: no allocation, no iterator).
   * `ind`: lit indicator lamps (`IND` bits; ignored without indicator meshes). */
  putHex(i: number, m: THREE.Matrix4, paint: Hex, tail: Hex, bar: Hex | null, lit = true, ind = 0): void {
    const meshes = this.meshes;
    for (let k = 0; k < meshes.length; k++) meshes[k].setMatrixAt(i, m);
    // Colours come from a per-style memo of the instance-buffer floats (`rgbMemo`): the same
    // values `setColorAt(i, color)` would write, copied without going through Color objects.
    syncRgbMemo();
    if (this.indL && this.indR) {
      this.indL.setColorAt(i, ind & IND.left && lit ? tmpIndOn : tmpIndOff);
      this.indR.setColorAt(i, ind & IND.right && lit ? tmpIndOn : tmpIndOff);
    }
    if (!lit) {
      const low = artStyle.lowpoly;
      putRgb(this.paint, i, paintRgb(low ? styledPaint(paint) : paint));
      putRgb(this.tail, i, paintRgb(UNLIT_TAIL));
      putRgb(this.head, i, paintRgb(UNLIT_HEAD));
      if (this.bar) putRgb(this.bar, i, low ? lampRgb(BAR.off) : paintRgb(BAR.off));
      return;
    }
    if (artStyle.lowpoly) {
      putRgb(this.paint, i, paintRgb(styledPaint(paint)));
      putRgb(this.tail, i, lampRgb(tail));
      putRgb(this.head, i, lampRgb(HEAD));
      if (this.bar) putRgb(this.bar, i, lampRgb(bar ?? BAR.off));
      return;
    }
    putRgb(this.paint, i, paintRgb(paint));
    putRgb(this.tail, i, paintRgb(tail));
    putRgb(this.head, i, paintRgb(HEAD));
    if (this.bar) putRgb(this.bar, i, paintRgb(bar ?? BAR.off));
  }

  end(): void {
    for (const mesh of this.meshes) {
      mesh.count = this.count;
      mesh.instanceMatrix.needsUpdate = true;
      if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
    }
  }

  /** Whether instance `i`'s left / right indicator lamps are lit (read back from the instance
   * colours; automation). */
  indicatorsLit(i: number): { left: boolean; right: boolean } {
    const lit = (m: THREE.InstancedMesh | null) => (m ? (m.getColorAt(i, tmpRead), tmpRead.r > 0.5) : false);
    return { left: lit(this.indL), right: lit(this.indR) };
  }

  dispose(): void {
    for (const mesh of this.meshes) {
      mesh.removeFromParent();
      mesh.geometry.dispose();
    }
  }
}

/** Materials shared by every instanced car (fleet and parked props look the same). */
export interface FleetMaterials {
  paint: THREE.Material;
  detail: THREE.Material;
  lamp: THREE.Material;
  /** Light-bar lenses (vertex colour × instance brightness). */
  lens: THREE.Material;
}

/** Lens colours of a car with its lights off (below 1, so they never reach the bloom pass). */
const UNLIT_TAIL = 0x3a0c0c;
const UNLIT_HEAD = 0x3c3b36;
/**
 * Instance colours per hex as the instance buffer stores them (Float32 linear RGB), so `putHex`
 * (per car per frame) copies three floats instead of a Color round trip through
 * `setHex` / `setColorAt` (~10 KB a frame at 3–4 fps, allocation pass 2026-10-20). `paintRgb`
 * = `Color.setHex(hex)`; `lampRgb` = `styledLamp(hex)`, which also depends on the art style
 * and on night > 0.2: that memo is cleared when either changes.
 */
const paintMemo = new Map<number, Float32Array>();
const lampMemo = new Map<number, Float32Array>();
let lampMemoState = -1;
const memoColor = new THREE.Color();
function syncRgbMemo(): void {
  const state = (artStyle.lowpoly ? 1 : 0) + (artStyle.night > 0.2 ? 2 : 0);
  if (state === lampMemoState) return;
  lampMemoState = state;
  lampMemo.clear();
}
function paintRgb(hex: Hex): Float32Array {
  let c = paintMemo.get(hex);
  if (!c) {
    c = new Float32Array(3);
    memoColor.setHex(hex).toArray(c);
    paintMemo.set(hex, c);
  }
  return c;
}
function lampRgb(hex: Hex): Float32Array {
  let c = lampMemo.get(hex);
  if (!c) {
    c = new Float32Array(3);
    styledLamp(memoColor, hex).toArray(c);
    lampMemo.set(hex, c);
  }
  return c;
}
/** `mesh.setColorAt(i, rgb)` for a colour already in instance-buffer floats. */
function putRgb(mesh: THREE.InstancedMesh, i: number, rgb: Float32Array): void {
  if (mesh.instanceColor === null) mesh.setColorAt(i, memoColor.fromArray(rgb));
  else (mesh.instanceColor.array as Float32Array).set(rgb, i * 3);
}
const tmpBar = new THREE.Color();
const tmpHead = new THREE.Color(HEAD);
const tmpPaint = new THREE.Color();
const tmpTail = new THREE.Color();
const tmpHeadStyled = new THREE.Color();
/** Lit / unlit indicator brightness as grey instance colours (refreshed per art style by
 * `Fleet.begin`; the hue is the geometry's vertex colour). */
const tmpIndOn = new THREE.Color();
const tmpIndOff = new THREE.Color();
const tmpRead = new THREE.Color();
const tmpLens = new THREE.Color();

/** Hide an instance set's meshes while it has no instances (per frame: no allocation). */
function showIfUsed(set: InstancedCar): void {
  const meshes = set.meshes;
  const on = set.count > 0;
  for (let k = 0; k < meshes.length; k++) meshes[k].visible = on;
}

/** One car model: full (mid-LOD mesh) and proxy instance sets. */
export class FleetModel {
  full: InstancedCar | null = null;
  proxy: InstancedCar;
  readonly info: CarModelInfo;
  /** Police livery scheme's body texture material (null: plain paint). */
  livMat: THREE.MeshStandardMaterial | null = null;
  /** Paint of the plain paint parts under a livery scheme (its base colour), else null. */
  liveryBase: number | null = null;
  /** The full mesh has light-bar lenses (flashed per instance; no box bar). */
  lensed = false;

  constructor(
    readonly carId: string,
    private readonly group: THREE.Group,
    private readonly mats: FleetMaterials,
    readonly police: boolean,
  ) {
    this.info = sim.carModelInfo(carId);
    this.proxy = new InstancedCar(proxyGeoms(this.info), MAX_PROXY, mats, this.barAt(), !this.police);
    group.add(...this.proxy.meshes);
    // The mid LOD (or the full mesh when there is no mid file): drawn undeformed here.
    const now = carMidMeshNow(carId);
    if (now) this.setMesh(now);
    else if (now === undefined) void loadCarMidMesh(carId).then((m) => m && this.setMesh(m));
  }

  private barAt(): [number, number] | null {
    const [w, , h] = this.info.body_size_m;
    return this.police ? [w * 0.64, h] : null;
  }

  private setMesh(mesh: CarMeshJson): void {
    if (this.full) return;
    const g = fullGeoms(mesh, this.info, this.police, !this.police);
    if (g.liv) {
      const tex = liveryTexture(mesh);
      this.livMat = new THREE.MeshStandardMaterial({ map: tex, flatShading: true, metalness: 0.25, roughness: 0.55 });
      applyCarEnv(this.livMat, !artStyle.lowpoly);
      this.liveryBase = liveryBaseColour(mesh)?.getHex() ?? null;
    }
    this.lensed = g.lensL != null || g.lensR != null;
    this.full = new InstancedCar(g, MAX_FULL, this.mats, this.lensed ? null : this.barAt(), !this.police, this.livMat);
    this.group.add(...this.full.meshes);
  }

  begin(): void {
    this.proxy.count = 0;
    if (this.full) this.full.count = 0;
  }

  end(): void {
    this.proxy.end();
    this.full?.end();
    // An empty set skips the render list (no per-mesh state setup for zero instances).
    showIfUsed(this.proxy);
    if (this.full) showIfUsed(this.full);
  }
}

/** Every instanced car of a frame. Call begin(), add() per car, end(). */
export class Fleet {
  readonly group = new THREE.Group();
  /** Full meshes within this distance from the camera (m); graphics quality lowers it on phones. */
  fullLodM = FULL_LOD_M;
  private readonly models = new Map<string, FleetModel>();
  /** Bumped whenever the models are dropped (`clear`): holders of FleetModel refs re-resolve. */
  generation = 0;
  readonly mats: FleetMaterials = {
    paint: new THREE.MeshStandardMaterial({ vertexColors: true, flatShading: true, metalness: 0.25, roughness: 0.55 }),
    detail: new THREE.MeshStandardMaterial({ vertexColors: true, flatShading: true, roughness: 0.6 }),
    lamp: new THREE.MeshBasicMaterial({ color: 0xffffff }),
    lens: new THREE.MeshBasicMaterial({ vertexColors: true }),
  };
  /** Art-style version the materials and model geometry were built for. */
  private styleVersion = -1;
  private readonly m = new THREE.Matrix4();
  private readonly p = new THREE.Vector3();
  private readonly q = new THREE.Quaternion();
  private readonly one = new THREE.Vector3(1, 1, 1);
  /** Instance set and index the last `add` wrote (automation read-back; null = not drawn). */
  readonly lastAdded: { car: InstancedCar | null; i: number } = { car: null, i: -1 };

  /** Model for `carId` (built on first use; `police` adds a light bar). */
  model(carId: string, police = false): FleetModel {
    const key = police ? `${carId}#police` : carId;
    let m = this.models.get(key);
    if (!m) {
      m = new FleetModel(carId, this.group, this.mats, police);
      this.models.set(key, m);
    }
    return m;
  }

  /** True once `carId`'s full instanced mesh exists. */
  hasFull(carId: string, police = false): boolean {
    return this.models.get(police ? `${carId}#police` : carId)?.full != null;
  }

  begin(): void {
    if (this.styleVersion !== artStyle.version) this.applyStyle();
    const on = indicatorLevel(true);
    const off = indicatorLevel(false);
    tmpIndOn.setRGB(on, on, on);
    tmpIndOff.setRGB(off, off, off);
    for (const m of this.models.values()) m.begin();
  }

  /**
   * Match the art style: paint / detail surfaces as R's panel meshes in that style (so a
   * promotion does not change the sheen), and rebuild the models (detail vertex colours are
   * baked into their geometry; car meshes are cached, so this is immediate).
   */
  private applyStyle(): void {
    this.styleVersion = artStyle.version;
    const lowpoly = artStyle.lowpoly;
    const paint = this.mats.paint as THREE.MeshStandardMaterial;
    const detail = this.mats.detail as THREE.MeshStandardMaterial;
    paint.roughness = lowpoly ? 0.85 : 0.55;
    paint.metalness = lowpoly ? 0.15 : 0.25;
    detail.roughness = lowpoly ? 0.85 : 0.6;
    // Image-based lighting in greybox, as the panel meshes (render/panels.ts applyCarEnv).
    applyCarEnv(paint, !lowpoly);
    applyCarEnv(detail, !lowpoly);
    if (this.models.size > 0) this.clear();
  }

  /**
   * Draw one car. `rec` is a draw record array and `o` the record's position offset (xyz then
   * quaternion xyzw). `dist` is its distance from the camera. `ind` is the lit indicator lamps
   * (`IND` bits, blink phase applied). Returns how it was drawn.
   */
  add(
    model: FleetModel,
    rec: Float32Array,
    o: number,
    dist: number,
    paint: number,
    tail: number,
    bar: number | null,
    ind = 0,
    lens: ArrayLike<number> | null = null,
  ): DrawMode {
    this.p.set(rec[o], rec[o + 1], rec[o + 2]);
    this.q.set(rec[o + 3], rec[o + 4], rec[o + 5], rec[o + 6]);
    this.m.compose(this.p, this.q, this.one);
    const full = model.full;
    const last = this.lastAdded;
    if (full && dist < this.fullLodM && full.count < MAX_FULL) {
      last.car = full;
      last.i = full.count;
      full.addHex(this.m, paint, tail, bar, ind);
      full.putLens(last.i, lens);
      return "full";
    }
    last.car = null;
    if (model.proxy.count < MAX_PROXY) {
      last.car = model.proxy;
      last.i = model.proxy.count;
      model.proxy.addHex(this.m, paint, tail, bar, ind);
    }
    return "proxy";
  }

  end(): void {
    for (const m of this.models.values()) m.end();
  }

  /** Drop every model (new session). */
  clear(): void {
    for (const m of this.models.values()) {
      m.proxy.dispose();
      m.full?.dispose();
      m.livMat?.dispose();
    }
    this.models.clear();
    this.generation++;
  }
}
