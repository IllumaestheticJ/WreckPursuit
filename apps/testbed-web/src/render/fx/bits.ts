// Loose bits (crash effects): glass shards and plastic / metal chips. Small flat-shaded pieces
// thrown on kinematic arcs (no physics bodies), bouncing on the local ground plane, coming to
// rest, staying a while and shrinking away. One InstancedMesh per kind.
import * as THREE from "three";
import { ballistic, FxRng, P, Pool, randomOrientation, spin, STRIDE } from "./pool";
import type { Plane } from "./sparks";

/** How one kind of bit moves and lives. */
export interface BitsStyle {
  name: string;
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  /** Rest height above the ground plane, m (half the piece's thickness, no z-fighting). */
  lift: number;
  /** Fraction of the vertical speed kept per bounce. */
  bounce: number;
  /** Fraction of the horizontal speed kept per bounce. */
  friction: number;
  /** Time spent shrinking away at the end of the life, s. */
  shrinkS: number;
}

/** Throw parameters (one reusable object per caller: no allocation per throw). */
export class BitArgs {
  /** Origin, m. */
  x = 0;
  y = 0;
  z = 0;
  /** Base velocity (inherited from the cars), m/s. */
  vx = 0;
  vy = 0;
  vz = 0;
  /** Burst direction bias (unit-ish). */
  nx = 0;
  ny = 0;
  nz = 1;
  /** Burst speed, m/s. */
  burst = 2;
  /** Piece size range, m. */
  smin = 0.04;
  smax = 0.1;
  /** Colours to pick from (linear RGB triplets). */
  colours: Float32Array = new Float32Array([0.5, 0.5, 0.5]);
  plane: Plane = { ga: 0, gb: 0, gc: 0 };
}

export class Bits {
  readonly pool: Pool;
  /** Lifetime of new bits, s (quality). */
  lifeS = 30;

  constructor(
    readonly style: BitsStyle,
    max: number,
  ) {
    this.pool = new Pool(style.geometry, style.material, max, false);
    this.pool.mesh.name = style.name;
  }

  /** Throw `count` bits described by `a` (reused by the caller; see `BitArgs`). */
  throw(rng: FxRng, count: number, a: BitArgs): void {
    const d = this.pool.d;
    const colours = a.colours;
    const nCol = colours.length / 3;
    for (let k = 0; k < count; k++) {
      const o = this.pool.spawn();
      const s = a.burst * rng.range(0.3, 1);
      d[o + P.px] = a.x + rng.signed() * 0.15;
      d[o + P.py] = a.y + rng.signed() * 0.15;
      d[o + P.pz] = a.z + rng.signed() * 0.08;
      d[o + P.vx] = a.vx + (a.nx * 0.7 + rng.signed() * 0.7) * s;
      d[o + P.vy] = a.vy + (a.ny * 0.7 + rng.signed() * 0.7) * s;
      d[o + P.vz] = a.vz + (Math.max(a.nz, 0) * 0.7 + rng.range(0.3, 1.0)) * s;
      d[o + P.wx] = rng.signed() * 18;
      d[o + P.wy] = rng.signed() * 18;
      d[o + P.wz] = rng.signed() * 18;
      randomOrientation(d, o, rng);
      d[o + P.s0] = rng.range(a.smin, a.smax);
      d[o + P.life] = this.lifeS * rng.range(0.75, 1.25);
      d[o + P.ga] = a.plane.ga;
      d[o + P.gb] = a.plane.gb;
      d[o + P.gc] = a.plane.gc;
      const c = Math.min(Math.floor(rng.next() * nCol), nCol - 1) * 3;
      const shade = rng.range(0.8, 1.1);
      d[o + P.r] = colours[c] * shade;
      d[o + P.g] = colours[c + 1] * shade;
      d[o + P.b] = colours[c + 2] * shade;
      d[o + P.alpha] = 1;
    }
  }

  update(dt: number): void {
    const pool = this.pool;
    const d = pool.d;
    const st = this.style;
    for (let i = pool.n - 1; i >= 0; i--) {
      const o = i * STRIDE;
      d[o + P.age] += dt;
      if (d[o + P.age] >= d[o + P.life]) {
        pool.kill(i);
        continue;
      }
      if (d[o + P.rest] > 0.5) continue;
      const landed = ballistic(d, o, dt, 0.3, st.bounce, st.friction, st.lift * d[o + P.s0]);
      if (landed) {
        d[o + P.wx] *= 0.5;
        d[o + P.wy] *= 0.5;
        d[o + P.wz] *= 0.5;
        // Settled: lie flat (drop the tumble about X/Y), keep the heading.
        if (Math.abs(d[o + P.vz]) < 0.5 && d[o + P.vx] * d[o + P.vx] + d[o + P.vy] * d[o + P.vy] < 0.36) {
          d[o + P.rest] = 1;
          const qz = d[o + P.qz];
          const qw = d[o + P.qw];
          const m = Math.sqrt(qz * qz + qw * qw) || 1;
          d[o + P.qx] = 0;
          d[o + P.qy] = 0;
          d[o + P.qz] = qz / m;
          d[o + P.qw] = qw / m;
        }
      }
      if (d[o + P.rest] < 0.5) spin(d, o, dt);
    }
    for (let i = 0; i < pool.n; i++) {
      const o = i * STRIDE;
      const left = d[o + P.life] - d[o + P.age];
      const s = d[o + P.s0] * Math.min(left / st.shrinkS, 1);
      d[o + P.sx] = s;
      d[o + P.sy] = s;
      d[o + P.sz] = s;
      pool.write(i);
    }
    pool.commit();
  }
}

/** A flat triangle shard (unit size, double-sided via the material). */
export function shardGeometry(): THREE.BufferGeometry {
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute([-0.5, -0.35, 0, 0.55, -0.2, 0, -0.05, 0.6, 0], 3));
  g.computeVertexNormals();
  return g;
}

/** A flat-shaded chip: a squashed tetrahedron (4 faces). */
export function chipGeometry(): THREE.BufferGeometry {
  return new THREE.TetrahedronGeometry(0.6, 0).scale(1, 1, 0.45);
}
