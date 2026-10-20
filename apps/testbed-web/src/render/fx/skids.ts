// Skid marks (crash effects): dark decal ribbons laid behind wheels that slip hard (burnouts,
// locked brakes, drifts). One ring buffer of quads in one mesh: a new segment overwrites the
// oldest, only the written range is uploaded, and the fade is done in the shader from each
// vertex's birth time, so old marks cost nothing per frame. The marks sit on the wheel's
// contact patch (the physics road surface, hills and bridges included), lifted 1.5 cm and drawn
// with polygonOffset so they never z-fight with the road.
import * as THREE from "three";
import { ownRange, type Range } from "./pool";

/** Floats per skid track (one per wheel): active, last centre xyz, last left edge xyz, last
 * right edge xyz, last strength. */
export const TRACK = 11;

const VERTS_PER_SEG = 4;

/** One wheel's skid input (one reusable object: no allocation per call). */
export class SkidArgs {
  /** Contact patch centre (already lifted off the road), m. */
  cx = 0;
  cy = 0;
  cz = 0;
  /** Unit vector across the tyre. */
  sx = 1;
  sy = 0;
  sz = 0;
  /** Half the tyre width, m. */
  halfW = 0.1;
  /** 0 = not skidding (breaks the ribbon) … 1 = black. */
  strength = 0;
  /** Effects clock, s (the fade starts from here). */
  time = 0;
}

export class Skids {
  readonly mesh: THREE.Mesh;
  /** Segments allocated. */
  readonly max: number;
  /** Segments in use before wrapping (quality, ≤ max). */
  cap: number;
  /** Seconds a mark takes to fade out completely (quality). */
  fadeS = 120;
  /** Segments laid since the last reset (automation). */
  laid = 0;
  /** Segment length, m. */
  readonly segM = 0.35;
  private next = 0;
  private used = 0;
  private readonly pos: Float32Array;
  private readonly info: Float32Array;
  private readonly posAttr: THREE.BufferAttribute;
  private readonly infoAttr: THREE.BufferAttribute;
  private readonly uniforms = { uTime: { value: 0 }, uFade: { value: 120 } };
  /** Dirty range of segments written this frame (first, end), -1 = clean. */
  private dirtyFrom = -1;
  private dirtyTo = -1;
  private wrapped = false;
  /** Permanent upload ranges of the two attributes (see `ownRange`). */
  private readonly posRange: Range;
  private readonly infoRange: Range;
  /** The new end of the segment being laid: left xyz, right xyz. */
  private readonly edge = new Float32Array(6);

  constructor(max: number) {
    this.max = max;
    this.cap = max;
    this.pos = new Float32Array(max * VERTS_PER_SEG * 3);
    this.info = new Float32Array(max * VERTS_PER_SEG * 2);
    const index = new Uint32Array(max * 6);
    for (let s = 0; s < max; s++) {
      const v = s * 4;
      index.set([v, v + 1, v + 2, v + 2, v + 1, v + 3], s * 6);
    }
    const geom = new THREE.BufferGeometry();
    this.posAttr = new THREE.BufferAttribute(this.pos, 3).setUsage(THREE.DynamicDrawUsage);
    this.infoAttr = new THREE.BufferAttribute(this.info, 2).setUsage(THREE.DynamicDrawUsage);
    this.posRange = ownRange(this.posAttr);
    this.infoRange = ownRange(this.infoAttr);
    geom.setAttribute("position", this.posAttr);
    geom.setAttribute("aSkid", this.infoAttr);
    geom.setIndex(new THREE.BufferAttribute(index, 1));
    geom.setDrawRange(0, 0);
    const mat = new THREE.MeshBasicMaterial({
      color: 0x0a0a0b,
      transparent: true,
      depthWrite: false,
      polygonOffset: true,
      polygonOffsetFactor: -4,
      polygonOffsetUnits: -8,
      side: THREE.DoubleSide,
      // Transparent + DoubleSide: without this Three draws back then front faces and flips
      // `side`, bumping `material.version` twice a render (re-resolves the shader program and
      // allocates its parameters every frame; browser-perf.md round 12b).
      forceSinglePass: true,
    });
    const u = this.uniforms;
    mat.onBeforeCompile = (shader) => {
      shader.uniforms.uTime = u.uTime;
      shader.uniforms.uFade = u.uFade;
      shader.vertexShader = shader.vertexShader
        .replace("#include <common>", "#include <common>\nattribute vec2 aSkid;\nuniform float uTime;\nuniform float uFade;\nvarying float vSkid;")
        .replace(
          "#include <begin_vertex>",
          "#include <begin_vertex>\nfloat skidAge = uTime - aSkid.x;\nvSkid = aSkid.y * (1.0 - smoothstep(uFade * 0.6, uFade, skidAge));",
        );
      shader.fragmentShader = shader.fragmentShader
        .replace("#include <common>", "#include <common>\nvarying float vSkid;")
        .replace("#include <color_fragment>", "#include <color_fragment>\ndiffuseColor.a *= vSkid;");
    };
    mat.customProgramCacheKey = () => "fx-skids";
    this.mesh = new THREE.Mesh(geom, mat);
    this.mesh.name = "fx-skids";
    this.mesh.frustumCulled = false;
    this.mesh.visible = false;
    this.mesh.renderOrder = 1;
  }

  /** Segments currently drawn. */
  get segments(): number {
    return this.used;
  }

  setCap(cap: number): void {
    this.cap = Math.min(Math.max(cap, 1), this.max);
    this.clear();
  }

  clear(): void {
    this.next = 0;
    this.used = 0;
    this.laid = 0;
    this.wrapped = false;
    this.mesh.geometry.setDrawRange(0, 0);
    this.mesh.visible = false;
  }

  /**
   * Advance one wheel's track (record at `o` in `tracks`, `TRACK` floats) with `a`: a strength
   * of 0 (not skidding) breaks the ribbon.
   */
  track(tracks: Float32Array, o: number, a: SkidArgs): void {
    if (a.strength <= 0) {
      tracks[o] = 0;
      return;
    }
    const cx = a.cx;
    const cy = a.cy;
    const cz = a.cz;
    const hw = a.halfW;
    const lx = cx - a.sx * hw;
    const ly = cy - a.sy * hw;
    const lz = cz - a.sz * hw;
    const rx = cx + a.sx * hw;
    const ry = cy + a.sy * hw;
    const rz = cz + a.sz * hw;
    const ex = cx - tracks[o + 1];
    const ey = cy - tracks[o + 2];
    const ez = cz - tracks[o + 3];
    const dist = tracks[o] > 0.5 ? Math.sqrt(ex * ex + ey * ey + ez * ez) : 0;
    if (tracks[o] > 0.5 && dist > 4) tracks[o] = 0; // teleport / reset: start a new ribbon
    if (tracks[o] > 0.5) {
      if (dist < this.segM) return;
      // The new end of the segment, read by `segment`.
      const e = this.edge;
      e[0] = lx;
      e[1] = ly;
      e[2] = lz;
      e[3] = rx;
      e[4] = ry;
      e[5] = rz;
      this.segment(tracks, o, a);
    }
    tracks[o] = 1;
    tracks[o + 1] = cx;
    tracks[o + 2] = cy;
    tracks[o + 3] = cz;
    tracks[o + 4] = lx;
    tracks[o + 5] = ly;
    tracks[o + 6] = lz;
    tracks[o + 7] = rx;
    tracks[o + 8] = ry;
    tracks[o + 9] = rz;
    tracks[o + 10] = a.strength;
  }

  private segment(tracks: Float32Array, o: number, a: SkidArgs): void {
    const e = this.edge;
    const strength = a.strength;
    const time = a.time;
    const s = this.next;
    const p = s * 12;
    const pos = this.pos;
    pos[p] = tracks[o + 4];
    pos[p + 1] = tracks[o + 5];
    pos[p + 2] = tracks[o + 6];
    pos[p + 3] = tracks[o + 7];
    pos[p + 4] = tracks[o + 8];
    pos[p + 5] = tracks[o + 9];
    pos[p + 6] = e[0];
    pos[p + 7] = e[1];
    pos[p + 8] = e[2];
    pos[p + 9] = e[3];
    pos[p + 10] = e[4];
    pos[p + 11] = e[5];
    const q = s * 8;
    const a0 = tracks[o + 10];
    const info = this.info;
    info[q] = time;
    info[q + 1] = a0;
    info[q + 2] = time;
    info[q + 3] = a0;
    info[q + 4] = time;
    info[q + 5] = strength;
    info[q + 6] = time;
    info[q + 7] = strength;
    if (this.dirtyFrom < 0) {
      this.dirtyFrom = s;
      this.dirtyTo = s + 1;
    } else if (s === this.dirtyTo) {
      this.dirtyTo = s + 1;
    } else {
      // Wrapped to 0 within this frame: upload the whole ring (once per `cap` segments).
      this.dirtyFrom = 0;
      this.dirtyTo = this.cap;
    }
    this.next = s + 1;
    if (this.next >= this.cap) {
      this.next = 0;
      this.wrapped = true;
    }
    this.used = this.wrapped ? this.cap : this.next;
    this.laid++;
  }

  /** Upload the segments written this frame. */
  private flush(): void {
    if (this.dirtyFrom < 0) return;
    const a = this.dirtyFrom;
    const n = this.dirtyTo - a;
    this.posRange.start = a * 12;
    this.posRange.count = n * 12;
    this.infoRange.start = a * 8;
    this.infoRange.count = n * 8;
    this.posAttr.needsUpdate = true;
    this.infoAttr.needsUpdate = true;
    this.dirtyFrom = -1;
    this.dirtyTo = -1;
  }

  /** Once per frame after all tracks: upload new segments, advance the fade clock. */
  update(time: number): void {
    this.flush();
    this.uniforms.uTime.value = time;
    this.uniforms.uFade.value = this.fadeS;
    this.mesh.geometry.setDrawRange(0, this.used * 6);
    this.mesh.visible = this.used > 0;
  }
}
