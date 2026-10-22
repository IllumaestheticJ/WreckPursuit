// Smoke (crash effects): low-poly puffs (flat-shaded icosahedra) that rise, grow and fade.
// Used for radiator steam (white), engine smoke (grey → black as the engine dies) and tyre
// smoke (light grey, low and wide). One InstancedMesh with a per-instance alpha.
import * as THREE from "three";
import { FxRng, P, Pool, randomOrientation, spin, STRIDE, withInstanceAlpha } from "./pool";

/** Puff parameters (one reusable object per emitter: no allocation per puff). */
export class PuffArgs {
  /** Origin, m. */
  x = 0;
  y = 0;
  z = 0;
  /** Initial velocity, m/s. */
  vx = 0;
  vy = 0;
  vz = 0;
  /** Colour, linear RGB. */
  r = 1;
  g = 1;
  b = 1;
  /** Radius at birth and at death, m. */
  s0 = 0.2;
  s1 = 1;
  /** Lifetime, s. */
  life = 1.5;
  /** Peak opacity. */
  alpha = 0.5;
  /** Buoyancy, m/s². */
  rise = 1;

  /** Set the look (colour, size, life, opacity, buoyancy) in one go. */
  look(r: number, g: number, b: number, s0: number, s1: number, life: number, alpha: number, rise: number): this {
    this.r = r;
    this.g = g;
    this.b = b;
    this.s0 = s0;
    this.s1 = s1;
    this.life = life;
    this.alpha = alpha;
    this.rise = rise;
    return this;
  }
}

export class Smoke {
  readonly pool: Pool;

  constructor(max: number) {
    const geom = new THREE.IcosahedronGeometry(1, 0);
    const mat = withInstanceAlpha(
      new THREE.MeshLambertMaterial({ color: 0xffffff, flatShading: true, transparent: true, depthWrite: false }),
      "fx-smoke",
    );
    this.pool = new Pool(geom, mat, max, true);
    this.pool.mesh.name = "fx-smoke";
    // After the cars and glass, so puffs over a windscreen still show.
    this.pool.mesh.renderOrder = 4;
  }

  /** One puff described by `a` (reused by the caller; see `PuffArgs`). */
  puff(rng: FxRng, a: PuffArgs): void {
    const d = this.pool.d;
    const o = this.pool.spawn();
    d[o + P.px] = a.x + rng.signed() * 0.08;
    d[o + P.py] = a.y + rng.signed() * 0.08;
    d[o + P.pz] = a.z;
    d[o + P.vx] = a.vx + rng.signed() * 0.3;
    d[o + P.vy] = a.vy + rng.signed() * 0.3;
    d[o + P.vz] = a.vz + rng.range(0, 0.3);
    const shade = rng.range(0.92, 1.05);
    d[o + P.r] = Math.min(a.r * shade, 1);
    d[o + P.g] = Math.min(a.g * shade, 1);
    d[o + P.b] = Math.min(a.b * shade, 1);
    d[o + P.s0] = a.s0 * rng.range(0.8, 1.2);
    d[o + P.s1] = a.s1 * rng.range(0.85, 1.2);
    d[o + P.life] = a.life * rng.range(0.8, 1.2);
    d[o + P.u0] = a.alpha;
    d[o + P.u1] = a.rise;
    d[o + P.wx] = rng.signed() * 0.8;
    d[o + P.wy] = rng.signed() * 0.8;
    d[o + P.wz] = rng.signed() * 0.8;
    randomOrientation(d, o, rng);
  }

  update(dt: number): void {
    const pool = this.pool;
    const d = pool.d;
    const k = Math.max(1 - 1.4 * dt, 0);
    for (let i = pool.n - 1; i >= 0; i--) {
      const o = i * STRIDE;
      d[o + P.age] += dt;
      if (d[o + P.age] >= d[o + P.life]) {
        pool.kill(i);
        continue;
      }
      d[o + P.vx] *= k;
      d[o + P.vy] *= k;
      d[o + P.vz] = d[o + P.vz] * k + d[o + P.u1] * dt;
      d[o + P.px] += d[o + P.vx] * dt;
      d[o + P.py] += d[o + P.vy] * dt;
      d[o + P.pz] += d[o + P.vz] * dt;
      spin(d, o, dt);
    }
    for (let i = 0; i < pool.n; i++) {
      const o = i * STRIDE;
      const t = d[o + P.age] / d[o + P.life];
      const grow = 1 - (1 - t) * (1 - t); // ease out
      const s = d[o + P.s0] + (d[o + P.s1] - d[o + P.s0]) * grow;
      // Quick fade in, long fade out.
      const a = t < 0.12 ? t / 0.12 : Math.pow(1 - (t - 0.12) / 0.88, 1.5);
      d[o + P.sx] = s;
      d[o + P.sy] = s;
      d[o + P.sz] = s * 0.8;
      d[o + P.alpha] = a * d[o + P.u0];
      pool.write(i);
    }
    pool.commit();
  }
}
