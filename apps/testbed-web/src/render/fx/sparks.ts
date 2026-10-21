// Sparks (crash effects): short-lived, unlit, bright streaks along the scrape direction. One
// InstancedMesh; each spark is a thin box stretched along its velocity (a motion streak) that
// cools from white-yellow to orange to dull red, bounces once or twice and dies.
import * as THREE from "three";
import { ballistic, FxRng, P, Pool, STRIDE } from "./pool";

const UP_Y = new THREE.Vector3(0, 1, 0);
const tmpDir = new THREE.Vector3();
const tmpQ = new THREE.Quaternion();

/** Ground plane `z = ga + gb·x + gc·y` handed to spawners. */
export interface Plane {
  ga: number;
  gb: number;
  gc: number;
}

/** Spark emission parameters (one reusable object per caller: no allocation per emit). */
export class SparkArgs {
  /** Origin, m. */
  x = 0;
  y = 0;
  z = 0;
  /** Unit travel direction. */
  dx = 0;
  dy = 0;
  dz = 1;
  /** Typical speed, m/s. */
  speed = 1;
  /** 0 = a tight jet … 1 = a hemisphere. */
  spread = 0.5;
  /** Kick off the surface (direction × 1 m/s). */
  nx = 0;
  ny = 0;
  nz = 0;
  /** Lifetime multiplier (hard-hit bursts live longer). */
  lifeScale = 1;
  plane: Plane = { ga: 0, gb: 0, gc: 0 };
}

export class Sparks {
  readonly pool: Pool;
  /** 0 day … 1 night: night sparks are thicker, live longer and read brighter. */
  night = 0;

  constructor(max: number) {
    // Unit box spanning y ∈ [-1, 0]: the head sits at the particle, the tail trails behind.
    const geom = new THREE.BoxGeometry(1, 1, 1).translate(0, -0.5, 0);
    // Unlit and not tone-mapped: sparks are emissive (the bloom of machine S picks them up).
    const mat = new THREE.MeshBasicMaterial({ color: 0xffffff, toneMapped: false });
    this.pool = new Pool(geom, mat, max, false);
    this.pool.mesh.name = "fx-sparks";
    this.pool.mesh.renderOrder = 3;
  }

  /**
   * Emit `count` sparks described by `a` (reused by the caller; see `SparkArgs`).
   */
  emit(rng: FxRng, count: number, a: SparkArgs): void {
    const d = this.pool.d;
    const lifeK = (1 + 0.4 * this.night) * a.lifeScale;
    for (let k = 0; k < count; k++) {
      const o = this.pool.spawn();
      const s = a.speed * rng.range(0.45, 1.05);
      const kick = rng.range(0.4, 2.2);
      d[o + P.px] = a.x;
      d[o + P.py] = a.y;
      d[o + P.pz] = a.z;
      d[o + P.vx] = (a.dx + rng.signed() * a.spread) * s + a.nx * kick;
      d[o + P.vy] = (a.dy + rng.signed() * a.spread) * s + a.ny * kick;
      d[o + P.vz] = (a.dz + rng.signed() * a.spread * 0.5) * s + a.nz * kick + rng.range(0.3, 1.8);
      d[o + P.life] = rng.range(0.12, 0.45) * lifeK;
      d[o + P.s0] = rng.range(0.6, 1.2); // thickness factor
      d[o + P.ga] = a.plane.ga;
      d[o + P.gb] = a.plane.gb;
      d[o + P.gc] = a.plane.gc;
      d[o + P.r] = 1;
      d[o + P.g] = 0.92;
      d[o + P.b] = 0.6;
    }
  }

  update(dt: number): void {
    const pool = this.pool;
    const d = pool.d;
    for (let i = pool.n - 1; i >= 0; i--) {
      const o = i * STRIDE;
      d[o + P.age] += dt;
      if (d[o + P.age] >= d[o + P.life]) {
        pool.kill(i);
        continue;
      }
      // A bounce costs a spark most of its energy and some of its life.
      if (ballistic(d, o, dt, 1.2, 0.3, 0.5, 0.01)) d[o + P.life] = Math.min(d[o + P.life], d[o + P.age] + 0.12);
    }
    const thick = 0.04 * (1 + 0.5 * this.night);
    pool.colourScale = 0.85 + 0.15 * this.night;
    for (let i = 0; i < pool.n; i++) {
      const o = i * STRIDE;
      const t = d[o + P.age] / d[o + P.life];
      // Colour ramp: white-yellow (0) → orange (0.35) → dull red (1).
      if (t < 0.35) {
        const k = t / 0.35;
        d[o + P.r] = 1;
        d[o + P.g] = 0.92 - 0.47 * k;
        d[o + P.b] = 0.6 - 0.52 * k;
      } else {
        const k = (t - 0.35) / 0.65;
        d[o + P.r] = 1 - 0.45 * k;
        d[o + P.g] = 0.45 - 0.37 * k;
        d[o + P.b] = 0.08 - 0.06 * k;
      }
      const vx = d[o + P.vx];
      const vy = d[o + P.vy];
      const vz = d[o + P.vz];
      const v = Math.sqrt(vx * vx + vy * vy + vz * vz);
      if (v > 1e-3) tmpQ.setFromUnitVectors(UP_Y, tmpDir.set(vx / v, vy / v, vz / v));
      else tmpQ.identity();
      // Motion streak: ~35 ms of travel, shrinking as the spark cools.
      const w = thick * d[o + P.s0] * (1 - 0.6 * t);
      d[o + P.sx] = w;
      d[o + P.sy] = Math.min(Math.max(v * 0.045, 0.05), 0.9) * (1 - 0.5 * t);
      d[o + P.sz] = w;
      pool.writeOriented(i, tmpQ);
    }
    pool.commit();
  }
}
