// Shared building blocks of the crash effects (docs/integration/crash-fx.md): a fixed-capacity
// particle pool drawn as one InstancedMesh, a per-instance alpha attribute for lit materials,
// and a tiny visual-only random generator.
//
// Everything lives in the scene's Z-up `world` group (sim coordinates: +Z up, metres).
// Nothing here allocates per frame: particle state is one Float32Array, dead particles are
// removed by copying the last one over them, and only the live instances are uploaded.
// Hot functions take integers and objects, not freshly computed numbers: V8 boxes a double
// passed to a function it does not inline (a small heap allocation per call), so per-instance
// values travel in the particle record and spawn parameters in reusable argument objects.
// `Math.hypot` allocates its argument list: use `Math.sqrt` of the sum of squares.
import * as THREE from "three";

/** Floats per particle in `Pool.d`. */
export const STRIDE = 30;
/** Field offsets inside one particle record. */
export const P = {
  /** Position, m. */
  px: 0,
  py: 1,
  pz: 2,
  /** Velocity, m/s. */
  vx: 3,
  vy: 4,
  vz: 5,
  /** Age and lifetime, s. */
  age: 6,
  life: 7,
  /** Size at birth / at death (meaning per system). */
  s0: 8,
  s1: 9,
  /** Orientation quaternion. */
  qx: 10,
  qy: 11,
  qz: 12,
  qw: 13,
  /** Angular velocity (rad/s about world X, Y, Z). */
  wx: 14,
  wy: 15,
  wz: 16,
  /** Local ground plane: z = ga + gb·x + gc·y. */
  ga: 17,
  gb: 18,
  gc: 19,
  /** Colour (linear RGB 0..1). */
  r: 20,
  g: 21,
  b: 22,
  /** 1 once the particle came to rest on the ground. */
  rest: 23,
  /** Free per-system values. */
  u0: 24,
  u1: 25,
  /** Instance scale and alpha for this frame (set by the system before `write`). */
  sx: 26,
  sy: 27,
  sz: 28,
  alpha: 29,
} as const;

/** Small xorshift32 generator for visual jitter (keeps screenshots reproducible). Never used by
 * the simulation. */
export class FxRng {
  private s: number;
  constructor(seed = 0x9e3779b9) {
    this.s = seed >>> 0 || 1;
  }
  /** Uniform in [0, 1). */
  next(): number {
    let x = this.s;
    x ^= x << 13;
    x >>>= 0;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    this.s = x;
    return x / 4294967296;
  }
  /** Uniform in [a, b). */
  range(a: number, b: number): number {
    return a + (b - a) * this.next();
  }
  /** Uniform in [-1, 1). */
  signed(): number {
    return this.next() * 2 - 1;
  }
  /** Stochastic rounding: `x` = 2.3 gives 2 (70 %) or 3 (30 %), so low rates still emit. */
  round(x: number): number {
    const f = Math.floor(x);
    return f + (this.next() < x - f ? 1 : 0);
  }
}

/**
 * Adds an `aAlpha` instance attribute to a built-in material: the fragment alpha is multiplied
 * by it (fog, lighting and flat shading stay Three's own).
 */
export function withInstanceAlpha<T extends THREE.Material>(mat: T, key: string): T {
  mat.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", "#include <common>\nattribute float aAlpha;\nvarying float vFxAlpha;")
      .replace("#include <begin_vertex>", "#include <begin_vertex>\nvFxAlpha = aAlpha;");
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", "#include <common>\nvarying float vFxAlpha;")
      .replace("#include <color_fragment>", "#include <color_fragment>\ndiffuseColor.a *= vFxAlpha;");
  };
  mat.customProgramCacheKey = () => key;
  return mat;
}

/** An upload range object reused every frame (`addUpdateRange` allocates one per call). */
export interface Range {
  start: number;
  count: number;
}

/**
 * Give `attr` one permanent upload range and return it: the caller sets `start` / `count`
 * (array elements) and `needsUpdate` each frame. Three empties `updateRanges` after every
 * upload, and refilling the emptied array re-allocates its storage, so the range stays put
 * (clearing is disabled for this attribute).
 */
export function ownRange(attr: THREE.BufferAttribute): Range {
  const range: Range = { start: 0, count: 0 };
  attr.updateRanges = [range];
  attr.clearUpdateRanges = () => {};
  return range;
}

const tmpM = new THREE.Matrix4();
const tmpP = new THREE.Vector3();
const tmpQ = new THREE.Quaternion();
const tmpS = new THREE.Vector3();

/**
 * A fixed-capacity particle set drawn as one InstancedMesh (one draw call, hidden when empty).
 * `cap` can be lowered at run time (quality) up to the allocated `max`.
 */
export class Pool {
  readonly mesh: THREE.InstancedMesh;
  /** Particle records, `STRIDE` floats each. */
  readonly d: Float32Array;
  /** Live particles (records 0..n-1). */
  n = 0;
  /** Current cap (≤ max). When full, new particles replace old ones round-robin. */
  cap: number;
  /** Particles spawned since the last reset (automation). */
  spawned = 0;
  private replace = 0;
  private readonly matrixRange: Range;
  private readonly colourRange: Range;
  private readonly alphaRange: Range | null;
  private readonly alpha: THREE.InstancedBufferAttribute | null;
  private readonly alphas: Float32Array | null;
  private readonly matrices: Float32Array;
  private readonly colours: Float32Array;
  /** Colour multiplier applied by `write` (per frame, e.g. brighter sparks at night). */
  colourScale = 1;

  constructor(
    geometry: THREE.BufferGeometry,
    material: THREE.Material,
    readonly max: number,
    withAlpha: boolean,
  ) {
    this.cap = max;
    this.d = new Float32Array(max * STRIDE);
    this.mesh = new THREE.InstancedMesh(geometry, material, max);
    this.mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    this.mesh.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(max * 3), 3);
    this.mesh.instanceColor.setUsage(THREE.DynamicDrawUsage);
    if (withAlpha) {
      this.alpha = new THREE.InstancedBufferAttribute(new Float32Array(max), 1);
      this.alpha.setUsage(THREE.DynamicDrawUsage);
      geometry.setAttribute("aAlpha", this.alpha);
      this.alphas = this.alpha.array as Float32Array;
    } else {
      this.alpha = null;
      this.alphas = null;
    }
    this.alphaRange = this.alpha ? ownRange(this.alpha) : null;
    this.matrixRange = ownRange(this.mesh.instanceMatrix);
    this.colourRange = ownRange(this.mesh.instanceColor);
    this.matrices = this.mesh.instanceMatrix.array as Float32Array;
    this.colours = this.mesh.instanceColor.array as Float32Array;
    // Particles spread over the whole map; the bounding sphere would be stale. One draw call.
    this.mesh.frustumCulled = false;
    this.mesh.count = 0;
    this.mesh.visible = false;
  }

  /** Index of a fresh record (fields zeroed, quaternion identity, age 0). */
  spawn(): number {
    let i: number;
    if (this.n < this.cap) i = this.n++;
    else {
      i = this.replace;
      this.replace = (this.replace + 1) % Math.max(this.cap, 1);
    }
    const o = i * STRIDE;
    this.d.fill(0, o, o + STRIDE);
    this.d[o + P.qw] = 1;
    this.spawned++;
    return o;
  }

  /** Remove record `i` (swap with the last one). Iterate backwards when killing in a loop. */
  kill(i: number): void {
    const last = --this.n;
    if (i !== last) this.d.copyWithin(i * STRIDE, last * STRIDE, last * STRIDE + STRIDE);
  }

  /** Drop particles over a lowered cap. */
  clampTo(cap: number): void {
    this.cap = Math.min(Math.max(cap, 0), this.max);
    if (this.n > this.cap) this.n = this.cap;
    this.replace = 0;
  }

  clear(): void {
    this.n = 0;
    this.replace = 0;
    this.spawned = 0;
    this.mesh.count = 0;
    this.mesh.visible = false;
  }

  /** Write instance `i`: position, record quaternion, scale `sx/sy/sz`, colour × `colourScale`, alpha. */
  write(i: number): void {
    const d = this.d;
    const o = i * STRIDE;
    tmpQ.set(d[o + P.qx], d[o + P.qy], d[o + P.qz], d[o + P.qw]);
    this.writeOriented(i, tmpQ);
  }

  /** Same as `write`, with an explicit orientation (streaks along their velocity). */
  writeOriented(i: number, q: THREE.Quaternion): void {
    const d = this.d;
    const o = i * STRIDE;
    tmpP.set(d[o + P.px], d[o + P.py], d[o + P.pz]);
    tmpS.set(d[o + P.sx], d[o + P.sy], d[o + P.sz]);
    tmpM.compose(tmpP, q, tmpS).toArray(this.matrices, i * 16);
    const c = this.colours;
    const k = this.colourScale;
    c[i * 3] = d[o + P.r] * k;
    c[i * 3 + 1] = d[o + P.g] * k;
    c[i * 3 + 2] = d[o + P.b] * k;
    if (this.alphas) this.alphas[i] = d[o + P.alpha];
  }

  /** Upload the live instances (call after writing all of them). */
  commit(): void {
    const n = this.n;
    this.mesh.count = n;
    this.mesh.visible = n > 0;
    if (n === 0) return;
    this.matrixRange.count = n * 16;
    this.mesh.instanceMatrix.needsUpdate = true;
    this.colourRange.count = n * 3;
    this.mesh.instanceColor!.needsUpdate = true;
    if (this.alpha && this.alphaRange) {
      this.alphaRange.count = n;
      this.alpha.needsUpdate = true;
    }
  }
}

/** Integrate a ballistic particle with ground bounce. Returns true when it just landed. */
export function ballistic(d: Float32Array, o: number, dt: number, drag: number, bounce: number, friction: number, lift: number): boolean {
  if (d[o + P.rest] > 0.5) return false;
  const k = Math.max(1 - drag * dt, 0);
  d[o + P.vx] *= k;
  d[o + P.vy] *= k;
  d[o + P.vz] = d[o + P.vz] * k - 9.81 * dt;
  d[o + P.px] += d[o + P.vx] * dt;
  d[o + P.py] += d[o + P.vy] * dt;
  d[o + P.pz] += d[o + P.vz] * dt;
  const gz = d[o + P.ga] + d[o + P.gb] * d[o + P.px] + d[o + P.gc] * d[o + P.py] + lift;
  if (d[o + P.pz] >= gz) return false;
  d[o + P.pz] = gz;
  if (d[o + P.vz] < 0) d[o + P.vz] = -d[o + P.vz] * bounce;
  d[o + P.vx] *= friction;
  d[o + P.vy] *= friction;
  return true;
}

/** Advance the record's orientation by its angular velocity over `dt` (q ← r(ω·dt) · q). */
export function spin(d: Float32Array, o: number, dt: number): void {
  const wx = d[o + P.wx];
  const wy = d[o + P.wy];
  const wz = d[o + P.wz];
  const w = Math.sqrt(wx * wx + wy * wy + wz * wz);
  if (w < 1e-4) return;
  const half = 0.5 * w * dt;
  const s = Math.sin(half) / w;
  const rx = wx * s;
  const ry = wy * s;
  const rz = wz * s;
  const rw = Math.cos(half);
  const qx = d[o + P.qx];
  const qy = d[o + P.qy];
  const qz = d[o + P.qz];
  const qw = d[o + P.qw];
  const x = rw * qx + rx * qw + ry * qz - rz * qy;
  const y = rw * qy - rx * qz + ry * qw + rz * qx;
  const z = rw * qz + rx * qy - ry * qx + rz * qw;
  const ww = rw * qw - rx * qx - ry * qy - rz * qz;
  const n = 1 / (Math.sqrt(x * x + y * y + z * z + ww * ww) || 1);
  d[o + P.qx] = x * n;
  d[o + P.qy] = y * n;
  d[o + P.qz] = z * n;
  d[o + P.qw] = ww * n;
}

/** A random orientation into the record (for tumbling pieces). */
export function randomOrientation(d: Float32Array, o: number, rng: FxRng): void {
  // Shoemake's uniform random quaternion.
  const u1 = rng.next();
  const u2 = rng.next() * Math.PI * 2;
  const u3 = rng.next() * Math.PI * 2;
  const a = Math.sqrt(1 - u1);
  const b = Math.sqrt(u1);
  d[o + P.qx] = a * Math.sin(u2);
  d[o + P.qy] = a * Math.cos(u2);
  d[o + P.qz] = b * Math.sin(u3);
  d[o + P.qw] = b * Math.cos(u3);
}
