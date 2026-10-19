// Cameras for the browser testbed.
//
// Angles (cycle with C, pick with 1–8): near chase (default), far chase, hood, bumper,
// cockpit (dash placeholder), side tracking, TV / cinematic (cuts between roadside spots,
// frames the nearest pursuer), and the orbit / free debug camera. Hold V (or gamepad R3) to
// look back. Drag the mouse or use the right stick to look around; the view springs back.
//
// Chase follow: the camera sits at a car-relative offset built from a *smoothed heading*, and
// only that offset is sprung — never the camera's world position. A world-space position spring
// trails the car by roughly (2/ω)·speed, which is what made the old chase cam drift ~10–16 m
// back at city-chase speeds. Speed adds at most `pullBack` metres.
//
// All targets are Three.js objects: the vehicle root (sim chassis space: +X right, +Y
// forward, +Z up) inside the Z-up `world` group. Three world space is +Y up.
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { store } from "./settings";

/** Every camera angle. `cinematic` keeps its old id (URL `?cam=`, replay default). */
export type CameraMode = "chase" | "chaseFar" | "hood" | "bumper" | "cockpit" | "side" | "cinematic" | "orbit";

/** Camera angles in cycle / number-key order, with the label shown in the HUD and help. */
export const CAMERAS: { mode: CameraMode; label: string }[] = [
  { mode: "chase", label: "near chase" },
  { mode: "chaseFar", label: "far chase" },
  { mode: "hood", label: "hood" },
  { mode: "bumper", label: "bumper" },
  { mode: "cockpit", label: "cockpit" },
  { mode: "side", label: "side tracking" },
  { mode: "cinematic", label: "TV / cinematic" },
  { mode: "orbit", label: "orbit / free (debug)" },
];
export const CAMERA_MODES: CameraMode[] = CAMERAS.map((c) => c.mode);

/** Key help line for the F1 overlay. */
export const CAMERA_HELP =
  `C next camera · V (hold) look back · drag mouse / right stick: look around (springs back)<br>` +
  CAMERAS.map((c, i) => `${i + 1} ${c.label}`).join(" · ");

/** Human label for a mode (HUD readout). */
export const cameraLabel = (m: CameraMode): string => CAMERAS.find((c) => c.mode === m)?.label ?? m;

const STORE_KEY = "wp.testbed.camera.v1";

/** Chase-camera tuning, metres / degrees. Distances are from the car's centre. */
interface ChaseTuning {
  dist: number;
  height: number;
  /** Extra distance at high speed (reached at `PULL_BACK_SPEED`). */
  pullBack: number;
  fov: number;
  /** Extra FOV at high speed — gives a sense of speed without moving the camera away. */
  fovSpeed: number;
}
const NEAR_CHASE: ChaseTuning = { dist: 5.6, height: 1.8, pullBack: 1.2, fov: 62, fovSpeed: 8 };
const FAR_CHASE: ChaseTuning = { dist: 8.5, height: 2.9, pullBack: 1.5, fov: 60, fovSpeed: 6 };
const PULL_BACK_SPEED = 50; // m/s
const UP = new THREE.Vector3(0, 1, 0);

/** Exact critically damped spring step (error `x`, velocity `v`) over `dt`: stable for any
 * frame time, unlike an Euler step, which blows up once ω·dt approaches 1 (slow frames). */
function critStep(x: number, v: number, omega: number, dt: number): [number, number] {
  const e = Math.exp(-omega * dt);
  const c = v + omega * x;
  return [(x + c * dt) * e, (v - omega * c * dt) * e];
}

/** Critically damped spring toward a target (per component). */
class Spring3 {
  readonly pos = new THREE.Vector3();
  private readonly vel = new THREE.Vector3();
  private init = false;
  constructor(private readonly omega: number) {}
  step(target: THREE.Vector3, dt: number): THREE.Vector3 {
    if (!this.init) {
      this.pos.copy(target);
      this.init = true;
      return this.pos;
    }
    for (const k of ["x", "y", "z"] as const) {
      const [x, v] = critStep(this.pos[k] - target[k], this.vel[k], this.omega, dt);
      this.pos[k] = target[k] + x;
      this.vel[k] = v;
    }
    return this.pos;
  }
  reset(): void {
    this.init = false;
    this.vel.set(0, 0, 0);
  }
}

/** Critically damped spring for an angle (radians), wrapping the error to ±π. */
class AngleSpring {
  value = 0;
  private vel = 0;
  private init = false;
  step(target: number, omega: number, dt: number): number {
    if (!this.init) {
      this.value = target;
      this.init = true;
      return this.value;
    }
    const err = Math.atan2(Math.sin(this.value - target), Math.cos(this.value - target));
    const [x, v] = critStep(err, this.vel, omega, dt);
    this.value = target + x;
    this.vel = v;
    return this.value;
  }
  reset(): void {
    this.init = false;
    this.vel = 0;
  }
}

/** Small deterministic RNG for shot choice (render-side only; keeps screenshots repeatable). */
class ShotRng {
  constructor(private s = 0x2545f491) {}
  next(): number {
    this.s = (Math.imul(this.s, 1664525) + 1013904223) >>> 0;
    return this.s / 4294967296;
  }
}

const smooth01 = (x: number) => {
  const t = Math.min(Math.max(x, 0), 1);
  return t * t * (3 - 2 * t);
};
const expBlend = (rate: number, dt: number) => 1 - Math.exp(-rate * dt);
/** Heading (radians) of a horizontal Three-world direction; 0 = +Z. */
const headingOf = (v: THREE.Vector3) => Math.atan2(v.x, v.z);
const dirOf = (heading: number) => new THREE.Vector3(Math.sin(heading), 0, Math.cos(heading));

/** One TV-camera shot. */
interface TvShot {
  kind: "roadside" | "heli" | "follow";
  spot: THREE.Vector3;
  timer: number;
}

export class CameraRig {
  readonly camera: THREE.PerspectiveCamera;
  mode: CameraMode = "chase";
  /** The angle the viewer last picked (persisted). Presets return to it. */
  preferred: CameraMode = "chase";
  /** Set every frame by the app: look-back key held. */
  lookBack = false;
  private readonly orbit: OrbitControls;
  // Chase / side follow state.
  private readonly heading = new AngleSpring();
  private readonly offset = new Spring3(9);
  private readonly lookOffset = new Spring3(10);
  private readonly carY = new Spring3(10);
  private readonly lastCar = new THREE.Vector3();
  private readonly vel = new THREE.Vector3();
  private haveLast = false;
  private wasLookBack = false;
  // Look-around (mouse drag / right stick), radians; springs back when released.
  private lookYaw = 0;
  private lookPitch = 0;
  private dragging = false;
  private releaseT = 0;
  // Cockpit: own car hidden on a layer the camera does not render, plus a dash placeholder.
  private hiddenRoot: THREE.Object3D | null = null;
  private dash: THREE.Group | null = null;
  // TV camera.
  private tv: TvShot | null = null;
  private tvCount = 0;
  private readonly rng = new ShotRng();
  private fovNow = 60;
  // Gamepad edges (View/Back = next camera, R3 = look back).
  private padPrev: boolean[] = [];
  /** Called when the gamepad asks for the next camera. */
  onPadNext: (() => void) | null = null;

  constructor(dom: HTMLElement) {
    this.camera = new THREE.PerspectiveCamera(60, window.innerWidth / window.innerHeight, 0.1, 3000);
    this.camera.position.set(0, 6, -12);
    this.orbit = new OrbitControls(this.camera, dom);
    this.orbit.enabled = false;
    this.orbit.enableDamping = true;
    window.addEventListener("resize", () => {
      this.camera.aspect = window.innerWidth / window.innerHeight;
      this.camera.updateProjectionMatrix();
    });
    const saved = store.get(STORE_KEY) as CameraMode | null;
    if (saved && CAMERA_MODES.includes(saved)) this.preferred = this.mode = saved;
    this.installLook(dom);
  }

  /** Viewer picked an angle (keys / pad): switch and remember it. */
  choose(mode: CameraMode, target?: THREE.Object3D): void {
    this.preferred = mode;
    store.set(STORE_KEY, mode);
    this.setMode(mode, target);
  }

  /** Viewer cycles to the next (or previous) angle. */
  cycle(target?: THREE.Object3D, step = 1): void {
    const i = CAMERA_MODES.indexOf(this.mode);
    const n = CAMERA_MODES.length;
    this.choose(CAMERA_MODES[(i + step + n) % n], target);
  }

  /** Switch camera without remembering it (presets, replays, benches, automation).
   * `orbitOffset` (vehicle-local metres) places a fresh orbit camera. */
  setMode(mode: CameraMode, target?: THREE.Object3D, orbitOffset: [number, number, number] = [-7, -9, 6]): void {
    this.mode = mode;
    this.orbit.enabled = mode === "orbit";
    if (mode === "orbit" && target) {
      target.updateWorldMatrix(true, false);
      const p = target.getWorldPosition(new THREE.Vector3());
      this.orbit.target.copy(p);
      this.camera.position.copy(target.localToWorld(new THREE.Vector3(...orbitOffset)));
      this.camera.lookAt(p);
    }
    if (mode !== "cockpit") this.showOwnCar();
    this.resetFollow();
    this.tv = null;
  }

  private resetFollow(): void {
    this.heading.reset();
    this.offset.reset();
    this.lookOffset.reset();
    this.carY.reset();
    this.haveLast = false;
    this.vel.set(0, 0, 0);
  }

  /** `target` is the vehicle root; `size` its body size [w, l, h] (sim metres); `speed` m/s.
   * `pursuer` (optional) is the nearest police car, framed by the TV camera. */
  update(
    target: THREE.Object3D | null,
    size: [number, number, number],
    speed: number,
    dt: number,
    pursuer: THREE.Object3D | null = null,
  ): void {
    this.pollPad(dt);
    if (!target) {
      this.orbit.update();
      return;
    }
    if (this.hiddenRoot && this.hiddenRoot !== target) this.showOwnCar();
    target.updateWorldMatrix(true, false);
    const car = target.getWorldPosition(new THREE.Vector3());
    // Teleports (reset, focus change, replay seek) snap instead of swinging across the map.
    if (this.haveLast && car.distanceTo(this.lastCar) > 15) this.resetFollow();
    if (this.haveLast && dt > 0) {
      const v = car.clone().sub(this.lastCar).divideScalar(dt);
      this.vel.lerp(v, expBlend(8, dt));
    }
    this.lastCar.copy(car);
    this.haveLast = true;

    const lookBack = this.lookBack && this.mode !== "orbit" && this.mode !== "cinematic";
    if (lookBack !== this.wasLookBack) {
      // Look-back is a cut, not a swing.
      this.offset.reset();
      this.lookOffset.reset();
      this.wasLookBack = lookBack;
    }
    this.springBackLook(dt);

    const local = (x: number, y: number, z: number) => target.localToWorld(new THREE.Vector3(x, y, z));
    const fwd = local(0, 1, 0).sub(car).setY(0);
    if (fwd.lengthSq() < 1e-6) fwd.set(0, 0, 1);
    fwd.normalize();
    const v = Math.abs(speed);
    let fov = 60;
    switch (this.mode) {
      case "chase":
      case "chaseFar":
        fov = this.chase(car, fwd, size, v, dt, this.mode === "chase" ? NEAR_CHASE : FAR_CHASE, lookBack);
        break;
      case "side":
        fov = this.side(car, fwd, v, dt, lookBack, size);
        break;
      case "hood":
      case "bumper":
      case "cockpit":
        fov = this.mounted(target, local, size, lookBack);
        break;
      case "orbit": {
        // Follow the car with the orbit pivot; the user controls angle and distance.
        const delta = car.clone().sub(this.orbit.target);
        this.orbit.target.add(delta);
        this.camera.position.add(delta);
        this.orbit.update();
        fov = 60;
        break;
      }
      case "cinematic":
        fov = this.cinematic(car, fwd, dt, pursuer);
        break;
    }
    // Mounted cameras change FOV instantly (a cut); follow cameras ease.
    const instant = this.mode === "hood" || this.mode === "bumper" || this.mode === "cockpit" || this.mode === "orbit";
    this.fovNow = instant ? fov : this.fovNow + (fov - this.fovNow) * expBlend(3, dt);
    if (Math.abs(this.camera.fov - this.fovNow) > 0.01) {
      this.camera.fov = this.fovNow;
      this.camera.updateProjectionMatrix();
    }
  }

  // ----- chase ------------------------------------------------------------------------------

  /** Smoothed heading that follows the body, and the direction of travel when sliding. */
  private followHeading(fwd: THREE.Vector3, v: number, dt: number): number {
    let dir = fwd;
    const hv = this.vel.clone().setY(0);
    // Blend toward the travel direction in a slide (not when reversing).
    if (v > 4 && hv.lengthSq() > 1 && hv.dot(fwd) > 0) dir = fwd.clone().lerp(hv.normalize(), 0.35).normalize();
    // Stiffer at speed so the car stays framed; soft at low speed so it can swing a little.
    const omega = 3.5 + Math.min(v, 40) * 0.06;
    return this.heading.step(headingOf(dir), omega, dt);
  }

  private chase(
    car: THREE.Vector3,
    fwd: THREE.Vector3,
    size: [number, number, number],
    v: number,
    dt: number,
    t: ChaseTuning,
    lookBack: boolean,
  ): number {
    const k = smooth01(v / PULL_BACK_SPEED);
    const dist = Math.max(t.dist, size[1] / 2 + 2.6) + t.pullBack * k;
    const h = Math.max(t.height, size[2] + 0.35);
    const back = dirOf(this.followHeading(fwd, v, dt) + this.lookYaw + (lookBack ? Math.PI : 0)).negate();
    const desired = back.multiplyScalar(dist).addScaledVector(UP, h + this.lookPitch * dist);
    // Damp vertical bounce of the body, not its horizontal motion.
    const base = new THREE.Vector3(car.x, this.carY.step(car, dt).y, car.z);
    this.camera.position.copy(base).add(this.offset.step(desired, dt));
    // Look ahead of the nose along the *actual* body heading: in a turn the view leads into it.
    const ahead = lookBack ? -3 : 2.5 + Math.min(v * 0.08, 4);
    const look = fwd.clone().multiplyScalar(ahead).addScaledVector(UP, 0.9);
    this.camera.lookAt(base.clone().add(this.lookOffset.step(look, dt)));
    return t.fov + t.fovSpeed * k;
  }

  private side(car: THREE.Vector3, fwd: THREE.Vector3, v: number, dt: number, lookBack: boolean, size: [number, number, number]): number {
    if (lookBack) return this.chase(car, fwd, size, v, dt, NEAR_CHASE, true);
    const hd = this.followHeading(fwd, v, dt) + this.lookYaw;
    const f = dirOf(hd);
    const right = new THREE.Vector3().crossVectors(f, UP).normalize();
    const desired = right.multiplyScalar(7.5).addScaledVector(f, -0.5).addScaledVector(UP, 1.3 + this.lookPitch * 7.5);
    const base = new THREE.Vector3(car.x, this.carY.step(car, dt).y, car.z);
    this.camera.position.copy(base).add(this.offset.step(desired, dt));
    const look = f.clone().multiplyScalar(0.5 + Math.min(v * 0.015, 0.8)).addScaledVector(UP, 0.7);
    this.camera.lookAt(base.clone().add(this.lookOffset.step(look, dt)));
    return 52;
  }

  // ----- body-mounted (hood, bumper, cockpit) -----------------------------------------------

  private mounted(
    target: THREE.Object3D,
    local: (x: number, y: number, z: number) => THREE.Vector3,
    size: [number, number, number],
    lookBack: boolean,
  ): number {
    const [w, l, h] = size;
    let eye: THREE.Vector3;
    let fov: number;
    if (lookBack) {
      eye = new THREE.Vector3(0, -l / 2 - 0.05, h * 0.75);
      fov = 70;
    } else if (this.mode === "hood") {
      eye = new THREE.Vector3(0, l * 0.18, h + 0.15);
      fov = 75;
    } else if (this.mode === "bumper") {
      eye = new THREE.Vector3(0, l / 2 + 0.05, 0.45);
      fov = 75;
    } else {
      eye = new THREE.Vector3(-w * 0.2, -l * 0.04, h * 0.8);
      fov = 72;
    }
    const cockpit = this.mode === "cockpit" && !lookBack;
    if (this.mode === "cockpit") this.hideOwnCar(target, size, cockpit);
    // View direction in chassis space, turned by look-around.
    const yaw = this.lookYaw + (lookBack ? Math.PI : 0);
    const dir = new THREE.Vector3(-Math.sin(yaw), Math.cos(yaw), this.mode === "bumper" ? -0.008 : -0.03 + this.lookPitch);
    this.camera.position.copy(local(eye.x, eye.y, eye.z));
    // Keep the horizon tied to the car (roll with the body), like a real mounted camera.
    this.camera.up.copy(local(0, 0, 1).sub(local(0, 0, 0)).normalize());
    this.camera.lookAt(local(eye.x + dir.x * 20, eye.y + dir.y * 20, eye.z + dir.z * 20));
    this.camera.up.copy(UP);
    return fov;
  }

  /** Cockpit: hide the focused car's own geometry and show a simple dash placeholder. */
  private hideOwnCar(target: THREE.Object3D, size: [number, number, number], showDash: boolean): void {
    if (!this.dash || this.hiddenRoot !== target) {
      this.showOwnCar();
      this.dash = buildDash(size);
      target.add(this.dash);
      this.hiddenRoot = target;
    }
    this.dash.visible = showDash;
    // Re-applied every frame: views add meshes lazily (skinned body, detached parts).
    target.traverse((o) => {
      if (o === this.dash || o.parent === this.dash) return;
      if (isDrawable(o)) o.layers.set(1);
    });
  }

  private showOwnCar(): void {
    if (this.hiddenRoot) {
      this.hiddenRoot.traverse((o) => {
        if (isDrawable(o)) o.layers.set(0);
      });
    }
    if (this.dash) {
      this.dash.removeFromParent();
      this.dash.traverse((o) => {
        if (o instanceof THREE.Mesh) {
          o.geometry.dispose();
          (o.material as THREE.Material).dispose();
        }
      });
    }
    this.dash = null;
    this.hiddenRoot = null;
  }

  // ----- TV / cinematic ---------------------------------------------------------------------

  /** Roadside TV camera: plants a camera ahead of the car beside its path and holds it until
   * the car has gone past, then cuts. Every third cut is a helicopter or low follow shot.
   * Frames the pursuer too when one is close. */
  private cinematic(car: THREE.Vector3, fwd: THREE.Vector3, dt: number, pursuer: THREE.Object3D | null): number {
    const hv = this.vel.clone().setY(0);
    const travel = hv.lengthSq() > 4 ? hv.clone().normalize() : fwd;
    const spd = hv.length();
    const shot = this.tv;
    let cut = !shot;
    if (shot) {
      shot.timer -= dt;
      if (shot.kind === "roadside") {
        const rel = car.clone().sub(shot.spot);
        rel.y = 0;
        // Cut once the car is well past the camera, too far away, or the shot is stale.
        cut = rel.dot(travel) > 14 || rel.length() > 90 || shot.timer <= 0;
      } else {
        cut = shot.timer <= 0;
      }
    }
    if (cut) this.tv = this.newTvShot(car, travel, spd);
    const s = this.tv!;

    // Look target: the car, pulled toward a close pursuer so both are in frame.
    const look = car.clone().addScaledVector(UP, 0.8);
    let half = 4.5; // metres of the subject to keep in frame
    if (pursuer) {
      const p = pursuer.getWorldPosition(new THREE.Vector3());
      const sep = p.distanceTo(car);
      if (sep < 45) {
        look.lerp(p.addScaledVector(UP, 0.8), 0.35);
        half = Math.max(half, sep * 0.6);
      }
    }
    if (s.kind === "roadside") {
      this.camera.position.copy(s.spot);
    } else if (s.kind === "heli") {
      const want = car.clone().addScaledVector(travel, -14).addScaledVector(UP, 24);
      this.camera.position.lerp(want, expBlend(1.5, dt));
    } else {
      const right = new THREE.Vector3().crossVectors(travel, UP).normalize();
      const want = car.clone().addScaledVector(travel, -7).addScaledVector(right, 2.5).addScaledVector(UP, 0.8);
      this.camera.position.lerp(want, expBlend(3, dt));
    }
    this.camera.lookAt(look);
    const d = Math.max(this.camera.position.distanceTo(look), 1);
    // Long lens for far roadside shots: keep the subject a readable size.
    return THREE.MathUtils.clamp(THREE.MathUtils.radToDeg(2 * Math.atan(half / d)), 14, 65);
  }

  private newTvShot(car: THREE.Vector3, travel: THREE.Vector3, spd: number): TvShot {
    this.tvCount++;
    const right = new THREE.Vector3().crossVectors(travel, UP).normalize();
    if (this.tvCount % 3 === 0) {
      const kind = this.rng.next() < 0.5 ? "heli" : "follow";
      // Start the moving shots already in place (a cut, not a fly-in).
      const spot =
        kind === "heli"
          ? car.clone().addScaledVector(travel, -14).addScaledVector(UP, 24)
          : car.clone().addScaledVector(travel, -7).addScaledVector(right, 2.5).addScaledVector(UP, 0.8);
      this.camera.position.copy(spot);
      return { kind, spot, timer: 4 };
    }
    const side = this.rng.next() < 0.5 ? -1 : 1;
    const high = this.rng.next() < 0.35;
    // Far enough ahead that the car drives toward the lens for a couple of seconds.
    const ahead = THREE.MathUtils.clamp(spd * 1.6, 18, 55);
    const spot = car
      .clone()
      .addScaledVector(travel, ahead)
      .addScaledVector(right, side * (high ? 9 : 6.5))
      .addScaledVector(UP, high ? 6 : 1.3);
    return { kind: "roadside", spot, timer: 9 };
  }

  // ----- look-around ------------------------------------------------------------------------

  private installLook(dom: HTMLElement): void {
    let lastX = 0;
    let lastY = 0;
    dom.addEventListener("pointerdown", (e) => {
      if (this.mode === "orbit" || e.button !== 0) return;
      this.dragging = true;
      lastX = e.clientX;
      lastY = e.clientY;
    });
    window.addEventListener("pointermove", (e) => {
      if (!this.dragging) return;
      this.lookYaw = THREE.MathUtils.clamp(this.lookYaw - (e.clientX - lastX) * 0.006, -Math.PI, Math.PI);
      this.lookPitch = THREE.MathUtils.clamp(this.lookPitch - (e.clientY - lastY) * 0.003, -0.25, 0.5);
      lastX = e.clientX;
      lastY = e.clientY;
      this.releaseT = 0.5;
    });
    const up = () => (this.dragging = false);
    window.addEventListener("pointerup", up);
    window.addEventListener("blur", up);
  }

  /** After the drag (or stick) is released and a short hold, ease the view back to centre. */
  private springBackLook(dt: number): void {
    if (this.dragging) return;
    if (this.releaseT > 0) {
      this.releaseT -= dt;
      return;
    }
    const k = expBlend(5, dt);
    this.lookYaw -= this.lookYaw * k;
    this.lookPitch -= this.lookPitch * k;
  }

  /** Gamepad: right stick looks around, View/Back cycles, R3 held looks back. */
  private pollPad(dt: number): void {
    const pads = navigator.getGamepads ? navigator.getGamepads() : [];
    const pad = Array.from(pads).find((p) => p && p.connected);
    if (!pad) return;
    const pressed = pad.buttons.map((b) => b.pressed);
    if (pressed[8] && !this.padPrev[8]) this.onPadNext?.();
    this.padPrev = pressed;
    if (pressed[11]) this.lookBack = true;
    const dz = (a: number) => (Math.abs(a) < 0.15 ? 0 : a);
    const x = dz(pad.axes[2] ?? 0);
    const y = dz(pad.axes[3] ?? 0);
    if (x || y) {
      // Stick is absolute: full deflection = look 120° to the side / up-down a bit.
      this.lookYaw += (-x * 2.1 - this.lookYaw) * expBlend(10, dt);
      this.lookPitch += (-y * 0.3 - this.lookPitch) * expBlend(10, dt);
      this.releaseT = 0.15;
    }
  }
}

const isDrawable = (o: THREE.Object3D) =>
  o instanceof THREE.Mesh || o instanceof THREE.Line || o instanceof THREE.Points || o instanceof THREE.Sprite;

/** Placeholder interior in chassis space: dash top, A-pillars, roof header and a wheel. */
function buildDash(size: [number, number, number]): THREE.Group {
  const [w, l, h] = size;
  const g = new THREE.Group();
  g.name = "cockpit-dash";
  const mat = new THREE.MeshStandardMaterial({ color: 0x1c1f24, roughness: 0.9 });
  const trim = new THREE.MeshStandardMaterial({ color: 0x3a3f47, roughness: 0.6 });
  const add = (geo: THREE.BufferGeometry, m: THREE.Material, x: number, y: number, z: number) => {
    const mesh = new THREE.Mesh(geo, m);
    mesh.position.set(x, y, z);
    g.add(mesh);
    return mesh;
  };
  const dashY = l * 0.1;
  add(new THREE.BoxGeometry(w * 0.92, 0.5, 0.18), mat, 0, dashY, h * 0.6);
  // Instrument binnacle in front of the driver.
  add(new THREE.BoxGeometry(0.42, 0.18, 0.1), trim, -w * 0.2, dashY - 0.22, h * 0.7);
  // A-pillars lean back toward the roof.
  for (const sx of [-1, 1]) {
    const p = add(new THREE.BoxGeometry(0.07, 0.07, h * 0.42), mat, sx * w * 0.44, dashY - 0.05, h * 0.84);
    p.rotation.x = -0.6;
  }
  add(new THREE.BoxGeometry(w * 0.92, 0.25, 0.06), mat, 0, dashY - 0.3, h * 1.0);
  const wheel = add(new THREE.TorusGeometry(0.19, 0.025, 8, 28), trim, -w * 0.2, dashY - 0.42, h * 0.62);
  wheel.rotation.x = Math.PI / 2 - 0.45;
  return g;
}
