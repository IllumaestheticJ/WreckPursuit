// Police spike strips in the 3D world (playtest row 88). wp_police places them and they burst
// tyres in the sim, but until now only the map drew them. Read from map_dynamic()'s tool list
// (crates/wp_wasm/src/map.rs, TOOL_STRIDE), a few times a second. Roadblocks are police cars
// parked across the road, which render/lod.ts already draws, so only spike strips are drawn.
// Sim space (+Z up); the group is parented under the scene's sim-space `world` group.
import * as THREE from "three";
import { decodeMapDynamic, type MapTool } from "../../../ui-kit/src/map/data";

/** Most spike strips drawn at once. */
const MAX_STRIPS = 8;
/** Teeth per metre along a strip, and rows across it. */
const TEETH_PER_M = 8;
const TEETH_ROWS = 3;
const MAX_TEETH = MAX_STRIPS * 16 * TEETH_PER_M * TEETH_ROWS;
/** Strip width across the road and base height, m. */
const STRIP_W = 0.32;
const STRIP_H = 0.035;
/** How often the tool list is refreshed, ms. */
const REFRESH_MS = 200;

/** Ground height under a sim-space point (m), or null if unknown. */
export type GroundFn = (x: number, y: number) => number | null;

/** Draws the active spike strips: a dark base with rows of steel teeth and yellow end caps. */
export class PoliceToolsView {
  readonly group = new THREE.Group();
  private readonly bases: THREE.Mesh[] = [];
  private readonly teeth: THREE.InstancedMesh;
  private readonly caps: THREE.InstancedMesh;
  private key = "";
  private lastMs = -Infinity;
  private readonly m = new THREE.Matrix4();
  private readonly q = new THREE.Quaternion();
  private readonly p = new THREE.Vector3();
  private readonly s = new THREE.Vector3(1, 1, 1);
  private readonly zAxis = new THREE.Vector3(0, 0, 1);

  constructor() {
    const baseMat = new THREE.MeshStandardMaterial({ color: 0x1a1a1c, roughness: 0.9, flatShading: true });
    const baseGeom = new THREE.BoxGeometry(1, STRIP_W, STRIP_H);
    for (let i = 0; i < MAX_STRIPS; i++) {
      const b = new THREE.Mesh(baseGeom, baseMat);
      b.visible = false;
      this.bases.push(b);
      this.group.add(b);
    }
    // A four-sided spike pointing up (+Z).
    const tooth = new THREE.ConeGeometry(0.022, 0.07, 4).rotateX(Math.PI / 2);
    this.teeth = new THREE.InstancedMesh(tooth, new THREE.MeshStandardMaterial({ color: 0xc9ced6, metalness: 0.8, roughness: 0.35, flatShading: true }), MAX_TEETH);
    this.teeth.count = 0;
    this.teeth.frustumCulled = false;
    const cap = new THREE.BoxGeometry(0.14, STRIP_W + 0.04, STRIP_H + 0.03);
    this.caps = new THREE.InstancedMesh(cap, new THREE.MeshStandardMaterial({ color: 0xffc21a, emissive: 0x3a2a00, roughness: 0.6 }), MAX_STRIPS * 2);
    this.caps.count = 0;
    this.caps.frustumCulled = false;
    this.group.add(this.teeth, this.caps);
  }

  /** Number of spike strips drawn (automation). */
  get drawn(): number {
    return this.bases.filter((b) => b.visible).length;
  }

  /**
   * Refresh from `map_dynamic(player)` (throttled). `read` returns the raw floats or throws
   * when no police system is running; `ground` seats strips on hilly roads.
   */
  update(read: () => Float32Array, ground: GroundFn, nowMs: number): void {
    if (nowMs - this.lastMs < REFRESH_MS) return;
    this.lastMs = nowMs;
    let spikes: MapTool[] = [];
    try {
      spikes = decodeMapDynamic(read()).tools.filter((t) => t.kind === "spikes");
    } catch {
      spikes = [];
    }
    // Strips are static once placed: rebuild only when the set changes.
    const key = spikes.map((t) => `${t.x.toFixed(1)},${t.y.toFixed(1)},${t.halfLenM.toFixed(1)}`).join(";");
    if (key === this.key) return;
    this.key = key;
    let nTeeth = 0;
    let nCaps = 0;
    for (let i = 0; i < MAX_STRIPS; i++) {
      const base = this.bases[i];
      const t = spikes[i];
      base.visible = !!t;
      if (!t) continue;
      const len = 2 * t.halfLenM;
      const dl = Math.hypot(t.dir[0], t.dir[1]) || 1;
      const dx = t.dir[0] / dl;
      const dy = t.dir[1] / dl;
      const z = ground(t.x, t.y) ?? 0;
      // Local +X runs along the strip.
      this.q.setFromAxisAngle(this.zAxis, Math.atan2(dy, dx));
      base.position.set(t.x, t.y, z + STRIP_H / 2);
      base.quaternion.copy(this.q);
      base.scale.set(len, 1, 1);
      // Teeth in rows along the strip.
      const n = Math.max(2, Math.round(len * TEETH_PER_M));
      for (let k = 0; k < n && nTeeth < MAX_TEETH - TEETH_ROWS; k++) {
        const along = -t.halfLenM + ((k + 0.5) / n) * len;
        for (let r = 0; r < TEETH_ROWS; r++) {
          const across = (r - (TEETH_ROWS - 1) / 2) * (STRIP_W * 0.3) + (k % 2 ? 0.02 : -0.02);
          this.p.set(t.x + dx * along - dy * across, t.y + dy * along + dx * across, z + STRIP_H + 0.03);
          this.teeth.setMatrixAt(nTeeth++, this.m.compose(this.p, this.q, this.s));
        }
      }
      for (const end of [-1, 1]) {
        this.p.set(t.x + dx * end * t.halfLenM, t.y + dy * end * t.halfLenM, z + (STRIP_H + 0.03) / 2);
        this.caps.setMatrixAt(nCaps++, this.m.compose(this.p, this.q, this.s));
      }
    }
    this.teeth.count = nTeeth;
    this.caps.count = nCaps;
    this.teeth.instanceMatrix.needsUpdate = true;
    this.caps.instanceMatrix.needsUpdate = true;
  }

  /** Hide everything (new session). */
  reset(): void {
    this.key = "";
    this.lastMs = -Infinity;
    for (const b of this.bases) b.visible = false;
    this.teeth.count = 0;
    this.caps.count = 0;
  }
}
