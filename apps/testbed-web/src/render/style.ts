// Art style switch for the testbed (machine S): "greybox" (the debug look every workstream
// built against) or "lowpoly" — stylised low-poly dark (docs/art/STYLE_DECISION.md,
// docs/art/ART_BIBLE.md). Low-poly dark is the default; `?style=greybox` or the "style"
// selector switches back so the owner can compare in the same build.
//
// What low-poly dark changes (render only, the sim never sees it):
// - city greybox materials take palette colours and flat shading; buildings get a small shader
//   patch: per-district façade tiers (district raster from map_static_json) and procedural
//   windows that glow at night (warm homes, cool offices), fading to their average with
//   distance so they never shimmer;
// - street lamps (instanced: poles, glowing heads, additive light pools on the road) placed
//   along the kerbs every ~30 m, sodium or LED by district;
// - a gradient sky dome, palette fog and hemisphere / sun light per time of day
//   (palette.ts TIME_OF_DAY), weather fog and cloud grey layered on top;
// - car paints, lamps and police light bars from the palette (vehicle_view.ts reads `artStyle`);
// - the post stack (post.ts): HDR target, bloom on lights only, ACES tonemap.
// Weather (render/weather.ts) keeps working on top: its wet-road patch sits on the same road
// materials (only their colours change) and its fog wins whenever it is thicker.
import * as THREE from "three";
import { tagFeature } from "../perf";
import {
  CITY_MATERIAL_COLOURS,
  DISTRICTS,
  DISTRICT_LOOKS,
  PALETTE,
  TIME_OF_DAY,
  districtLook,
  type Hex,
  type TimeOfDayLook,
} from "./palette";
import { LowPolyPost, type PostParams } from "./post";
import { CITY_TILE_M } from "./city_tiles";

/** Style ids. */
export type StyleId = "greybox" | "lowpoly";
/** Selector labels. */
export const STYLE_LABELS: Record<StyleId, string> = { greybox: "greybox", lowpoly: "low-poly dark" };

/**
 * Current style, read by views that colour themselves (vehicle_view.ts). `version` grows on
 * every switch so a view can re-apply its materials once.
 */
export const artStyle = {
  id: "lowpoly" as StyleId,
  version: 0,
  /** Night factor of the last frame (0 noon … 1 midnight): light pools, lamp glow. */
  night: 0,
  get lowpoly(): boolean {
    return this.id === "lowpoly";
  },
};

/** Linear THREE.Color from an sRGB palette hex. */
export function col(hex: Hex): THREE.Color {
  return new THREE.Color(hex);
}

/** Street-lamp spacing along kerbs, m (one lamp per grid cell of this size). */
const LAMP_CELL_M = 30;
const LAMP_HEIGHT_M = 7;
/** Pole → head arm, m (toward the road). */
const LAMP_ARM_M = 1.6;
const LAMP_POOL_RADIUS_M = 10;
/** District raster resolution (texels per side). */
const DISTRICT_RES = 256;

/** The parts of SceneView the style drives (scene.ts). */
export interface StyledScene {
  readonly scene: THREE.Scene;
  readonly renderer: THREE.WebGLRenderer;
  readonly hemi: THREE.HemisphereLight;
  readonly sun: THREE.DirectionalLight;
  readonly groundMaterial: THREE.MeshStandardMaterial;
  /** Graphics quality (render/quality.ts): the post target uses MSAA only when it allows. */
  readonly quality: { readonly antialias: boolean; readonly lite: boolean; readonly lampDrawM?: number };
  /** Light-bar glow multiplier for the police helicopters. */
  policeGlow: number;
  /** City draw distance, m (scene.ts; lamp tiles past it are hidden like city tiles). */
  readonly drawDistanceM: number;
  /** Render override (post stack); null = plain renderer.render. */
  renderOverride: ((camera: THREE.Camera) => void) | null;
  /** Compile override (shader prewarm of `objects`, lit by the scene, for the post stack's HDR
   * target); null = renderer.compile. */
  compileOverride: ((camera: THREE.Camera, objects: THREE.Scene) => void) | null;
  readonly signals: { setLampBoost(k: number): void };
}

/** map_static_json() subset used here. */
interface MapStaticJson {
  roads: { class: number; layer: number; pts: number[] }[];
  zones: { kind: string; poly: number[] }[];
}

/** Original values of a material the style recolours. */
interface Saved {
  mat: THREE.MeshStandardMaterial;
  color: THREE.Color;
  flat: boolean;
  roughness: number;
  metalness: number;
  lowpoly: THREE.Color;
}

/** District raster: index per texel (255 = none), sim-space rect. */
export interface DistrictMap {
  tex: THREE.DataTexture;
  data: Uint8Array;
  /** [x0, y0, width, height], sim metres. */
  rect: [number, number, number, number];
}

/** Rasterise district polygons (first zone wins, like `RoadNetwork::zones`) by scanline fill. */
export function districtMap(zones: MapStaticJson["zones"]): DistrictMap {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const z of zones)
    for (let i = 0; i + 1 < z.poly.length; i += 2) {
      x0 = Math.min(x0, z.poly[i]);
      x1 = Math.max(x1, z.poly[i]);
      y0 = Math.min(y0, z.poly[i + 1]);
      y1 = Math.max(y1, z.poly[i + 1]);
    }
  if (!isFinite(x0)) [x0, y0, x1, y1] = [0, 0, 1, 1];
  const n = DISTRICT_RES;
  const w = Math.max(x1 - x0, 1), h = Math.max(y1 - y0, 1);
  const data = new Uint8Array(n * n).fill(255);
  // Last zone first so earlier zones overwrite it.
  for (let zi = zones.length - 1; zi >= 0; zi--) {
    const z = zones[zi];
    const idx = (DISTRICTS as readonly string[]).indexOf(z.kind);
    const v = idx < 0 ? 1 : idx;
    const p = z.poly;
    const m = p.length >> 1;
    if (m < 3) continue;
    const xs: number[] = [];
    for (let row = 0; row < n; row++) {
      const y = y0 + ((row + 0.5) / n) * h;
      xs.length = 0;
      for (let i = 0, j = m - 1; i < m; j = i++) {
        const yi = p[2 * i + 1], yj = p[2 * j + 1];
        if (yi > y !== yj > y) xs.push(p[2 * i] + ((y - yi) / (yj - yi)) * (p[2 * j] - p[2 * i]));
      }
      xs.sort((a, b) => a - b);
      for (let k = 0; k + 1 < xs.length; k += 2) {
        const c0 = Math.max(0, Math.ceil(((xs[k] - x0) / w) * n - 0.5));
        const c1 = Math.min(n - 1, Math.floor(((xs[k + 1] - x0) / w) * n - 0.5));
        for (let c = c0; c <= c1; c++) data[row * n + c] = v;
      }
    }
  }
  const tex = new THREE.DataTexture(data, n, n, THREE.RedFormat, THREE.UnsignedByteType);
  tex.magFilter = THREE.NearestFilter;
  tex.minFilter = THREE.NearestFilter;
  tex.unpackAlignment = 1;
  tex.needsUpdate = true;
  return { tex, data, rect: [x0, y0, w, h] };
}

/** District index at a sim-space point (255 outside every zone). */
export function districtAt(d: DistrictMap, x: number, y: number): number {
  const [x0, y0, w, h] = d.rect;
  const c = Math.floor(((x - x0) / w) * DISTRICT_RES);
  const r = Math.floor(((y - y0) / h) * DISTRICT_RES);
  if (c < 0 || r < 0 || c >= DISTRICT_RES || r >= DISTRICT_RES) return 255;
  return d.data[r * DISTRICT_RES + c];
}

/** Building shader uniforms (shared by every city's building material). */
const buildingUniforms = {
  uStyleOn: { value: 0 },
  uNight: { value: 0 },
  uDistrict: { value: null as THREE.Texture | null },
  uDistrictRect: { value: new THREE.Vector4(0, 0, 1, 1) },
  uTiers: { value: Array.from({ length: 18 }, () => new THREE.Color()) },
  uWinColour: { value: Array.from({ length: 6 }, () => new THREE.Color()) },
  uWinLit: { value: new Array<number>(6).fill(0) },
  uWinDark: { value: col(PALETTE.windowDark) },
  uWinDay: { value: col(PALETTE.windowDay) },
  uWinGlow: { value: PALETTE.windowGlow },
};
DISTRICTS.forEach((d, i) => {
  const look = DISTRICT_LOOKS[d];
  look.buildings.forEach((hex, t) => buildingUniforms.uTiers.value[i * 3 + t].setHex(hex));
  buildingUniforms.uWinColour.value[i].setHex(look.window);
  buildingUniforms.uWinLit.value[i] = look.windowsLit;
});

/**
 * Building shader patch: district tiers + procedural windows. With uStyleOn = 0 the material
 * renders exactly as the greybox did.
 */
function patchBuilding(mat: THREE.MeshStandardMaterial, district: DistrictMap | null): void {
  const own = { ...buildingUniforms, uDistrict: { value: district?.tex ?? null }, uDistrictRect: { value: new THREE.Vector4(...(district?.rect ?? [0, 0, 1, 1])) }, uHasDistrict: { value: district ? 1 : 0 } };
  mat.onBeforeCompile = (sh) => {
    Object.assign(sh.uniforms, own);
    sh.vertexShader =
      "attribute float styleSeed;\nvarying vec3 vStyleWorld;\nvarying vec3 vStyleNormal;\nflat varying float vStyleSeed;\n" +
      sh.vertexShader.replace(
        "#include <project_vertex>",
        "#include <project_vertex>\n  vStyleWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;\n" +
          "  vStyleNormal = normalize(mat3(modelMatrix) * objectNormal);\n  vStyleSeed = styleSeed;",
      );
    sh.fragmentShader =
      /* glsl */ `varying vec3 vStyleWorld;
varying vec3 vStyleNormal;
flat varying float vStyleSeed;
uniform float uStyleOn, uNight, uWinGlow, uHasDistrict;
uniform sampler2D uDistrict;
uniform vec4 uDistrictRect;
uniform vec3 uTiers[18];
uniform vec3 uWinColour[6];
uniform float uWinLit[6];
uniform vec3 uWinDark;
uniform vec3 uWinDay;
float styleHash(vec3 p) { p = fract(p * 0.1031); p += dot(p, p.zyx + 31.32); return fract((p.x + p.y) * p.z); }
` +
      sh.fragmentShader
        .replace(
          "#include <color_fragment>",
          /* glsl */ `#include <color_fragment>
  vec3 styleGlow = vec3(0.0);
  if (uStyleOn > 0.5) {
    vec3 wp = vStyleWorld;
    // Per-triangle normal and façade seed from the CPU (prepareBuildings): derivative normals
    // are too noisy at city-scale coordinates and made the window grid sparkle.
    vec3 fn = normalize(vStyleNormal);
    // District from the raster (Three is +Y up; sim XY = (x, -z)); default Commercial.
    int di = 1;
    if (uHasDistrict > 0.5) {
      vec2 duv = (vec2(wp.x, -wp.z) - uDistrictRect.xy) / uDistrictRect.zw;
      float raw = texture2D(uDistrict, clamp(duv, 0.0, 1.0)).r * 255.0;
      if (raw < 5.5) di = int(raw + 0.5);
    }
    // One façade = one seed: it picks the tier and seeds its windows.
    vec3 fid = vec3(vStyleSeed * 113.0, vStyleSeed * 57.0, vStyleSeed * 29.0);
    float hf = fract(vStyleSeed * 7.31);
    int tier = hf < 0.4 ? 0 : (hf < 0.75 ? 1 : 2);
    vec3 base = uTiers[di * 3 + tier];
    if (abs(fn.y) < 0.3) {
      vec3 t = normalize(vec3(-fn.z, 0.0, fn.x));
      vec2 g = vec2(dot(wp, t) / 3.0, wp.y / 3.2);
      vec2 f = fract(g);
      vec2 cell = floor(g);
      float win = step(0.18, f.x) * step(f.x, 0.82) * step(0.3, f.y) * step(f.y, 0.82);
      // Far away a window is smaller than a pixel: blend to the average instead of shimmering.
      float fw = max(max(fwidth(g.x), fwidth(g.y)), 1e-4);
      float near = 1.0 - smoothstep(0.25, 0.6, fw);
      float h = styleHash(vec3(cell, 0.0) + fid);
      float litShare = uWinLit[di] * (0.35 + 0.65 * uNight);
      float lit = step(h, litShare);
      float area = 0.64 * 0.52;
      float mask = mix(area, win, near);
      float glow = mix(area * litShare, win * lit, near);
      // Cool or warm per window, within the district's character.
      vec3 wc = mix(uWinColour[di], vec3(1.0, 0.82, 0.55), step(0.82, fract(h * 13.7)) * 0.6);
      diffuseColor.rgb = mix(base, mix(uWinDay, uWinDark, uNight), mask * 0.8);
      styleGlow = wc * glow * uWinGlow * smoothstep(0.15, 0.7, uNight);
    } else {
      diffuseColor.rgb = base * 0.8; // roofs a shade darker
    }
  }`,
        )
        .replace("#include <emissivemap_fragment>", "#include <emissivemap_fragment>\n  totalEmissiveRadiance += styleGlow;");
  };
  mat.customProgramCacheKey = () => "wp-style-building";
  mat.needsUpdate = true;
}

/**
 * Give the building mesh exact per-triangle normals and a per-façade seed (attribute
 * `styleSeed`, 0..1: quantised normal + plane offset hashed, so both triangles of a wall quad
 * and every wall in the same plane share it). Non-indexed so every triangle owns its vertices;
 * the greybox look is unchanged (it was flat-shaded already).
 */
function prepareBuildings(mesh: THREE.Mesh): void {
  let g = mesh.geometry;
  if (g.getAttribute("styleSeed")) return;
  if (g.index) g = g.toNonIndexed();
  const pos = g.attributes.position;
  const normals = new Float32Array(pos.count * 3);
  const seeds = new Float32Array(pos.count);
  const a = new THREE.Vector3(), b = new THREE.Vector3(), c = new THREE.Vector3(), n = new THREE.Vector3(), e = new THREE.Vector3();
  for (let i = 0; i + 2 < pos.count; i += 3) {
    a.fromBufferAttribute(pos, i);
    b.fromBufferAttribute(pos, i + 1);
    c.fromBufferAttribute(pos, i + 2);
    n.subVectors(b, a).cross(e.subVectors(c, a));
    if (n.lengthSq() > 0) n.normalize();
    const d = Math.round(n.dot(a));
    let h = Math.imul(Math.round(n.x * 8) + 17, 73856093) ^ Math.imul(Math.round(n.z * 8) + 17, 19349663) ^ Math.imul(d, 83492791);
    h = Math.imul(h ^ (h >>> 13), 0x5bd1e995);
    const seed = ((h ^ (h >>> 15)) >>> 0) / 4294967296;
    for (let k = 0; k < 3; k++) {
      n.toArray(normals, (i + k) * 3);
      seeds[i + k] = seed;
    }
  }
  g.setAttribute("normal", new THREE.BufferAttribute(normals, 3));
  g.setAttribute("styleSeed", new THREE.BufferAttribute(seeds, 1));
  mesh.geometry = g;
}

/** Light pool shader: radial falloff, additive, fades with distance. */
const POOL_VS = /* glsl */ `
  varying vec2 vLocal; varying vec3 vCol; varying float vDist;
  void main() {
    vLocal = position.xy;
    vCol = instanceColor;
    vec4 mv = modelViewMatrix * instanceMatrix * vec4(position, 1.0);
    vDist = -mv.z;
    gl_Position = projectionMatrix * mv;
  }`;
const POOL_FS = /* glsl */ `
  uniform float uStrength; varying vec2 vLocal; varying vec3 vCol; varying float vDist;
  void main() {
    float r = length(vLocal);
    // Local +Y points at the road: fade out before the kerb so the sidewalk step never cuts it.
    float a = (1.0 - smoothstep(0.0, 1.0, r)) * smoothstep(-0.32, 0.05, vLocal.y) * (1.0 - smoothstep(180.0, 320.0, vDist));
    gl_FragColor = vec4(vCol * a * a * uStrength, 1.0);
  }`;

/** Headlight pool geometry per size. */
const poolGeos = new Map<string, THREE.BufferGeometry>();

/**
 * Headlight pool for one car (chassis space, +Y forward): an additive quad on the road ahead,
 * brightest near the car. Caller positions it and sets `uniforms.uStrength`.
 */
export function makeHeadlightPool(width: number, length: number): THREE.Mesh {
  // One geometry per size (a car view is built on every traffic / police promotion).
  const gk = `${width} ${length}`;
  let geo = poolGeos.get(gk);
  if (!geo) poolGeos.set(gk, (geo = new THREE.PlaneGeometry(width, length, 1, 1).translate(0, length / 2, 0)));
  const mat = new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    blending: THREE.AdditiveBlending,
    uniforms: { uStrength: { value: 0 }, uLen: { value: length }, uWidth: { value: width } },
    vertexShader: /* glsl */ `varying vec2 vP; void main() { vP = position.xy; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
    fragmentShader: /* glsl */ `
      uniform float uStrength, uLen, uWidth; varying vec2 vP;
      void main() {
        float d = clamp(vP.y / uLen, 0.0, 1.0);
        // Cone: narrow at the bumper, full width at the far end.
        float halfW = uWidth * (0.25 + 0.75 * d) * 0.5;
        float side = 1.0 - smoothstep(halfW * 0.55, halfW, abs(vP.x));
        float along = smoothstep(0.0, 0.08, d) * (1.0 - smoothstep(0.45, 1.0, d));
        gl_FragColor = vec4(vec3(1.0, 0.95, 0.82) * side * along * uStrength, 1.0);
      }`,
  });
  const m = new THREE.Mesh(geo, mat);
  m.renderOrder = 3;
  m.frustumCulled = false;
  return m;
}

/** Per-city data built once the city mesh and its map are both available. */
interface CityStyle {
  id: string;
  root: THREE.Object3D;
  building: THREE.MeshStandardMaterial | null;
  lamps: THREE.Group | null;
  /** Lamp tiles (one group per CITY_TILE_M cell) with their world bounding spheres. */
  lampTiles: { group: THREE.Group; sphere: THREE.Sphere }[];
  /** Lamp count (tests, docs). */
  lampCount: number;
  built: boolean;
}

/** Interpolate the time-of-day keyframes at `hour`. */
export function timeOfDay(hour: number): TimeOfDayLook {
  const h = ((hour % 24) + 24) % 24;
  let i = 0;
  while (i + 1 < TIME_OF_DAY.length - 1 && TIME_OF_DAY[i + 1].hour <= h) i++;
  const a = TIME_OF_DAY[i], b = TIME_OF_DAY[i + 1];
  const t = Math.min(Math.max((h - a.hour) / Math.max(b.hour - a.hour, 1e-6), 0), 1);
  const mixHex = (x: Hex, y: Hex) => col(x).lerp(col(y), t).getHex();
  const mixN = (x: number, y: number) => x + (y - x) * t;
  return {
    hour: h,
    skyTop: mixHex(a.skyTop, b.skyTop),
    horizon: mixHex(a.horizon, b.horizon),
    fogDensity: mixN(a.fogDensity, b.fogDensity),
    hemiSky: mixHex(a.hemiSky, b.hemiSky),
    hemiGround: mixHex(a.hemiGround, b.hemiGround),
    hemiIntensity: mixN(a.hemiIntensity, b.hemiIntensity),
    sun: mixHex(a.sun, b.sun),
    sunIntensity: mixN(a.sunIntensity, b.sunIntensity),
    sunElevationDeg: mixN(a.sunElevationDeg, b.sunElevationDeg),
    bloomThreshold: mixN(a.bloomThreshold, b.bloomThreshold),
    bloomStrength: mixN(a.bloomStrength, b.bloomStrength),
    exposure: mixN(a.exposure, b.exposure),
  };
}

/** Weather inputs the style reacts to (weather_info() subset). */
export interface StyleWeather {
  on: boolean;
  /** Cloud cover 0..1. */
  cloud: number;
}

/** Owns the low-poly dark look and the switch back to greybox. */
export class ArtStyle {
  private readonly post: LowPolyPost;
  private readonly saved: Saved[] = [];
  private readonly savedSet = new Set<THREE.Material>();
  private readonly cities = new Map<string, CityStyle>();
  private readonly fog = new THREE.FogExp2(0x1d2030, 0.004);
  private readonly sky: THREE.Mesh;
  private readonly skyUniforms = { uTop: { value: new THREE.Color() }, uHorizon: { value: new THREE.Color() }, uBottom: { value: new THREE.Color() } };
  private readonly greyLights: { hemiSky: THREE.Color; hemiGround: THREE.Color; sun: THREE.Color; sunPos: THREE.Vector3 };
  private readonly poolMaterials: THREE.ShaderMaterial[] = [];
  private readonly lampHeadMaterials: THREE.MeshBasicMaterial[] = [];
  private look: TimeOfDayLook = timeOfDay(12);
  private lastDt = 0;
  private readonly camPos = new THREE.Vector3();
  private lampTilesShown = 0;
  /** Last fog density the style itself asked for (before weather), for automation. */
  baseFogDensity = 0;
  /** Bloom on/off (debug). */
  get bloom(): boolean {
    return this.post.bloomOn;
  }
  set bloom(on: boolean) {
    this.post.bloomOn = on;
  }

  constructor(private readonly view: StyledScene, initial: StyleId) {
    // The post target's MSAA replaces the canvas MSAA (scene.ts `postAA`), so it follows the
    // graphics quality (none in the low profile); `?msaa=0` drops it in any profile.
    const msaa = view.quality.antialias && new URLSearchParams(location.search).get("msaa") !== "0";
    this.post = new LowPolyPost(msaa ? 4 : 0);
    // The low graphics profile skips bloom (its passes are ~10 % of a software-GL frame); the
    // tonemap / grade still runs, and the debug toggle can turn bloom back on.
    this.post.bloomOn = !view.quality.lite;
    this.greyLights = {
      hemiSky: view.hemi.color.clone(),
      hemiGround: view.hemi.groundColor.clone(),
      sun: view.sun.color.clone(),
      sunPos: view.sun.position.clone(),
    };
    // Sky dome: follows the camera, drawn first, never fogged.
    const skyMat = new THREE.ShaderMaterial({
      side: THREE.BackSide,
      depthWrite: false,
      fog: false,
      uniforms: this.skyUniforms,
      // On the far plane (z = w): drawn after the opaque scene with the depth test, so only the
      // pixels nothing covers are shaded (it was drawn first over the whole screen), and it
      // never hides geometry past its radius.
      vertexShader: /* glsl */ `varying vec3 vDir; void main() { vDir = position; vec4 p = projectionMatrix * modelViewMatrix * vec4(position, 1.0); gl_Position = p.xyww; }`,
      fragmentShader: /* glsl */ `
        uniform vec3 uTop, uHorizon, uBottom; varying vec3 vDir;
        void main() {
          float y = normalize(vDir).y;
          vec3 c = y > 0.0 ? mix(uHorizon, uTop, pow(min(y * 1.6, 1.0), 0.7)) : mix(uHorizon, uBottom, min(-y * 5.0, 1.0));
          gl_FragColor = vec4(c, 1.0);
        }`,
    });
    this.sky = new THREE.Mesh(new THREE.SphereGeometry(1500, 24, 12), skyMat);
    this.sky.renderOrder = 1; // after the opaque scene (renderOrder 0), see the vertex shader
    this.sky.frustumCulled = false;
    this.sky.visible = false;
    view.scene.add(tagFeature(this.sky, "sky"));
    this.track(view.groundMaterial, PALETTE.flatGround);
    view.renderOverride = (camera) => this.render(camera);
    view.compileOverride = (camera, objects) => {
      if (artStyle.lowpoly) this.post.compile(view.renderer, objects, camera, view.scene);
      else view.renderer.compile(objects, camera, view.scene);
    };
    this.set(initial);
  }

  get id(): StyleId {
    return artStyle.id;
  }

  /** Remember a material's greybox values and its low-poly colour. */
  private track(mat: THREE.MeshStandardMaterial, lowpoly: Hex): void {
    if (this.savedSet.has(mat)) return;
    this.savedSet.add(mat);
    this.saved.push({
      mat,
      color: mat.color.clone(),
      flat: mat.flatShading,
      roughness: mat.roughness,
      metalness: mat.metalness,
      lowpoly: col(lowpoly),
    });
    if (artStyle.lowpoly) this.applyOne(this.saved[this.saved.length - 1], true);
  }

  private applyOne(s: Saved, on: boolean): void {
    s.mat.color.copy(on ? s.lowpoly : s.color);
    s.mat.roughness = on ? 1 : s.roughness;
    s.mat.metalness = on ? 0 : s.metalness;
    if (s.mat.flatShading !== (on || s.flat)) {
      s.mat.flatShading = on || s.flat;
      s.mat.needsUpdate = true;
    }
  }

  /** Switch style (re-applies every tracked material; views pick it up via `artStyle.version`). */
  set(id: StyleId): void {
    artStyle.id = id;
    artStyle.version++;
    const on = id === "lowpoly";
    for (const s of this.saved) this.applyOne(s, on);
    buildingUniforms.uStyleOn.value = on ? 1 : 0;
    for (const c of this.cities.values()) if (c.lamps) c.lamps.visible = on;
    this.sky.visible = on;
    this.view.policeGlow = on ? PALETTE.policeRedGlow : 1;
    this.view.signals.setLampBoost(on ? PALETTE.signalGlow : 1);
    if (!on) {
      if (this.view.scene.fog === this.fog) this.view.scene.fog = null;
      this.view.hemi.color.copy(this.greyLights.hemiSky);
      this.view.hemi.groundColor.copy(this.greyLights.hemiGround);
      this.view.sun.color.copy(this.greyLights.sun);
      this.view.sun.position.copy(this.greyLights.sunPos);
    }
  }

  /** A city's glTF scene has loaded (scene.ts onCityLoaded). */
  attachCity(root: THREE.Object3D, cityId: string): void {
    if (this.cities.has(cityId)) return;
    const c: CityStyle = { id: cityId, root, building: null, lamps: null, lampTiles: [], lampCount: 0, built: false };
    this.cities.set(cityId, c);
    root.traverse((o) => {
      const m = (o as THREE.Mesh).material as THREE.Material | THREE.Material[] | undefined;
      for (const mat of m ? (Array.isArray(m) ? m : [m]) : []) {
        if (!(mat instanceof THREE.MeshStandardMaterial)) continue;
        const hex = CITY_MATERIAL_COLOURS[mat.name];
        if (hex !== undefined) this.track(mat, hex);
        if (mat.name === "building") {
          c.building = mat;
          prepareBuildings(o as THREE.Mesh);
        }
      }
    });
  }

  /**
   * Finish a city once the running session is in it (needs map_static_json for districts and
   * lamp sides). Cheap no-op after the first call per city.
   */
  ensureCity(cityId: string, mapJson: () => string): void {
    const c = this.cities.get(cityId);
    if (!c || c.built) return;
    c.built = true;
    let map: MapStaticJson | null = null;
    try {
      map = JSON.parse(mapJson()) as MapStaticJson | null;
    } catch {
      map = null;
    }
    const district = map && map.zones.length ? districtMap(map.zones) : null;
    if (c.building) patchBuilding(c.building, district);
    if (map) {
      c.lamps = this.buildLamps(c.root, map, district);
      c.lamps.visible = artStyle.lowpoly && c.root.visible !== false;
      this.view.scene.add(tagFeature(c.lamps, "lamps"));
    }
  }

  /** Lamps of a city (automation / docs). */
  lampCount(cityId: string): number {
    return this.cities.get(cityId)?.lampCount ?? 0;
  }

  /**
   * Street lamps along the kerbs: one per LAMP_CELL_M cell, on the sidewalk side of the kerb
   * face, arm over the road (the nearest road centreline says which side the road is).
   */
  private buildLamps(root: THREE.Object3D, map: MapStaticJson, district: DistrictMap | null): THREE.Group {
    const group = new THREE.Group();
    group.name = "style-street-lamps";
    // The city is tiled by now (render/city_tiles.ts): every tile of the kerb mesh shares its
    // material, so collect them all by material name.
    const kerbs: THREE.Mesh[] = [];
    root.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (mesh.isMesh && !Array.isArray(mesh.material) && (mesh.material as THREE.Material).name === "kerb") kerbs.push(mesh);
    });
    // Road segments (sim XY) bucketed on a 50 m grid for the nearest-centreline lookup.
    const BUCKET = 50;
    const buckets = new Map<string, number[]>();
    for (const r of map.roads) {
      if (r.layer !== 0) continue;
      for (let i = 0; i + 3 < r.pts.length; i += 2) {
        const [ax, ay, bx, by] = [r.pts[i], r.pts[i + 1], r.pts[i + 2], r.pts[i + 3]];
        const kx0 = Math.floor(Math.min(ax, bx) / BUCKET), kx1 = Math.floor(Math.max(ax, bx) / BUCKET);
        const ky0 = Math.floor(Math.min(ay, by) / BUCKET), ky1 = Math.floor(Math.max(ay, by) / BUCKET);
        for (let kx = kx0; kx <= kx1; kx++)
          for (let ky = ky0; ky <= ky1; ky++) {
            const k = `${kx},${ky}`;
            let b = buckets.get(k);
            if (!b) buckets.set(k, (b = []));
            b.push(ax, ay, bx, by);
          }
      }
    }
    const nearestRoad = (x: number, y: number): [number, number] | null => {
      let best = Infinity;
      let out: [number, number] | null = null;
      const kx = Math.floor(x / BUCKET), ky = Math.floor(y / BUCKET);
      for (let dx = -1; dx <= 1; dx++)
        for (let dy = -1; dy <= 1; dy++) {
          const b = buckets.get(`${kx + dx},${ky + dy}`);
          if (!b) continue;
          for (let i = 0; i < b.length; i += 4) {
            const [ax, ay, bx, by] = [b[i], b[i + 1], b[i + 2], b[i + 3]];
            const ex = bx - ax, ey = by - ay;
            const t = Math.min(Math.max(((x - ax) * ex + (y - ay) * ey) / Math.max(ex * ex + ey * ey, 1e-6), 0), 1);
            const px = ax + ex * t, py = ay + ey * t;
            const d = (px - x) ** 2 + (py - y) ** 2;
            if (d < best) {
              best = d;
              out = [px, py];
            }
          }
        }
      return best < 40 * 40 ? out : null;
    };

    // Candidates: kerb triangles facing sideways (the kerb's road face), one per cell.
    type Lamp = { base: THREE.Vector3; toRoad: THREE.Vector3; along: THREE.Vector3; led: boolean };
    const lamps: Lamp[] = [];
    const taken = new Set<string>();
    for (const k of kerbs) {
      k.updateWorldMatrix(true, false);
      const g = k.geometry;
      const pos = g.attributes.position;
      const idx = g.index;
      const tris = idx ? idx.count / 3 : pos.count / 3;
      const a = new THREE.Vector3(), b = new THREE.Vector3(), c = new THREE.Vector3();
      const n = new THREE.Vector3(), e1 = new THREE.Vector3(), e2 = new THREE.Vector3();
      for (let t = 0; t < tris; t++) {
        const i0 = idx ? idx.getX(t * 3) : t * 3;
        const i1 = idx ? idx.getX(t * 3 + 1) : t * 3 + 1;
        const i2 = idx ? idx.getX(t * 3 + 2) : t * 3 + 2;
        a.fromBufferAttribute(pos, i0).applyMatrix4(k.matrixWorld);
        b.fromBufferAttribute(pos, i1).applyMatrix4(k.matrixWorld);
        c.fromBufferAttribute(pos, i2).applyMatrix4(k.matrixWorld);
        e1.subVectors(b, a);
        e2.subVectors(c, a);
        n.crossVectors(e1, e2);
        const area = n.length();
        if (area < 1e-3) continue;
        n.divideScalar(area);
        if (Math.abs(n.y) > 0.3) continue; // top faces
        const cx = (a.x + b.x + c.x) / 3, cz = (a.z + b.z + c.z) / 3;
        const key = `${Math.floor(cx / LAMP_CELL_M)},${Math.floor(cz / LAMP_CELL_M)}`;
        if (taken.has(key)) continue;
        // Sim XY = (x, -z).
        const road = nearestRoad(cx, -cz);
        if (!road) continue;
        taken.add(key);
        const toRoad = new THREE.Vector3(road[0] - cx, 0, -road[1] - cz);
        if (toRoad.lengthSq() < 1e-4) continue;
        toRoad.normalize();
        // Along the kerb: the triangle's longest edge (carries the road grade).
        const ab = a.distanceTo(b), bc = b.distanceTo(c), ca = c.distanceTo(a);
        const along = (ab >= bc && ab >= ca ? e1.subVectors(b, a) : bc >= ca ? e1.subVectors(c, b) : e1.subVectors(a, c)).clone().normalize();
        const top = Math.max(a.y, b.y, c.y);
        const base = new THREE.Vector3(cx, top, cz).addScaledVector(toRoad, -0.7);
        const d = district ? districtAt(district, base.x, -base.z) : 255;
        const look = d < DISTRICTS.length ? DISTRICT_LOOKS[DISTRICTS[d]] : districtLook("Commercial");
        lamps.push({ base, toRoad, along, led: look.lamp === "led" });
      }
    }

    const count = lamps.length;
    const poleMat = new THREE.MeshStandardMaterial({ color: col(PALETTE.lampPole), roughness: 1, flatShading: true });
    // Pole and arm share a material: one merged geometry, one draw per tile.
    const poleGeo = mergeBoxes([
      new THREE.BoxGeometry(0.16, LAMP_HEIGHT_M, 0.16).translate(0, LAMP_HEIGHT_M / 2, 0),
      new THREE.BoxGeometry(0.1, 0.1, LAMP_ARM_M).translate(0, LAMP_HEIGHT_M - 0.05, LAMP_ARM_M / 2),
    ]);
    const headGeo = new THREE.BoxGeometry(0.34, 0.12, 0.62).translate(0, LAMP_HEIGHT_M - 0.12, LAMP_ARM_M);
    const poolGeo = new THREE.CircleGeometry(1, 20);
    const headMat = new THREE.MeshBasicMaterial({ color: 0xffffff, fog: true });
    this.lampHeadMaterials.push(headMat);
    const poolMat = new THREE.ShaderMaterial({
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      uniforms: { uStrength: { value: 0 } },
      vertexShader: POOL_VS,
      fragmentShader: POOL_FS,
    });
    this.poolMaterials.push(poolMat);
    // Lamps in city-tile cells (render/city_tiles.ts size): every cell is its own instanced
    // meshes with a bounding sphere, so Three frustum-culls them and update() hides cells past
    // the draw distance. One city-wide InstancedMesh (all lamps, never culled) cost more than
    // the whole greybox frame under SwiftShader (e2e/art_cost.mjs).
    const cells = new Map<string, Lamp[]>();
    for (const l of lamps) {
      const key = `${Math.floor(l.base.x / CITY_TILE_M)},${Math.floor(l.base.z / CITY_TILE_M)}`;
      let b = cells.get(key);
      if (!b) cells.set(key, (b = []));
      b.push(l);
    }
    const m = new THREE.Matrix4(), mp = new THREE.Matrix4();
    const sodium = col(PALETTE.lampSodium), led = col(PALETTE.lampLed);
    const up = new THREE.Vector3(0, 1, 0), side = new THREE.Vector3(), normal = new THREE.Vector3();
    const tiles: CityStyle["lampTiles"] = [];
    for (const key of [...cells.keys()].sort()) {
      const cell = cells.get(key)!;
      const n = cell.length;
      const poles = new THREE.InstancedMesh(poleGeo, poleMat, n);
      const heads = new THREE.InstancedMesh(headGeo, headMat, n);
      // Light pools are large additive discs: their fill is most of the lamps' cost on a
      // software rasterizer, so the low graphics profile goes without them.
      const pools = this.view.quality.lite ? null : new THREE.InstancedMesh(poolGeo, poolMat, n);
      if (pools) pools.renderOrder = 2;
      cell.forEach((l, i) => {
        // Pole frame: +Z toward the road, +Y up.
        side.crossVectors(up, l.toRoad).normalize();
        m.makeBasis(side, up, l.toRoad).setPosition(l.base);
        poles.setMatrixAt(i, m);
        heads.setMatrixAt(i, m);
        heads.setColorAt(i, l.led ? led : sodium);
        if (!pools) return;
        // Pool: a disc in the road plane under the head (tilted with the grade), 0.1 m up.
        normal.crossVectors(l.along, l.toRoad).normalize();
        if (normal.y < 0) normal.negate();
        // In-plane axes: y toward the road (the pool fades on the sidewalk side), x along it.
        const y = l.toRoad.clone().addScaledVector(normal, -l.toRoad.dot(normal)).normalize();
        const x = new THREE.Vector3().crossVectors(y, normal).normalize();
        const centre = l.base.clone().addScaledVector(l.toRoad, LAMP_ARM_M + 0.8);
        centre.y -= 0.13; // kerb top → road surface + ~2 cm
        mp.makeBasis(x.multiplyScalar(LAMP_POOL_RADIUS_M), y.multiplyScalar(LAMP_POOL_RADIUS_M), normal).setPosition(centre);
        pools.setMatrixAt(i, mp);
        pools.setColorAt(i, l.led ? led : sodium);
      });
      const tile = new THREE.Group();
      tile.name = `style-lamps#${key}`;
      const sphere = new THREE.Sphere();
      for (const im of pools ? [poles, heads, pools] : [poles, heads]) {
        im.instanceMatrix.needsUpdate = true;
        if (im.instanceColor) im.instanceColor.needsUpdate = true;
        im.computeBoundingSphere();
        if (sphere.isEmpty()) sphere.copy(im.boundingSphere!);
        else sphere.union(im.boundingSphere!);
        tile.add(im);
      }
      group.add(tile);
      tiles.push({ group: tile, sphere });
    }
    const c = [...this.cities.values()].find((x) => x.root === root);
    if (c) {
      c.lampTiles = tiles;
      c.lampCount = count;
    }
    return group;
  }

  /**
   * Once per frame before rendering: time of day (hour, night 0 noon … 1 midnight), weather
   * cloud / fog, camera (sky dome).
   */
  update(dt: number, hour: number, night: number, sunDimming: number, weather: StyleWeather, camera: THREE.Camera, cityId: string | null): void {
    this.lastDt = dt;
    artStyle.night = night;
    for (const c of this.cities.values()) if (c.lamps) c.lamps.visible = artStyle.lowpoly && c.id === cityId && c.root.parent?.visible !== false;
    if (!artStyle.lowpoly) return;
    this.cullLamps(cityId, camera);
    const v = this.view;
    const look = (this.look = timeOfDay(hour));
    buildingUniforms.uNight.value = night;

    // Sky + fog: weather clouds pull both toward the weather's fog grey.
    const scene = v.scene;
    const top = col(look.skyTop), horizon = col(look.horizon);
    this.baseFogDensity = look.fogDensity;
    if (weather.on && scene.fog && scene.fog !== this.fog && scene.fog instanceof THREE.FogExp2) {
      // Weather fog (weather.ts) is in charge; keep at least the style's distance fog.
      const wf = scene.fog;
      wf.density = Math.max(wf.density, look.fogDensity);
      wf.color.lerp(horizon, 0.35);
      const k = Math.min(1, 0.8 * weather.cloud);
      top.lerp(wf.color, k);
      horizon.copy(wf.color);
    } else {
      scene.fog = this.fog;
      this.fog.color.copy(horizon);
      this.fog.density = look.fogDensity;
    }
    if (scene.background instanceof THREE.Color) scene.background.copy(horizon);
    this.skyUniforms.uTop.value.copy(top);
    this.skyUniforms.uHorizon.value.copy(horizon);
    this.skyUniforms.uBottom.value.copy(horizon).multiplyScalar(0.6);
    this.sky.position.copy(camera.position);

    // Lights.
    v.hemi.color.setHex(look.hemiSky);
    v.hemi.groundColor.setHex(look.hemiGround);
    v.hemi.intensity = look.hemiIntensity * (0.6 + 0.4 * sunDimming);
    v.sun.color.setHex(look.sun);
    v.sun.intensity = look.sunIntensity * (0.35 + 0.65 * sunDimming);
    const el = (look.sunElevationDeg * Math.PI) / 180;
    v.sun.position.set(Math.cos(el) * 45, Math.sin(el) * 60, Math.cos(el) * 30);

    // Street lamps: dull lenses by day, glowing at night.
    const glow = 0.25 + (PALETTE.lampGlow - 0.25) * smooth(0.2, 0.7, night);
    for (const m of this.lampHeadMaterials) m.color.setScalar(glow);
    for (const m of this.poolMaterials) m.uniforms.uStrength.value = PALETTE.lampPool * smooth(0.25, 0.75, night);
  }

  /**
   * Hide lamp tiles past the city's draw distance (as scene.ts culls city tiles: the scene's
   * draw distance, the camera's far plane, fully fogged distance). Uses last frame's fog.
   */
  private cullLamps(cityId: string | null, camera: THREE.Camera): void {
    const c = cityId ? this.cities.get(cityId) : undefined;
    if (!c || !c.lamps?.visible) return;
    let far = this.view.drawDistanceM;
    if (camera instanceof THREE.PerspectiveCamera) far = Math.min(far, camera.far);
    far = Math.min(far, this.view.quality.lampDrawM ?? Infinity);
    const fog = this.view.scene.fog;
    if (fog instanceof THREE.FogExp2 && fog.density > 0) far = Math.min(far, 2.7 / fog.density);
    camera.getWorldPosition(this.camPos);
    let shown = 0;
    for (const t of c.lampTiles) {
      t.group.visible = t.sphere.center.distanceTo(this.camPos) - t.sphere.radius < far;
      if (t.group.visible) shown++;
    }
    this.lampTilesShown = shown;
  }

  /** Lamp tiles of a city: total and within the draw distance at the last frame (automation). */
  lampTileStats(cityId: string): { tiles: number; shown: number } {
    return { tiles: this.cities.get(cityId)?.lampTiles.length ?? 0, shown: this.lampTilesShown };
  }

  /** Post parameters for the current frame. */
  private postParams(): PostParams {
    return { threshold: this.look.bloomThreshold, bloom: this.look.bloomStrength, exposure: this.look.exposure };
  }

  /** Render through the post stack (low-poly dark) or plainly (greybox). */
  render(camera: THREE.Camera): void {
    const r = this.view.renderer;
    if (artStyle.lowpoly) this.post.render(r, this.view.scene, camera, this.postParams(), this.lastDt);
    else r.render(this.view.scene, camera);
  }
}

/** Merge non-indexed copies of box geometries (position + normal) into one geometry. */
function mergeBoxes(gs: THREE.BufferGeometry[]): THREE.BufferGeometry {
  const parts = gs.map((g) => g.toNonIndexed());
  const n = parts.reduce((a, g) => a + g.attributes.position.count, 0);
  const pos = new Float32Array(n * 3);
  const nor = new Float32Array(n * 3);
  let o = 0;
  for (const g of parts) {
    pos.set(g.attributes.position.array as Float32Array, o * 3);
    nor.set(g.attributes.normal.array as Float32Array, o * 3);
    o += g.attributes.position.count;
  }
  const out = new THREE.BufferGeometry();
  out.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  out.setAttribute("normal", new THREE.BufferAttribute(nor, 3));
  return out;
}

function smooth(e0: number, e1: number, x: number): number {
  const t = Math.min(Math.max((x - e0) / (e1 - e0), 0), 1);
  return t * t * (3 - 2 * t);
}

/** Paint for a vehicle view: player/scenario cars by id, traffic by id, police white. */
export function vehiclePaint(kind: "player" | "police" | "traffic", id: number): Hex {
  if (kind === "police") return PALETTE.policePaint;
  if (kind === "traffic") return PALETTE.trafficPaints[(id * 5) % PALETTE.trafficPaints.length];
  return PALETTE.carPaints[(id - 1 + PALETTE.carPaints.length) % PALETTE.carPaints.length];
}
