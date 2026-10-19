// Cameras: chase (spring-damped), hood, bumper, free orbit, replay cinematic.
// All targets are given as Three.js world-space objects (the vehicle root's world matrix).
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";

export type CameraMode = "chase" | "hood" | "bumper" | "orbit" | "cinematic";
export const CAMERA_MODES: CameraMode[] = ["chase", "hood", "bumper", "orbit", "cinematic"];

/** Critically-damped spring toward a target (per component). */
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
    // x'' = -2ωx' - ω²(x - target), semi-implicit Euler.
    const w = this.omega;
    const acc = this.pos.clone().sub(target).multiplyScalar(-w * w).addScaledVector(this.vel, -2 * w);
    this.vel.addScaledVector(acc, dt);
    this.pos.addScaledVector(this.vel, dt);
    return this.pos;
  }
  reset(): void {
    this.init = false;
    this.vel.set(0, 0, 0);
  }
}

export class CameraRig {
  readonly camera: THREE.PerspectiveCamera;
  mode: CameraMode = "chase";
  private readonly orbit: OrbitControls;
  private readonly chasePos = new Spring3(5);
  private readonly chaseLook = new Spring3(9);
  private cineTimer = 0;
  private cineShot = 0;
  private readonly cineAnchor = new THREE.Vector3();

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
  }

  /** Switch camera. `orbitOffset` (vehicle-local metres) places a fresh orbit camera. */
  setMode(mode: CameraMode, target?: THREE.Object3D, orbitOffset: [number, number, number] = [-7, -9, 6]): void {
    this.mode = mode;
    this.orbit.enabled = mode === "orbit";
    this.camera.fov = mode === "hood" || mode === "bumper" ? 75 : 60;
    this.camera.updateProjectionMatrix();
    if (mode === "orbit" && target) {
      const p = target.getWorldPosition(new THREE.Vector3());
      this.orbit.target.copy(p);
      this.camera.position.copy(target.localToWorld(new THREE.Vector3(...orbitOffset)));
      this.camera.lookAt(p);
    }
    this.chasePos.reset();
    this.chaseLook.reset();
    this.cineTimer = 0;
  }

  /** `target` is the vehicle root; `size` its body size [w, l, h] (sim metres). */
  update(target: THREE.Object3D | null, size: [number, number, number], speed: number, dt: number): void {
    if (!target) {
      this.orbit.update();
      return;
    }
    target.updateWorldMatrix(true, false);
    // Vehicle-local points (sim chassis space) → Three world.
    const local = (x: number, y: number, z: number) => target.localToWorld(new THREE.Vector3(x, y, z));
    const [, l, h] = size;
    switch (this.mode) {
      case "chase": {
        const back = 6 + Math.min(Math.abs(speed), 40) * 0.06;
        const desired = local(0, -back - l / 2, h + 1.6);
        // Keep the camera upright-ish: lift in Three's +Y.
        desired.y = Math.max(desired.y, target.getWorldPosition(new THREE.Vector3()).y + 1.5);
        this.camera.position.copy(this.chasePos.step(desired, dt));
        this.camera.lookAt(this.chaseLook.step(local(0, l * 0.3, h * 0.6), dt));
        break;
      }
      case "hood":
        this.camera.position.copy(local(0, l * 0.18, h + 0.15));
        this.camera.lookAt(local(0, l * 0.18 + 20, h - 0.4));
        break;
      case "bumper":
        this.camera.position.copy(local(0, l / 2 + 0.05, 0.45));
        this.camera.lookAt(local(0, l / 2 + 20, 0.3));
        break;
      case "orbit":
        // Follow the car with the orbit pivot; the user controls angle and distance.
        {
          const p = target.getWorldPosition(new THREE.Vector3());
          const delta = p.clone().sub(this.orbit.target);
          this.orbit.target.add(delta);
          this.camera.position.add(delta);
          this.orbit.update();
        }
        break;
      case "cinematic":
        this.cinematic(target, local, dt);
        break;
    }
  }

  /** Replay cinematic: cycles trackside / low chase / overhead shots every 4 s. */
  private cinematic(target: THREE.Object3D, local: (x: number, y: number, z: number) => THREE.Vector3, dt: number): void {
    const p = target.getWorldPosition(new THREE.Vector3());
    this.cineTimer -= dt;
    if (this.cineTimer <= 0) {
      this.cineTimer = 4;
      this.cineShot = (this.cineShot + 1) % 3;
      // Trackside anchor ahead of the car and off to the side.
      this.cineAnchor.copy(local(8, 25, 1.2));
    }
    switch (this.cineShot) {
      case 0:
        this.camera.position.copy(this.cineAnchor);
        break;
      case 1:
        this.camera.position.lerp(local(3, -7, 0.8), 1 - Math.exp(-dt * 3));
        break;
      default:
        this.camera.position.lerp(p.clone().add(new THREE.Vector3(0.1, 25, 0.1)), 1 - Math.exp(-dt * 2));
    }
    this.camera.lookAt(p);
  }
}
