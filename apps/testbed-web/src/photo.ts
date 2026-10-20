// Photo mode: freezes the game (a live session or a replay), hands the view to a free camera,
// applies colour filters and saves or copies the frame as a PNG.
//
// F3, the "photo" button or gamepad Start enters; F3 / Esc / Start / B leaves. Keys while in photo mode: WASD fly,
// Q/E down/up, Shift fast, Alt slow, drag the mouse to look, wheel = field of view, Z/X roll,
// 1–7 filter presets, H hide the panel, Space/Enter save, C copy, ,/. step a replay.
// Gamepad: left stick fly, right stick look, LB/RB down/up, A save, Y hide panel, B leave.
//
// The free camera lives in Three world space (+Y up) and is kept within `MAX_RANGE_M` of the
// focused car. Filters are a post-process pass over the rendered frame (after the sRGB output
// transform), so the preview and the saved PNG are the same pixels; the HUD is DOM over the
// canvas and never reaches the image. Nothing here touches the sim: freezing is done by the
// host (pause, or a replay played at speed 0).
import * as THREE from "three";
import { EffectComposer } from "three/examples/jsm/postprocessing/EffectComposer.js";
import { RenderPass } from "three/examples/jsm/postprocessing/RenderPass.js";
import { ShaderPass } from "three/examples/jsm/postprocessing/ShaderPass.js";
import { OutputPass } from "three/examples/jsm/postprocessing/OutputPass.js";
import { isHeld } from "./input";

/** How far the free camera may fly from the focused car, m. */
const MAX_RANGE_M = 80;
/** Base fly speed, m/s (Shift ×4, Alt ×0.25). */
const FLY_SPEED = 6;
/** Debug layers hidden while in photo mode (restored on leaving; saved toggles untouched). */
export const PHOTO_HIDDEN_LAYERS = ["lattice", "stress", "tireForces", "suspension", "slipColour", "contacts", "cgVelocity", "intents"];

/** Filter settings. All neutral = the game's own image. */
export interface PhotoFilter {
  /** Exposure, stops. */
  exposure: number;
  /** Contrast around mid grey, 1 = neutral. */
  contrast: number;
  /** Saturation, 0 = grey, 1 = neutral. */
  saturation: number;
  /** White balance, −1 cold … +1 warm. */
  warmth: number;
  /** Lifted blacks (faded film), 0..0.2. */
  fade: number;
  /** Vignette strength, 0..1. */
  vignette: number;
  /** Film grain, 0..1. */
  grain: number;
  /** 0 colour, 1 black & white, 2 sepia. */
  tone: number;
  /** 2.39:1 letterbox bars. */
  letterbox: boolean;
}

const NEUTRAL: PhotoFilter = { exposure: 0, contrast: 1, saturation: 1, warmth: 0, fade: 0, vignette: 0, grain: 0, tone: 0, letterbox: false };

/** Filter presets in key order (1–7). */
export const PHOTO_PRESETS: { id: string; label: string; filter: PhotoFilter }[] = [
  { id: "natural", label: "natural", filter: { ...NEUTRAL } },
  { id: "noir", label: "noir", filter: { ...NEUTRAL, exposure: -0.1, contrast: 1.35, tone: 1, vignette: 0.55, grain: 0.3 } },
  { id: "sepia", label: "sepia", filter: { ...NEUTRAL, contrast: 1.1, tone: 2, fade: 0.04, vignette: 0.45, grain: 0.2 } },
  { id: "golden", label: "golden hour", filter: { ...NEUTRAL, exposure: 0.1, contrast: 1.08, saturation: 1.15, warmth: 0.6, vignette: 0.25 } },
  { id: "cold", label: "cold steel", filter: { ...NEUTRAL, contrast: 1.15, saturation: 0.8, warmth: -0.55, vignette: 0.3 } },
  { id: "punchy", label: "punchy", filter: { ...NEUTRAL, contrast: 1.25, saturation: 1.45, vignette: 0.2 } },
  { id: "faded", label: "faded film", filter: { ...NEUTRAL, contrast: 0.82, saturation: 0.7, warmth: 0.15, fade: 0.08, vignette: 0.3, grain: 0.22 } },
];

/** Filter pass. Works on display (sRGB-encoded) values, after the OutputPass. */
const FilterShader = {
  name: "PhotoFilter",
  uniforms: {
    tDiffuse: { value: null },
    exposure: { value: 0 },
    contrast: { value: 1 },
    saturation: { value: 1 },
    warmth: { value: 0 },
    fade: { value: 0 },
    vignette: { value: 0 },
    grain: { value: 0 },
    tone: { value: 0 },
    letterbox: { value: 0 },
    aspect: { value: 16 / 9 },
  },
  vertexShader: /* glsl */ `
    varying vec2 vUv;
    void main() {
      vUv = uv;
      gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
    }`,
  fragmentShader: /* glsl */ `
    uniform sampler2D tDiffuse;
    uniform float exposure, contrast, saturation, warmth, fade, vignette, grain, letterbox, aspect;
    uniform int tone;
    varying vec2 vUv;
    // Fixed hash (no time input): the grain is the same in the preview and the saved image.
    float hash(vec2 p) {
      vec3 q = fract(vec3(p.xyx) * 0.1031);
      q += dot(q, q.yzx + 33.33);
      return fract((q.x + q.y) * q.z);
    }
    void main() {
      vec3 c = texture2D(tDiffuse, vUv).rgb;
      c *= exp2(exposure);
      c *= vec3(1.0 + 0.12 * warmth, 1.0 + 0.02 * warmth, 1.0 - 0.14 * warmth);
      float l = dot(c, vec3(0.2126, 0.7152, 0.0722));
      c = mix(vec3(l), c, saturation);
      if (tone == 1) c = vec3(l);
      if (tone == 2) c = vec3(l) * vec3(1.07, 0.93, 0.74);
      c = (c - 0.5) * contrast + 0.5;
      c = fade + c * (1.0 - fade);
      vec2 d = (vUv - 0.5) * vec2(aspect, 1.0);
      c *= 1.0 - vignette * smoothstep(0.35, 1.05, length(d) * 1.2);
      // Grain cell size follows the image, not the pixel: a 2x capture looks like the preview.
      vec2 cell = floor(vUv * vec2(aspect, 1.0) * 720.0);
      c += (hash(cell) - 0.5) * 0.18 * grain;
      // 2.39:1 bars top and bottom (nothing to add on a screen already that wide).
      if (letterbox > 0.5 && aspect < 2.39) {
        float bar = 0.5 * (1.0 - aspect / 2.39);
        if (vUv.y < bar || vUv.y > 1.0 - bar) c = vec3(0.0);
      }
      gl_FragColor = vec4(clamp(c, 0.0, 1.0), 1.0);
    }`,
};

/** What photo mode needs from the app. */
export interface PhotoHost {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  /** Freeze (`true`) or release the game: pause a live session, or play a replay at speed 0. */
  freeze(on: boolean): void;
  /** Step a frozen replay by `ticks` (negative = back); no-op for a live session. */
  replayStep(ticks: number): void;
  isReplay(): boolean;
  /** The focused car's root (the free camera stays near it). */
  focusRoot(): THREE.Object3D | null;
  /** File name stem for saved photos (scenario + tick). */
  fileStem(): string;
  /** Hide or restore the HUD and the debug layers. */
  setChrome(visible: boolean): void;
}

const tmpV = new THREE.Vector3();

/** The claude.ai artifact viewer's `downloads` capability (published testbed only). Inside the
 * viewer a page cannot start a download itself; the viewer asks the person to confirm instead. */
interface ArtifactDownloads {
  save(req: { filename: string; data: Blob }): Promise<{ status: string }>;
}
type ClaudeHost = { use(name: string): Promise<unknown> };
const artifactDownloads: Promise<ArtifactDownloads | null> = (() => {
  const host = (window as unknown as { claude?: ClaudeHost }).claude;
  return host?.use ? host.use("downloads").then((d) => (d as ArtifactDownloads) ?? null, () => null) : Promise.resolve(null);
})();

export class PhotoMode {
  active = false;
  readonly camera = new THREE.PerspectiveCamera(60, window.innerWidth / window.innerHeight, 0.1, 3000);
  readonly filter: PhotoFilter = { ...NEUTRAL };
  /** Capture size multiplier (1 = screen resolution, 2 = twice in each direction). */
  scale = 1;
  /** Photos saved / copied this page (automation). */
  saved = 0;
  copied = 0;
  private presetId = "natural";
  private composer: EffectComposer | null = null;
  private filterPass: ShaderPass | null = null;
  private yaw = 0;
  private pitch = 0;
  private roll = 0;
  private readonly vel = new THREE.Vector3();
  private dragging = false;
  private panelHidden = false;
  private padPrev: boolean[] = [];
  private readonly panel: HTMLElement;
  /** Small button that brings a hidden panel back (touch screens have no H key). */
  private readonly chip: HTMLButtonElement;
  private readonly status: HTMLElement;
  private readonly sliders = new Map<string, HTMLInputElement>();
  private readonly readouts = new Map<string, HTMLOutputElement>();

  constructor(private readonly host: PhotoHost) {
    this.panel = buildPanel();
    document.body.appendChild(this.panel);
    this.chip = document.createElement("button");
    this.chip.id = "photo-chip";
    this.chip.className = "hidden";
    this.chip.textContent = "📷 panel";
    this.chip.title = "show the photo panel (H)";
    this.chip.onclick = () => this.togglePanel();
    document.body.appendChild(this.chip);
    this.status = this.panel.querySelector(".ph-status")!;
    this.wirePanel();
    this.installMouse(host.renderer.domElement);
    window.addEventListener("resize", () => {
      this.camera.aspect = window.innerWidth / window.innerHeight;
      this.camera.updateProjectionMatrix();
      this.composer?.setSize(window.innerWidth, window.innerHeight);
    });
  }

  /** Enter photo mode, starting from the game camera's current view. */
  enter(from: THREE.PerspectiveCamera): void {
    if (this.active) return;
    this.active = true;
    this.host.freeze(true);
    this.host.setChrome(false);
    this.camera.position.copy(from.position);
    this.camera.fov = from.fov;
    this.camera.near = from.near;
    this.camera.far = from.far;
    this.camera.aspect = window.innerWidth / window.innerHeight;
    this.camera.updateProjectionMatrix();
    const e = new THREE.Euler().setFromQuaternion(from.quaternion, "YXZ");
    this.yaw = e.y;
    this.pitch = e.x;
    this.roll = 0;
    this.vel.set(0, 0, 0);
    this.applyRotation();
    this.ensureComposer();
    this.panelHidden = false;
    this.panel.classList.remove("hidden");
    this.chip.classList.add("hidden");
    this.panel.querySelector<HTMLElement>(".ph-replay")!.style.display = this.host.isReplay() ? "" : "none";
    this.syncPanel();
    this.say(this.host.isReplay() ? "replay frozen — , . step frames" : "game paused");
  }

  /** Leave photo mode: the game camera, HUD and play state come back as they were. */
  exit(): void {
    if (!this.active) return;
    this.active = false;
    this.dragging = false;
    this.panel.classList.add("hidden");
    this.chip.classList.add("hidden");
    this.host.setChrome(true);
    this.host.freeze(false);
  }

  /** Key handler; returns true when photo mode used the key (the app ignores it then). */
  onKey(e: KeyboardEvent): boolean {
    if (!this.active) return false;
    const n = Number(e.key);
    if (n >= 1 && n <= PHOTO_PRESETS.length) {
      this.usePreset(PHOTO_PRESETS[n - 1].id);
      return true;
    }
    switch (e.code) {
      case "Escape":
      case "F3":
        e.preventDefault();
        this.exit();
        break;
      case "KeyH":
        this.togglePanel();
        break;
      case "Space":
      case "Enter":
        e.preventDefault();
        void this.save();
        break;
      case "KeyC":
        void this.copy();
        break;
      case "Comma":
        this.host.replayStep(-4);
        break;
      case "Period":
        this.host.replayStep(4);
        break;
    }
    // Every other key is swallowed too: no layer toggles, camera switches or resets mid-shot.
    return true;
  }

  /** Call every frame. Polls the gamepad (Start enters from `gameCamera`'s view) and, while
   * active, flies the free camera. */
  update(dt: number, gameCamera: THREE.PerspectiveCamera): void {
    const pad = this.pollPad(dt, gameCamera);
    if (!this.active) return;
    const speed = FLY_SPEED * (isHeld("ShiftLeft") || isHeld("ShiftRight") ? 4 : isHeld("AltLeft") || isHeld("AltRight") ? 0.25 : 1);
    const axis = (pos: string, neg: string) => (isHeld(pos) ? 1 : 0) - (isHeld(neg) ? 1 : 0);
    const fwd = axis("KeyW", "KeyS") + axis("ArrowUp", "ArrowDown") + pad.fwd;
    const right = axis("KeyD", "KeyA") + axis("ArrowRight", "ArrowLeft") + pad.right;
    const up = axis("KeyE", "KeyQ") + pad.up;
    const rollIn = axis("KeyX", "KeyZ");
    if (rollIn) {
      this.roll = THREE.MathUtils.clamp(this.roll + rollIn * 0.6 * dt, -0.8, 0.8);
      this.syncPanel();
    }
    // Fly along the view (pitch included); Q/E are world up/down.
    const dirF = new THREE.Vector3(0, 0, -1).applyEuler(new THREE.Euler(this.pitch, this.yaw, 0, "YXZ"));
    const dirR = new THREE.Vector3(1, 0, 0).applyAxisAngle(new THREE.Vector3(0, 1, 0), this.yaw);
    const want = dirF.multiplyScalar(fwd).addScaledVector(dirR, right).addScaledVector(new THREE.Vector3(0, 1, 0), up);
    if (want.lengthSq() > 1) want.normalize();
    want.multiplyScalar(speed);
    // Eased start/stop: smooth enough to frame a shot, quick enough not to drift past it.
    this.vel.lerp(want, 1 - Math.exp(-10 * dt));
    this.camera.position.addScaledVector(this.vel, dt);
    const car = this.host.focusRoot();
    if (car) {
      const c = car.getWorldPosition(tmpV);
      const off = this.camera.position.clone().sub(c);
      if (off.length() > MAX_RANGE_M) this.camera.position.copy(c).add(off.setLength(MAX_RANGE_M));
    }
    this.applyRotation();
  }

  /** Draw the filtered frame to the canvas. */
  render(): void {
    this.ensureComposer();
    const u = this.filterPass!.uniforms;
    const f = this.filter;
    u.exposure.value = f.exposure;
    u.contrast.value = f.contrast;
    u.saturation.value = f.saturation;
    u.warmth.value = f.warmth;
    u.fade.value = f.fade;
    u.vignette.value = f.vignette;
    u.grain.value = f.grain;
    u.tone.value = f.tone;
    u.letterbox.value = f.letterbox ? 1 : 0;
    u.aspect.value = this.camera.aspect;
    this.composer!.render();
  }

  /** Render the photo at `scale` × screen size and return it as a PNG. */
  async capture(scale = this.scale): Promise<{ blob: Blob; width: number; height: number }> {
    const r = this.host.renderer;
    const size = r.getSize(new THREE.Vector2());
    const pr = r.getPixelRatio();
    // Stay within what the GPU can draw (and a sane file size).
    const limit = Math.min(r.capabilities.maxTextureSize, 8192);
    const want = Math.min(pr * scale, limit / Math.max(size.x, size.y));
    const resized = Math.abs(want - pr) > 1e-3;
    if (resized) {
      r.setPixelRatio(want);
      this.composer!.setPixelRatio(want);
      this.composer!.setSize(size.x, size.y);
    }
    this.render();
    const src = r.domElement;
    const out = document.createElement("canvas");
    out.width = src.width;
    out.height = src.height;
    out.getContext("2d")!.drawImage(src, 0, 0);
    if (resized) {
      r.setPixelRatio(pr);
      this.composer!.setPixelRatio(pr);
      this.composer!.setSize(size.x, size.y);
      this.render();
    }
    const blob = await new Promise<Blob>((res, rej) => out.toBlob((b) => (b ? res(b) : rej(new Error("toBlob failed"))), "image/png"));
    return { blob, width: out.width, height: out.height };
  }

  /** Save the photo as a PNG download. */
  async save(): Promise<void> {
    try {
      const { blob, width, height } = await this.capture();
      const name = `wreckpursuit-${this.host.fileStem()}.png`;
      const viewer = await artifactDownloads;
      if (viewer) {
        // Published testbed: the viewer shows a confirmation; "declined" is the person's choice.
        await viewer.save({ filename: name, data: blob });
      } else {
        const a = document.createElement("a");
        a.href = URL.createObjectURL(blob);
        a.download = name;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 1000);
      }
      this.saved++;
      this.say(`saved ${name} (${width}×${height})`);
    } catch (e) {
      const code = (e as { code?: string }).code;
      this.say(code === "declined" ? "not saved" : `save failed: ${code ?? e}`);
    }
  }

  /** Copy the photo to the clipboard as a PNG (needs https or localhost). */
  async copy(): Promise<void> {
    if (!navigator.clipboard?.write || typeof ClipboardItem === "undefined") {
      this.say("this browser cannot copy images here — use save");
      return;
    }
    try {
      let dims = "";
      // A promise inside the ClipboardItem keeps the click's user activation (Safari).
      const blob = this.capture().then((c) => {
        dims = `${c.width}×${c.height}`;
        return c.blob;
      });
      await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
      this.copied++;
      this.say(`copied ${dims} to the clipboard`);
    } catch (e) {
      this.say(`copy failed (${(e as Error).name ?? e}) — use save`);
    }
  }

  /** Apply a preset by id (`PHOTO_PRESETS`). */
  usePreset(id: string): void {
    const p = PHOTO_PRESETS.find((q) => q.id === id);
    if (!p) return;
    const letterbox = this.filter.letterbox;
    Object.assign(this.filter, p.filter, { letterbox });
    this.presetId = id;
    this.syncPanel();
  }

  /** Point the camera back at the focused car from just behind where it is. */
  resetCamera(): void {
    const car = this.host.focusRoot();
    if (!car) return;
    const c = car.getWorldPosition(new THREE.Vector3()).add(new THREE.Vector3(0, 0.8, 0));
    const fwd = car.localToWorld(new THREE.Vector3(0, 1, 0)).sub(car.getWorldPosition(new THREE.Vector3())).setY(0).normalize();
    this.camera.position.copy(c).addScaledVector(fwd, -6).add(new THREE.Vector3(0, 1.2, 0));
    this.camera.lookAt(c);
    const e = new THREE.Euler().setFromQuaternion(this.camera.quaternion, "YXZ");
    this.yaw = e.y;
    this.pitch = e.x;
    this.roll = 0;
    this.camera.fov = 50;
    this.camera.updateProjectionMatrix();
    this.vel.set(0, 0, 0);
    this.applyRotation();
    this.syncPanel();
  }

  // ----- internals -------------------------------------------------------------------------

  private applyRotation(): void {
    this.camera.quaternion.setFromEuler(new THREE.Euler(this.pitch, this.yaw, this.roll, "YXZ"));
  }

  private ensureComposer(): void {
    if (this.composer) return;
    const r = this.host.renderer;
    const size = r.getSize(new THREE.Vector2());
    // Multisampled target: the composer would otherwise lose the canvas' antialiasing.
    const rt = new THREE.WebGLRenderTarget(size.x * r.getPixelRatio(), size.y * r.getPixelRatio(), {
      type: THREE.HalfFloatType,
      samples: Math.min(4, r.capabilities.maxSamples),
    });
    this.composer = new EffectComposer(r, rt);
    this.composer.addPass(new RenderPass(this.host.scene, this.camera));
    this.composer.addPass(new OutputPass());
    this.filterPass = new ShaderPass(FilterShader);
    this.composer.addPass(this.filterPass);
  }

  private togglePanel(): void {
    this.panelHidden = !this.panelHidden;
    this.panel.classList.toggle("hidden", this.panelHidden);
    this.chip.classList.toggle("hidden", !this.panelHidden);
  }

  private say(text: string): void {
    this.status.textContent = text;
  }

  private installMouse(dom: HTMLElement): void {
    let lastX = 0;
    let lastY = 0;
    dom.addEventListener("pointerdown", (e) => {
      if (!this.active || e.button !== 0) return;
      this.dragging = true;
      lastX = e.clientX;
      lastY = e.clientY;
    });
    window.addEventListener("pointermove", (e) => {
      if (!this.dragging) return;
      // Slower look at narrow FOV, like a long lens.
      const k = 0.0045 * (this.camera.fov / 60);
      this.yaw -= (e.clientX - lastX) * k;
      this.pitch = THREE.MathUtils.clamp(this.pitch - (e.clientY - lastY) * k, -1.5, 1.5);
      lastX = e.clientX;
      lastY = e.clientY;
    });
    const up = () => (this.dragging = false);
    window.addEventListener("pointerup", up);
    window.addEventListener("blur", up);
    dom.addEventListener(
      "wheel",
      (e) => {
        if (!this.active) return;
        e.preventDefault();
        this.setFov(this.camera.fov * Math.exp(e.deltaY * 0.001));
      },
      { passive: false },
    );
  }

  private setFov(fov: number): void {
    this.camera.fov = THREE.MathUtils.clamp(fov, 10, 100);
    this.camera.updateProjectionMatrix();
    this.syncPanel();
  }

  /** Gamepad (standard mapping): Start enters/leaves; while active sticks fly/look, LB/RB
   * down/up, A save, B leave, Y hide the panel. */
  private pollPad(dt: number, gameCamera: THREE.PerspectiveCamera): { fwd: number; right: number; up: number } {
    const none = { fwd: 0, right: 0, up: 0 };
    const pads = navigator.getGamepads ? navigator.getGamepads() : [];
    const pad = Array.from(pads).find((p) => p && p.connected);
    if (!pad) return none;
    const pressed = pad.buttons.map((b) => b.pressed);
    const rising = (i: number) => pressed[i] && !this.padPrev[i];
    this.padPrev = pressed;
    if (!this.active) {
      if (rising(9)) this.enter(gameCamera);
      return none;
    }
    if (rising(9) || rising(1)) {
      this.exit();
      return none;
    }
    if (rising(0)) void this.save();
    if (rising(3)) this.togglePanel();
    const dz = (a: number) => (Math.abs(a) < 0.15 ? 0 : a);
    this.yaw -= dz(pad.axes[2] ?? 0) * 1.6 * dt * (this.camera.fov / 60);
    this.pitch = THREE.MathUtils.clamp(this.pitch - dz(pad.axes[3] ?? 0) * 1.2 * dt * (this.camera.fov / 60), -1.5, 1.5);
    return { fwd: -dz(pad.axes[1] ?? 0), right: dz(pad.axes[0] ?? 0), up: (pressed[5] ? 1 : 0) - (pressed[4] ? 1 : 0) };
  }

  private wirePanel(): void {
    const presets = this.panel.querySelector<HTMLElement>(".ph-presets")!;
    PHOTO_PRESETS.forEach((p, i) => {
      const b = document.createElement("button");
      b.textContent = `${i + 1} ${p.label}`;
      b.dataset.preset = p.id;
      b.onclick = () => this.usePreset(p.id);
      presets.appendChild(b);
    });
    // Hand keyboard focus back after each change: the app ignores keys typed into controls.
    this.panel.querySelectorAll<HTMLElement>("input, select").forEach((c) => c.addEventListener("change", () => c.blur()));
    this.panel.querySelectorAll<HTMLOutputElement>("output").forEach((o) => this.readouts.set(o.dataset.k!, o));
    this.panel.querySelectorAll<HTMLInputElement>("input[type=range]").forEach((s) => {
      this.sliders.set(s.dataset.k!, s);
      s.oninput = () => {
        const v = Number(s.value);
        if (s.dataset.k === "fov") return this.setFov(v);
        if (s.dataset.k === "roll") {
          this.roll = THREE.MathUtils.degToRad(v);
          this.applyRotation();
          return this.syncPanel();
        }
        (this.filter as unknown as Record<string, number>)[s.dataset.k!] = v;
        this.presetId = "";
        this.syncPanel();
      };
    });
    const lb = this.panel.querySelector<HTMLInputElement>(".ph-letterbox")!;
    lb.onchange = () => (this.filter.letterbox = lb.checked);
    const size = this.panel.querySelector<HTMLSelectElement>(".ph-size")!;
    size.onchange = () => (this.scale = Number(size.value));
    const on = (cls: string, f: () => void) => (this.panel.querySelector<HTMLElement>(cls)!.onclick = f);
    on(".ph-save", () => void this.save());
    on(".ph-copy", () => void this.copy());
    on(".ph-reset", () => this.resetCamera());
    on(".ph-exit", () => this.exit());
    on(".ph-hide", () => this.togglePanel());
    on(".ph-back", () => this.host.replayStep(-4));
    on(".ph-fwd", () => this.host.replayStep(4));
  }

  /** Reflect the current filter / camera values in the panel. */
  private syncPanel(): void {
    const f = this.filter as unknown as Record<string, number>;
    for (const [k, s] of this.sliders) {
      const v = k === "fov" ? this.camera.fov : k === "roll" ? THREE.MathUtils.radToDeg(this.roll) : f[k];
      if (document.activeElement !== s) s.value = String(v);
      const out = this.readouts.get(k);
      if (out) out.textContent = k === "fov" || k === "roll" ? `${v.toFixed(0)}°` : k === "exposure" ? `${v >= 0 ? "+" : ""}${v.toFixed(2)}` : v.toFixed(2);
    }
    this.panel.querySelectorAll<HTMLButtonElement>(".ph-presets button").forEach((b) => {
      b.classList.toggle("on", b.dataset.preset === this.presetId);
    });
    this.panel.querySelector<HTMLInputElement>(".ph-letterbox")!.checked = this.filter.letterbox;
  }
}

/** The photo-mode panel (outside #overlay, which photo mode hides). */
function buildPanel(): HTMLElement {
  const el = document.createElement("div");
  el.id = "photo-panel";
  el.className = "panel hidden";
  const slider = (k: string, label: string, min: number, max: number, step: number) =>
    `<label>${label}<input type="range" data-k="${k}" min="${min}" max="${max}" step="${step}" /><output data-k="${k}"></output></label>`;
  el.innerHTML =
    `<b>📷 Photo mode</b> <span class="dim">H hide · Esc leave</span>` +
    `<div class="ph-presets"></div>` +
    `<div class="ph-grid">` +
    slider("exposure", "exposure", -2, 2, 0.05) +
    slider("contrast", "contrast", 0.5, 1.6, 0.01) +
    slider("saturation", "saturation", 0, 2, 0.01) +
    slider("warmth", "warmth", -1, 1, 0.01) +
    slider("fade", "fade", 0, 0.2, 0.005) +
    slider("vignette", "vignette", 0, 1, 0.01) +
    slider("grain", "grain", 0, 1, 0.01) +
    slider("fov", "field of view", 10, 100, 1) +
    slider("roll", "roll", -45, 45, 0.5) +
    `</div>` +
    `<div class="row"><label><input type="checkbox" class="ph-letterbox" /> letterbox 2.39:1</label>` +
    ` size <select class="ph-size"><option value="1">screen</option><option value="2">2× screen</option></select></div>` +
    `<div class="row ph-replay">replay frame <button class="ph-back">◀</button><button class="ph-fwd">▶</button></div>` +
    `<div class="row"><button class="ph-save">💾 save PNG</button><button class="ph-copy">📋 copy</button>` +
    `<button class="ph-reset">reset camera</button><button class="ph-hide">hide</button><button class="ph-exit">leave</button></div>` +
    `<div class="ph-status dim"></div>` +
    `<div class="dim ph-keys">WASD fly · Q/E down/up · Shift fast · Alt slow · drag look · wheel zoom · Z/X roll · ` +
    `1–7 filters · Space save · C copy</div>`;
  return el;
}
