// Crash effects (lead, owner request row 69; docs/integration/crash-fx.md): sparks, smoke,
// glass shards, skid marks and loose debris that make impacts feel heavy.
//
// Purely visual: reads the drained sim events and the vehicle states the frame loop already
// has, never calls into the sim, never feeds anything back (determinism and goldens are
// untouched). Five pooled InstancedMeshes / one ring-buffer mesh = at most 5 draw calls, no
// per-frame allocation in steady state. Quality (`setQuality("low" | "med" | "high")`) scales
// pool sizes, emission rates, skid length and how long marks and bits stay.
//
// Everything is in sim coordinates under the scene's Z-up `world` group.
import * as THREE from "three";
import type { FxLevel } from "../quality";
import type { VehicleView } from "../../vehicle_view";
import { S, STATE_STRIDE, SW } from "../../wasm/layout";
import { BitArgs, Bits, chipGeometry, shardGeometry } from "./bits";
import { FxRng, P, STRIDE } from "./pool";
import { SkidArgs, Skids, TRACK } from "./skids";
import { PuffArgs, Smoke } from "./smoke";
import { SparkArgs, Sparks, type Plane } from "./sparks";

export type { FxLevel } from "../quality";

/** Per-quality caps and rates. */
export interface FxPreset {
  sparks: number;
  smoke: number;
  shards: number;
  chips: number;
  /** Skid ribbon segments (0.35 m each) kept before the oldest is overwritten. */
  skidSegments: number;
  /** Seconds until a skid mark has faded out. */
  skidFadeS: number;
  /** Emission multiplier (sparks, smoke, bits). */
  rate: number;
  /** Tyre smoke multiplier (0 = none). */
  tyreSmoke: number;
  /** Seconds glass shards stay before shrinking away. */
  shardLifeS: number;
  /** Seconds debris chips stay. */
  chipLifeS: number;
}

/** Quality presets. `low` is what the headless / software-GL profile runs (perf budget). */
export const FX_PRESETS: Record<FxLevel, FxPreset> = {
  low: { sparks: 160, smoke: 96, shards: 96, chips: 128, skidSegments: 2048, skidFadeS: 60, rate: 0.45, tyreSmoke: 0.35, shardLifeS: 6, chipLifeS: 25 },
  med: { sparks: 400, smoke: 192, shards: 192, chips: 320, skidSegments: 6144, skidFadeS: 150, rate: 0.75, tyreSmoke: 0.7, shardLifeS: 9, chipLifeS: 60 },
  high: { sparks: 800, smoke: 320, shards: 384, chips: 640, skidSegments: 12288, skidFadeS: 300, rate: 1, tyreSmoke: 1, shardLifeS: 12, chipLifeS: 120 },
};
const MAX = FX_PRESETS.high;

/** `wp_contracts::WORLD_ENTITY` (static world in Contact events). */
const WORLD = 4294967295;

// Thresholds (calibrated on crash/head_on.ron and the night run; see the doc).
/** Sliding speed above which a contact throws sparks, m/s. */
const SCRAPE_MIN_MPS = 2.5;
/** Per-tick impulse below which a contact is too light to spark, N·s. */
const SCRAPE_MIN_NS = 60;
/** Closing speed and per-tick impulse of a hard hit (spark burst + debris). */
const HIT_MIN_MPS = 3.5;
const HIT_MIN_NS = 250;
/** Minimum time between two hard-hit bursts of the same pair, s. */
const HIT_DEBOUNCE_S = 0.3;
/** Tyre slip speed (|ω|·r vs the road, m/s) that lays a skid mark / makes tyre smoke. */
const SKID_SLIP_MPS = 4;
const TYRE_SMOKE_SLIP_MPS = 7;
/** RegionDamaged severity of the front / rear that breaks the head / tail lights. */
const LIGHT_BREAK_SEVERITY = 0.45;
/** Engine / radiator health below which they smoke / steam. */
const ENGINE_SMOKE_HEALTH = 0.55;
const RADIATOR_STEAM_HEALTH = 0.7;

/** Fallback glass sites (fractions of body width, length, height) when the panel mesh is not
 * loaded (debug box / proxy). */
const GLASS_SITE: Record<string, [number, number, number]> = {
  FrontBumper: [0, 0.48, 0.45],
  Hood: [0, 0.3, 0.7],
  Windshield: [0, 0.15, 0.85],
  Roof: [0, -0.05, 0.95],
  RearGlass: [0, -0.33, 0.85],
  Trunk: [0, -0.4, 0.75],
  RearBumper: [0, -0.48, 0.5],
  LeftDoorF: [-0.5, 0.05, 0.75],
  LeftDoorR: [-0.5, -0.2, 0.75],
  RightDoorF: [0.5, 0.05, 0.75],
  RightDoorR: [0.5, -0.2, 0.75],
  FrontLeftFender: [-0.5, 0.35, 0.65],
  FrontRightFender: [0.5, 0.35, 0.65],
  RearLeftQuarter: [-0.5, -0.35, 0.7],
  RearRightQuarter: [0.5, -0.35, 0.7],
};

const GLASS_COLOURS = new Float32Array([0.62, 0.78, 0.86, 0.45, 0.6, 0.7, 0.85, 0.93, 1.0]);
const HEADLIGHT_COLOURS = new Float32Array([1.0, 0.95, 0.8, 0.85, 0.85, 0.8]);
const TAILLIGHT_COLOURS = new Float32Array([0.8, 0.06, 0.04, 0.55, 0.03, 0.02]);
/** Chip colours: [paint A, paint B, black plastic, dark plastic, metal]; paints set per hit. */
const chipColours = new Float32Array([0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.03, 0.03, 0.035, 0.09, 0.09, 0.1, 0.42, 0.43, 0.45]);

/** Per-vehicle effect state (created when a vehicle is first seen, reused after). */
class CarFx {
  frame = 0;
  vx = 0;
  vy = 0;
  vz = 0;
  /** Local ground plane from the wheel contacts (valid once ≥ 3 wheels touched). */
  readonly plane: Plane = { ga: 0, gb: 0, gc: 0 };
  planeOk = false;
  engine = 1;
  radiator = 1;
  smokeAcc = 0;
  steamAcc = 0;
  readonly tyreAcc = new Float32Array(4);
  readonly tracks = new Float32Array(4 * TRACK);
  /** Head / tail lights already burst (once per car end). */
  headBroken = false;
  tailBroken = false;
  paintR = 0.5;
  paintG = 0.5;
  paintB = 0.5;
}

const tmpUp = new THREE.Vector3();
const tmpFwd = new THREE.Vector3();
const tmpRight = new THREE.Vector3();
const tmpV = new THREE.Vector3();
const tmpSite = new THREE.Vector3();
const fallbackPlane: Plane = { ga: 0, gb: 0, gc: 0 };
// Reusable emitter arguments (no allocation per emit; see pool.ts on boxing).
const skidArgs = new SkidArgs();
const tyrePuff = new PuffArgs().look(0.82, 0.82, 0.84, 0.25, 1.25, 1.8, 0.26, 0.35);
const enginePuff = new PuffArgs();
const steamPuff = new PuffArgs().look(0.95, 0.96, 0.97, 0.14, 0.75, 1.0, 0.45, 1.8);
const sparkArgs = new SparkArgs();
const bitArgs = new BitArgs();

/** Counters for automation and the perf doc. */
export interface FxStats {
  quality: FxLevel;
  sparks: number;
  smoke: number;
  shards: number;
  chips: number;
  skidSegments: number;
  /** Live counts. */
  alive: { sparks: number; smoke: number; shards: number; chips: number; skids: number };
  /** Draw calls the effects add this frame (visible meshes). */
  drawCalls: number;
}

export class CrashFx {
  /** Add this to the scene's Z-up world group. */
  readonly group = new THREE.Group();
  readonly sparks: Sparks;
  readonly smoke: Smoke;
  readonly shards: Bits;
  readonly chips: Bits;
  readonly skids: Skids;
  private level: FxLevel;
  private preset: FxPreset;
  private readonly cars = new Map<number, CarFx>();
  private readonly lastHit = new Map<number, number>();
  private readonly rng = new FxRng(0x5eed);
  private time = 0;
  private frameNo = 0;

  constructor(level: FxLevel = "high") {
    this.sparks = new Sparks(MAX.sparks);
    this.smoke = new Smoke(MAX.smoke);
    this.shards = new Bits(
      {
        name: "fx-glass",
        geometry: shardGeometry(),
        material: new THREE.MeshStandardMaterial({
          flatShading: true,
          side: THREE.DoubleSide,
          metalness: 0.3,
          roughness: 0.15,
          emissive: 0x16242c,
        }),
        lift: 0.1,
        bounce: 0.2,
        friction: 0.4,
        shrinkS: 1.5,
      },
      MAX.shards,
    );
    this.chips = new Bits(
      {
        name: "fx-debris",
        geometry: chipGeometry(),
        material: new THREE.MeshStandardMaterial({ flatShading: true, roughness: 0.75, metalness: 0.15 }),
        lift: 0.3,
        bounce: 0.3,
        friction: 0.55,
        shrinkS: 2.5,
      },
      MAX.chips,
    );
    this.skids = new Skids(MAX.skidSegments);
    this.group.name = "crash-fx";
    // Skids first (on the road), then bits, sparks and smoke.
    this.group.add(this.skids.mesh, this.chips.pool.mesh, this.shards.pool.mesh, this.sparks.pool.mesh, this.smoke.pool.mesh);
    this.level = level;
    this.preset = FX_PRESETS[level];
    this.setQuality(level, true);
  }

  /** Current effects quality. */
  get quality(): FxLevel {
    return this.level;
  }

  /** Change the effects quality live (settings screen). Lowering it drops the excess. */
  setQuality(level: FxLevel, force = false): void {
    if (!force && level === this.level) return;
    const p = FX_PRESETS[level] ?? FX_PRESETS.med;
    this.level = level;
    this.preset = p;
    this.sparks.pool.clampTo(p.sparks);
    this.smoke.pool.clampTo(p.smoke);
    this.shards.pool.clampTo(p.shards);
    this.chips.pool.clampTo(p.chips);
    this.shards.lifeS = p.shardLifeS;
    this.chips.lifeS = p.chipLifeS;
    if (this.skids.cap !== p.skidSegments) this.skids.setCap(p.skidSegments);
    this.skids.fadeS = p.skidFadeS;
  }

  /** New session: drop every effect and per-car state. */
  reset(): void {
    this.sparks.pool.clear();
    this.smoke.pool.clear();
    this.shards.pool.clear();
    this.chips.pool.clear();
    this.skids.clear();
    this.cars.clear();
    this.lastHit.clear();
  }

  /**
   * One frame: per-car emitters (skids, tyre smoke, engine smoke / steam) from the vehicle
   * states and the views' interpolated poses, then the drained events (contacts, glass,
   * damage), then the particle step. `dt` = 0 while paused. `night` 0..1 (world_info).
   */
  frame(events: Record<string, Record<string, unknown>>[], cur: Float32Array, views: Map<number, VehicleView>, dt: number, night: number): void {
    this.frameNo++;
    this.time += dt;
    this.sparks.night = night;
    if (dt > 0) this.vehicles(cur, views, dt);
    for (let i = 0; i < events.length; i++) this.event(events[i], views);
    this.sparks.update(dt);
    this.smoke.update(dt);
    this.shards.update(dt);
    this.chips.update(dt);
    this.skids.update(this.time);
    // Forget vehicles that left the session (traffic despawns) now and then.
    if (this.frameNo % 300 === 0) for (const [id, c] of this.cars) if (this.frameNo - c.frame > 300) this.cars.delete(id);
  }

  private car(id: number): CarFx {
    let c = this.cars.get(id);
    if (!c) {
      c = new CarFx();
      this.cars.set(id, c);
    }
    return c;
  }

  private vehicles(cur: Float32Array, views: Map<number, VehicleView>, dt: number): void {
    const p = this.preset;
    const rng = this.rng;
    for (let o = 0; o + STATE_STRIDE <= cur.length; o += STATE_STRIDE) {
      const id = cur[o + S.entity];
      const view = views.get(id);
      if (!view) continue;
      const c = this.car(id);
      c.frame = this.frameNo;
      c.vx = cur[o + S.linVel];
      c.vy = cur[o + S.linVel + 1];
      c.vz = cur[o + S.linVel + 2];
      const wx = cur[o + S.angVel];
      const wy = cur[o + S.angVel + 1];
      const wz = cur[o + S.angVel + 2];
      const q = view.root.quaternion;
      const pos = view.root.position;
      const up = tmpUp.set(0, 0, 1).applyQuaternion(q);
      const fwd = tmpFwd.set(0, 1, 0).applyQuaternion(q);
      const right = tmpRight.set(1, 0, 0).applyQuaternion(q);
      const paint = view.panels.paint.color;
      c.paintR = paint.r;
      c.paintG = paint.g;
      c.paintB = paint.b;
      const info = view.info;

      // Wheels: contact patches → ground plane, skid marks, tyre smoke.
      let nContact = 0;
      let mx = 0;
      let my = 0;
      let mz = 0;
      for (let i = 0; i < 4 && i < info.wheels.length; i++) {
        const wb = o + S.wheelBase + i * S.wheelStride;
        const wi = info.wheels[i];
        const t = i * TRACK;
        const touching = cur[wb + SW.contact] > 0.5 && cur[wb + SW.detached] < 0.5;
        if (!touching) {
          c.tracks[t] = 0;
          continue;
        }
        const lz = wi.position_m[2] + (cur[wb + SW.compression] - wi.travel_m * 0.5) - wi.radius_m;
        const lx = wi.position_m[0];
        const ly = wi.position_m[1];
        const cx = pos.x + right.x * lx + fwd.x * ly + up.x * lz;
        const cy = pos.y + right.y * lx + fwd.y * ly + up.y * lz;
        const cz = pos.z + right.z * lx + fwd.z * ly + up.z * lz;
        nContact++;
        mx += cx;
        my += cy;
        mz += cz;
        // Contact patch velocity: v + ω × r.
        const rx = cx - pos.x;
        const ry = cy - pos.y;
        const rz = cz - pos.z;
        const pvx = c.vx + wy * rz - wz * ry;
        const pvy = c.vy + wz * rx - wx * rz;
        const pvz = c.vz + wx * ry - wy * rx;
        // Wheel heading: chassis forward turned by the steer angle about the chassis up.
        const st = cur[wb + SW.steer];
        const cs = Math.cos(st);
        const sn = Math.sin(st);
        const hx = fwd.x * cs - right.x * sn;
        const hy = fwd.y * cs - right.y * sn;
        const hz = fwd.z * cs - right.z * sn;
        const sx = right.x * cs + fwd.x * sn;
        const sy = right.y * cs + fwd.y * sn;
        const sz = right.z * cs + fwd.z * sn;
        const vLong = pvx * hx + pvy * hy + pvz * hz;
        const vLat = pvx * sx + pvy * sy + pvz * sz;
        // Slip speed of the tread against the road (sign-agnostic: burnouts and lock-ups).
        const dLong = Math.abs(cur[wb + SW.omega]) * wi.radius_m - Math.abs(vLong);
        const slip = Math.sqrt(dLong * dLong + vLat * vLat);
        const loaded = cur[wb + SW.fz] > 300;
        const strength = loaded && slip > SKID_SLIP_MPS ? Math.min(Math.max((slip - SKID_SLIP_MPS) / 8, 0.25), 1) : 0;
        const sk = skidArgs;
        sk.cx = cx + up.x * 0.015;
        sk.cy = cy + up.y * 0.015;
        sk.cz = cz + up.z * 0.015;
        sk.sx = sx;
        sk.sy = sy;
        sk.sz = sz;
        sk.halfW = wi.width_m * 0.5;
        sk.strength = strength;
        sk.time = this.time;
        this.skids.track(c.tracks, t, sk);
        if (loaded && slip > TYRE_SMOKE_SLIP_MPS && p.tyreSmoke > 0) {
          c.tyreAcc[i] += Math.min((slip - TYRE_SMOKE_SLIP_MPS) * 1.2, 14) * p.tyreSmoke * dt;
          while (c.tyreAcc[i] >= 1) {
            c.tyreAcc[i] -= 1;
            const tp = tyrePuff;
            tp.x = cx + up.x * 0.25;
            tp.y = cy + up.y * 0.25;
            tp.z = cz + up.z * 0.25;
            tp.vx = c.vx * 0.25;
            tp.vy = c.vy * 0.25;
            tp.vz = 0.2;
            this.smoke.puff(rng, tp);
          }
        } else {
          c.tyreAcc[i] = 0;
        }
      }
      if (nContact >= 3 && up.z > 0.6) {
        mx /= nContact;
        my /= nContact;
        mz /= nContact;
        c.plane.ga = mz + (up.x * mx + up.y * my) / up.z;
        c.plane.gb = -up.x / up.z;
        c.plane.gc = -up.y / up.z;
        c.planeOk = true;
      }

      // Damaged engine (grey → black) and radiator (white steam), from the hood.
      const w = info.body_size_m[0];
      const l = info.body_size_m[1];
      const h = info.body_size_m[2];
      if (c.engine < ENGINE_SMOKE_HEALTH) {
        const sev = Math.min((ENGINE_SMOKE_HEALTH - c.engine) / ENGINE_SMOKE_HEALTH, 1);
        c.smokeAcc += (3 + 14 * sev) * p.rate * dt;
        const grey = 0.5 - 0.43 * sev;
        while (c.smokeAcc >= 1) {
          c.smokeAcc -= 1;
          this.site(view, 0, l * 0.3, h * 0.78, tmpSite);
          const ep = enginePuff;
          ep.x = tmpSite.x;
          ep.y = tmpSite.y;
          ep.z = tmpSite.z;
          ep.vx = c.vx * 0.6;
          ep.vy = c.vy * 0.6;
          ep.vz = c.vz * 0.6 + 0.6;
          ep.r = grey;
          ep.g = grey;
          ep.b = grey * 1.04;
          ep.s0 = 0.18;
          ep.s1 = 0.9 + 0.9 * sev;
          ep.life = 1.6 + 1.2 * sev;
          ep.alpha = 0.5 + 0.35 * sev;
          ep.rise = 1.2;
          this.smoke.puff(rng, ep);
        }
      } else c.smokeAcc = 0;
      if (c.radiator < RADIATOR_STEAM_HEALTH) {
        const sev = Math.min((RADIATOR_STEAM_HEALTH - c.radiator) / RADIATOR_STEAM_HEALTH, 1);
        c.steamAcc += (2 + 10 * sev) * p.rate * dt;
        while (c.steamAcc >= 1) {
          c.steamAcc -= 1;
          this.site(view, rng.signed() * w * 0.2, l * 0.42, h * 0.6, tmpSite);
          const sp = steamPuff;
          sp.x = tmpSite.x;
          sp.y = tmpSite.y;
          sp.z = tmpSite.z;
          sp.vx = c.vx * 0.6;
          sp.vy = c.vy * 0.6;
          sp.vz = c.vz * 0.6 + 0.8;
          this.smoke.puff(rng, sp);
        }
      } else c.steamAcc = 0;
    }
  }

  /**
   * Live component health of one car from its telemetry (the HUD's damage summary; the frame
   * loop fetches it for the focus car). Overrides the event-tracked values, so a repaired car
   * stops smoking. `components` = `VehicleInfo.components` (telemetry order).
   */
  componentHealth(entity: number, tel: Float32Array, componentBase: number, components: string[]): void {
    if (tel.length === 0) return;
    const c = this.car(entity);
    const e = components.indexOf("Engine");
    const r = components.indexOf("Radiator");
    if (e >= 0 && componentBase + e < tel.length) c.engine = tel[componentBase + e];
    if (r >= 0 && componentBase + r < tel.length) c.radiator = tel[componentBase + r];
  }

  /** Glass shards and debris chips below their ground plane by more than 1 cm (automation:
   * should be 0). */
  belowGround(): number {
    let below = 0;
    for (const bits of [this.shards, this.chips]) {
      const d = bits.pool.d;
      for (let i = 0; i < bits.pool.n; i++) {
        const o = i * STRIDE;
        if (d[o + P.pz] < d[o + P.ga] + d[o + P.gb] * d[o + P.px] + d[o + P.gc] * d[o + P.py] - 0.01) below++;
      }
    }
    return below;
  }

  /** Effect counters (automation, perf doc). */
  stats(): FxStats {
    const meshes = [this.sparks.pool.mesh, this.smoke.pool.mesh, this.shards.pool.mesh, this.chips.pool.mesh, this.skids.mesh];
    return {
      quality: this.level,
      sparks: this.sparks.pool.spawned,
      smoke: this.smoke.pool.spawned,
      shards: this.shards.pool.spawned,
      chips: this.chips.pool.spawned,
      skidSegments: this.skids.laid,
      alive: {
        sparks: this.sparks.pool.n,
        smoke: this.smoke.pool.n,
        shards: this.shards.pool.n,
        chips: this.chips.pool.n,
        skids: this.skids.segments,
      },
      drawCalls: meshes.filter((m) => m.visible).length,
    };
  }

  /** Chassis-space point (x, y, z) of `view` in sim world space. */
  private site(view: VehicleView, x: number, y: number, z: number, out: THREE.Vector3): THREE.Vector3 {
    return out.set(x, y, z).applyQuaternion(view.root.quaternion).add(view.root.position);
  }

  /** Ground plane for an effect near `a` / `b` at height `z` (contact point). */
  private planeFor(a: number, b: number, z: number, nz: number): Plane {
    const ca = this.cars.get(a);
    if (ca?.planeOk) return ca.plane;
    const cb = b === WORLD ? undefined : this.cars.get(b);
    if (cb?.planeOk) return cb.plane;
    // No wheel contact known: a near-vertical normal means the contact is the ground itself.
    fallbackPlane.ga = Math.abs(nz) > 0.7 ? z : z - 0.4;
    fallbackPlane.gb = 0;
    fallbackPlane.gc = 0;
    return fallbackPlane;
  }

  private event(ev: Record<string, Record<string, unknown>>, views: Map<number, VehicleView>): void {
    const c = ev.Contact;
    if (c) return this.contact(c);
    const g = ev.GlassShattered;
    if (g) return this.glass(Number(g.vehicle), String(g.region), views);
    const d = ev.ComponentDamaged ?? ev.ComponentFailed;
    if (d) {
      const car = this.car(Number(d.vehicle));
      const h = ev.ComponentFailed ? 0 : Number(d.health);
      if (d.component === "Engine") car.engine = h;
      else if (d.component === "Radiator") car.radiator = h;
      return;
    }
    const rd = ev.RegionDamaged;
    if (rd) {
      if (Number(rd.severity) < LIGHT_BREAK_SEVERITY) return;
      const id = Number(rd.vehicle);
      const car = this.car(id);
      const r = rd.region;
      if ((r === "FrameFront" || r === "FrontBumper") && !car.headBroken) {
        car.headBroken = true;
        this.lights(id, true, views);
      } else if ((r === "FrameRear" || r === "RearBumper") && !car.tailBroken) {
        car.tailBroken = true;
        this.lights(id, false, views);
      }
      return;
    }
    const t = ev.TotalLoss;
    if (t) {
      const car = this.car(Number(t.vehicle));
      car.engine = Math.min(car.engine, 0.15);
      car.radiator = Math.min(car.radiator, 0.2);
      return;
    }
    const pd = ev.PartDetached;
    if (pd) {
      const view = views.get(Number(pd.vehicle));
      const car = this.cars.get(Number(pd.vehicle));
      if (!view || !car) return;
      // The panel itself flies off as panel debris (render/panels.ts); add a few chips.
      const pos = view.root.position;
      this.chipPaints(car, car);
      const n = this.rng.round(8 * this.preset.rate);
      const plane = this.planeFor(Number(pd.vehicle), WORLD, pos.z, 1);
      const b = bitArgs;
      b.x = pos.x;
      b.y = pos.y;
      b.z = pos.z + 0.5;
      b.vx = car.vx * 0.4;
      b.vy = car.vy * 0.4;
      b.vz = 0;
      b.nx = 0;
      b.ny = 0;
      b.nz = 1;
      b.burst = 2.5;
      b.smin = 0.05;
      b.smax = 0.13;
      b.colours = chipColours;
      b.plane = plane;
      this.chips.throw(this.rng, n, b);
    }
  }

  private chipPaints(a: CarFx | undefined, b: CarFx | undefined): void {
    const pa = a ?? b;
    const pb = b ?? a;
    chipColours[0] = pa?.paintR ?? 0.5;
    chipColours[1] = pa?.paintG ?? 0.5;
    chipColours[2] = pa?.paintB ?? 0.5;
    chipColours[3] = pb?.paintR ?? 0.5;
    chipColours[4] = pb?.paintG ?? 0.5;
    chipColours[5] = pb?.paintB ?? 0.5;
  }

  private contact(c: Record<string, unknown>): void {
    const a = c.a as number;
    const b = c.b as number;
    const pt = c.point as number[];
    const n = c.normal as number[];
    const imp = c.impulse_ns as number;
    const closing = c.rel_speed as number;
    const ca = this.cars.get(a);
    const cb = b === WORLD ? undefined : this.cars.get(b);
    // Relative velocity of a against b, minus its normal part = the scrape (sliding) velocity.
    let rvx = (ca?.vx ?? 0) - (cb?.vx ?? 0);
    let rvy = (ca?.vy ?? 0) - (cb?.vy ?? 0);
    let rvz = (ca?.vz ?? 0) - (cb?.vz ?? 0);
    const vn = rvx * n[0] + rvy * n[1] + rvz * n[2];
    rvx -= vn * n[0];
    rvy -= vn * n[1];
    rvz -= vn * n[2];
    const slide = Math.sqrt(rvx * rvx + rvy * rvy + rvz * rvz);
    const p = this.preset;
    const rng = this.rng;
    const plane = this.planeFor(a, b, pt[2], n[2]);
    // Sparks fly off the surface of b, back toward a: along -normal (wall, road). Between two
    // cars that would be into a's body, so they kick upward instead.
    const kx = cb ? 0 : -n[0];
    const ky = cb ? 0 : -n[1];
    const kz = cb ? 1 : -n[2];
    if (slide > SCRAPE_MIN_MPS && imp > SCRAPE_MIN_NS) {
      // Per tick (240 Hz): a sustained scrape throws a steady stream.
      const count = rng.round(p.rate * 2.2 * Math.min(slide / 12, 1.5) * Math.min(Math.max(imp / 400, 0.3), 2.5));
      if (count > 0) {
        const sa = sparkArgs;
        sa.x = pt[0];
        sa.y = pt[1];
        sa.z = pt[2];
        sa.dx = rvx / slide;
        sa.dy = rvy / slide;
        sa.dz = rvz / slide;
        sa.speed = slide * 0.8;
        sa.spread = 0.35;
        sa.nx = kx;
        sa.ny = ky;
        sa.nz = kz;
        sa.lifeScale = 1;
        sa.plane = plane;
        this.sparks.emit(rng, count, sa);
      }
    }
    if (closing > HIT_MIN_MPS && imp > HIT_MIN_NS) {
      const key = a * 65536 + (b === WORLD ? 65535 : b);
      const last = this.lastHit.get(key);
      if (last !== undefined && this.time - last < HIT_DEBOUNCE_S) return;
      if (this.lastHit.size > 512) this.lastHit.clear();
      this.lastHit.set(key, this.time);
      const hard = Math.min((closing / 10) * (imp / 1500), 3);
      // Spark burst: a wide spray off the impact.
      const burst = rng.round(p.rate * Math.min(10 + 30 * hard, 80));
      const dx = slide > 0.5 ? rvx / slide : 0;
      const dy = slide > 0.5 ? rvy / slide : 0;
      const dz = slide > 0.5 ? rvz / slide : 0;
      const sa = sparkArgs;
      sa.x = pt[0];
      sa.y = pt[1];
      sa.z = pt[2];
      sa.dx = dx * 0.5;
      sa.dy = dy * 0.5;
      sa.dz = dz * 0.5;
      sa.speed = 3 + closing * 0.22;
      sa.spread = 0.9;
      sa.nx = kx * 2;
      sa.ny = ky * 2;
      sa.nz = kz * 2;
      sa.lifeScale = 1.7;
      sa.plane = plane;
      this.sparks.emit(rng, burst, sa);
      // Debris chips: paint of both cars, black plastic, metal.
      this.chipPaints(ca, cb);
      const nChips = rng.round(p.rate * Math.min(3 + 9 * hard, 30));
      const bvx = ((ca?.vx ?? 0) + (cb?.vx ?? 0)) * 0.3;
      const bvy = ((ca?.vy ?? 0) + (cb?.vy ?? 0)) * 0.3;
      const ba = bitArgs;
      ba.x = pt[0];
      ba.y = pt[1];
      ba.z = pt[2];
      ba.vx = bvx;
      ba.vy = bvy;
      ba.vz = 0;
      ba.nx = kx;
      ba.ny = ky;
      ba.nz = Math.max(kz, 0.2);
      ba.burst = 2 + closing * 0.12;
      ba.smin = 0.05;
      ba.smax = 0.15;
      ba.colours = chipColours;
      ba.plane = plane;
      this.chips.throw(rng, nChips, ba);
    }
  }

  private glass(vehicle: number, region: string, views: Map<number, VehicleView>): void {
    const view = views.get(vehicle);
    if (!view) return;
    if (!(view.panels.ready && view.panels.materialCentre(region, "glass", tmpSite))) {
      const f = GLASS_SITE[region];
      if (!f) return;
      tmpSite.set(f[0] * view.info.body_size_m[0], f[1] * view.info.body_size_m[1], f[2] * view.info.body_size_m[2]);
    }
    this.burstShards(view, vehicle, GLASS_COLOURS, 36);
  }

  /** Head- or taillights broke (front / rear crushed): a few coloured lens shards. */
  private lights(vehicle: number, front: boolean, views: Map<number, VehicleView>): void {
    const view = views.get(vehicle);
    if (!view) return;
    const ok = view.panels.ready && view.panels.materialCentre(null, front ? "headlight" : "taillight", tmpSite);
    if (!ok) tmpSite.set(0, (front ? 0.48 : -0.48) * view.info.body_size_m[1], 0.45 * view.info.body_size_m[2]);
    this.burstShards(view, vehicle, front ? HEADLIGHT_COLOURS : TAILLIGHT_COLOURS, 14);
  }

  /** Shards from the chassis-space site in `tmpSite`, thrown outward from the car's centre. */
  private burstShards(view: VehicleView, vehicle: number, colours: Float32Array, count: number): void {
    const car = this.car(vehicle);
    const plane = car.planeOk ? car.plane : this.planeFor(vehicle, WORLD, view.root.position.z, 1);
    const p = this.preset;
    const len = Math.sqrt(tmpSite.x * tmpSite.x + tmpSite.y * tmpSite.y) || 1;
    tmpV.set(tmpSite.x / len, tmpSite.y / len, 0.4).applyQuaternion(view.root.quaternion);
    this.site(view, tmpSite.x, tmpSite.y, tmpSite.z, tmpSite);
    const n = Math.max(this.rng.round(count * p.rate), 4);
    const b = bitArgs;
    b.x = tmpSite.x;
    b.y = tmpSite.y;
    b.z = tmpSite.z;
    b.vx = car.vx * 0.3;
    b.vy = car.vy * 0.3;
    b.vz = car.vz * 0.3;
    b.nx = tmpV.x;
    b.ny = tmpV.y;
    b.nz = tmpV.z;
    b.burst = 2.8;
    b.smin = 0.045;
    b.smax = 0.12;
    b.colours = colours;
    b.plane = plane;
    this.shards.throw(this.rng, n, b);
    // A few glints at the break (bright, very short).
    const a = sparkArgs;
    a.x = tmpSite.x;
    a.y = tmpSite.y;
    a.z = tmpSite.z;
    a.dx = 0;
    a.dy = 0;
    a.dz = 1;
    a.speed = 2;
    a.spread = 1;
    a.nx = 0;
    a.ny = 0;
    a.nz = 0.5;
    a.lifeScale = 1;
    a.plane = plane;
    this.sparks.emit(this.rng, this.rng.round(6 * p.rate), a);
  }
}
