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
/**
 * Warning lamps (owner, 2026-10-26: "spike strips need lights to make them easier to spot"):
 * amber lamps at both ends and about every LAMP_SPACING_M along the strip, unlit-bright with a
 * glow halo so they read at night and in tunnels. Alternate lamps flash in turn at FLASH_HZ.
 */
const LAMP_SPACING_M = 1.5;
const MAX_LAMPS = MAX_STRIPS * 12;
const LAMP_R = 0.05;
const HALO_SIZE_M = 0.9;
const FLASH_HZ = 2.5;
const LAMP_ON = 0xffb020;
const LAMP_OFF = 0x4a3008;

/** Ground height under a sim-space point (m), or null if unknown. */
export type GroundFn = (x: number, y: number) => number | null;

/**
 * Draws the active spike strips: a dark base with rows of steel teeth, yellow end caps and
 * flashing amber warning lamps.
 */
export class PoliceToolsView {
  readonly group = new THREE.Group();
  private readonly bases: THREE.Mesh[] = [];
  private readonly teeth: THREE.InstancedMesh;
  private readonly caps: THREE.InstancedMesh;
  /** Lamp heads in two flash groups (even / odd along each strip) and their halos. */
  private readonly lamps: [THREE.InstancedMesh, THREE.InstancedMesh];
  private readonly lampMats: [THREE.MeshBasicMaterial, THREE.MeshBasicMaterial];
  private readonly halos: THREE.Sprite[] = [];
  private readonly haloMats: [THREE.SpriteMaterial, THREE.SpriteMaterial];
  private key = "";
  /** Height under each strip once known (by "x,y"): the ground ray is not cheap. */
  private readonly zAt = new Map<string, number>();
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
    const lampGeom = new THREE.SphereGeometry(LAMP_R, 8, 6);
    this.lampMats = [new THREE.MeshBasicMaterial({ color: LAMP_ON }), new THREE.MeshBasicMaterial({ color: LAMP_OFF })];
    this.lamps = [
      new THREE.InstancedMesh(lampGeom, this.lampMats[0], MAX_LAMPS),
      new THREE.InstancedMesh(lampGeom, this.lampMats[1], MAX_LAMPS),
    ];
    for (const l of this.lamps) {
      l.count = 0;
      l.frustumCulled = false;
      this.group.add(l);
    }
    const glow = haloTexture();
    const haloMat = () =>
      new THREE.SpriteMaterial({ map: glow, color: LAMP_ON, transparent: true, blending: THREE.AdditiveBlending, depthWrite: false });
    this.haloMats = [haloMat(), haloMat()];
    for (let i = 0; i < MAX_LAMPS; i++) {
      const h = new THREE.Sprite(this.haloMats[i % 2]);
      h.scale.set(HALO_SIZE_M, HALO_SIZE_M, 1);
      h.visible = false;
      this.halos.push(h);
      this.group.add(h);
    }
  }

  /** Flash the lamps: the two groups take turns. */
  private flash(nowMs: number): void {
    const on = Math.floor((nowMs / 1000) * FLASH_HZ * 2) % 2 === 0;
    this.lampMats[0].color.setHex(on ? LAMP_ON : LAMP_OFF);
    this.lampMats[1].color.setHex(on ? LAMP_OFF : LAMP_ON);
    this.haloMats[0].opacity = on ? 1 : 0.08;
    this.haloMats[1].opacity = on ? 0.08 : 1;
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
    this.flash(nowMs);
    if (nowMs - this.lastMs < REFRESH_MS) return;
    this.lastMs = nowMs;
    let spikes: MapTool[] = [];
    try {
      spikes = decodeMapDynamic(read()).tools.filter((t) => t.kind === "spikes");
    } catch {
      spikes = [];
    }
    // Strips are static once placed: rebuild only when the set changes, or when the height
    // under one does (the city greybox loads after a scenario's strip is down: the strip sat at
    // z = 0, under the road).
    const zs = spikes.slice(0, MAX_STRIPS).map((t) => {
      const at = `${t.x.toFixed(1)},${t.y.toFixed(1)}`;
      let z = this.zAt.get(at);
      if (z === undefined) {
        const g = ground(t.x, t.y);
        if (g !== null) this.zAt.set(at, (z = g));
      }
      return z ?? 0;
    });
    if (this.zAt.size > 4 * MAX_STRIPS) this.zAt.clear();
    const key = spikes.map((t, i) => `${t.x.toFixed(1)},${t.y.toFixed(1)},${t.halfLenM.toFixed(1)},${(zs[i] ?? 0).toFixed(1)}`).join(";");
    if (key === this.key) return;
    this.key = key;
    let nTeeth = 0;
    let nCaps = 0;
    const nLamps = [0, 0];
    let nHalos = 0;
    for (let i = 0; i < MAX_STRIPS; i++) {
      const base = this.bases[i];
      const t = spikes[i];
      base.visible = !!t;
      if (!t) continue;
      const len = 2 * t.halfLenM;
      const dl = Math.hypot(t.dir[0], t.dir[1]) || 1;
      const dx = t.dir[0] / dl;
      const dy = t.dir[1] / dl;
      const z = zs[i];
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
      // Lamps: one on each end cap and evenly between them, alternating flash groups.
      const gaps = Math.max(1, Math.round(len / LAMP_SPACING_M));
      for (let k = 0; k <= gaps && nHalos < MAX_LAMPS; k++) {
        const along = -t.halfLenM + (k / gaps) * len;
        const lz = z + STRIP_H + 0.03 + LAMP_R;
        this.p.set(t.x + dx * along, t.y + dy * along, lz);
        const g = k % 2;
        this.lamps[g].setMatrixAt(nLamps[g]++, this.m.compose(this.p, this.q, this.s));
        const h = this.halos[nHalos++];
        h.material = this.haloMats[g];
        // Raised so the road does not cut off the halo's lower half.
        h.position.set(this.p.x, this.p.y, lz + HALO_SIZE_M * 0.3);
        h.visible = true;
      }
    }
    this.teeth.count = nTeeth;
    this.caps.count = nCaps;
    this.teeth.instanceMatrix.needsUpdate = true;
    this.caps.instanceMatrix.needsUpdate = true;
    for (let g = 0; g < 2; g++) {
      this.lamps[g].count = nLamps[g];
      this.lamps[g].instanceMatrix.needsUpdate = true;
    }
    for (let i = nHalos; i < this.halos.length; i++) this.halos[i].visible = false;
  }

  /** Hide everything (new session). */
  reset(): void {
    this.key = "";
    this.lastMs = -Infinity;
    this.zAt.clear();
    for (const b of this.bases) b.visible = false;
    this.teeth.count = 0;
    this.caps.count = 0;
    for (const l of this.lamps) l.count = 0;
    for (const h of this.halos) h.visible = false;
  }
}

/** A soft round glow (white centre fading to clear) for the lamp halos. */
function haloTexture(): THREE.Texture {
  const size = 64;
  const c = document.createElement("canvas");
  c.width = c.height = size;
  const g = c.getContext("2d");
  if (g) {
    const r = g.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
    r.addColorStop(0, "rgba(255,255,255,1)");
    r.addColorStop(0.25, "rgba(255,255,255,0.55)");
    r.addColorStop(1, "rgba(255,255,255,0)");
    g.fillStyle = r;
    g.fillRect(0, 0, size, size);
  }
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}
