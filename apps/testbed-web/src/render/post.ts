// Post stack for the low-poly dark look (machine S): the scene renders into one HDR target
// (half float, 4× MSAA), a half-resolution bright pass keeps only what is brighter than the
// threshold (the emissive lights: lit surfaces stay below it by design, see palette.ts
// TIME_OF_DAY.bloomThreshold), two small blur levels (1/4 and 1/8 resolution) spread it, and
// one full-screen composite adds the bloom, applies exposure, ACES tonemap, saturation, a
// shadow lift, vignette and a little grain, and writes sRGB to the canvas.
//
// Cost (per frame, 1920×1080): bright 0.5 Mpx × 4 taps, downsamples and blurs ≈ 0.4 Mpx × 5
// taps in total, composite 2 Mpx × 3 taps: ≈ 10 M texture fetches, well under 1 ms on a laptop
// GPU (docs/integration/art.md has the measurements).
import * as THREE from "three";
import { POST } from "./palette";

const VS = /* glsl */ `varying vec2 vUv; void main() { vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }`;

const BRIGHT_FS = /* glsl */ `
  uniform sampler2D tSrc; uniform vec2 uTexel; uniform float uThreshold; varying vec2 vUv;
  vec3 tap(vec2 o) {
    vec3 c = texture2D(tSrc, vUv + o * uTexel).rgb;
    float l = max(c.r, max(c.g, c.b));
    return c * (max(l - uThreshold, 0.0) / max(l, 1e-4));
  }
  void main() {
    vec3 c = tap(vec2(-1.0, -1.0)) + tap(vec2(1.0, -1.0)) + tap(vec2(-1.0, 1.0)) + tap(vec2(1.0, 1.0));
    gl_FragColor = vec4(min(c * 0.25, vec3(60.0)), 1.0);
  }`;

const DOWN_FS = /* glsl */ `
  uniform sampler2D tSrc; uniform vec2 uTexel; varying vec2 vUv;
  void main() {
    vec3 c = texture2D(tSrc, vUv + vec2(-1.0, -1.0) * uTexel).rgb + texture2D(tSrc, vUv + vec2(1.0, -1.0) * uTexel).rgb
           + texture2D(tSrc, vUv + vec2(-1.0, 1.0) * uTexel).rgb + texture2D(tSrc, vUv + vec2(1.0, 1.0) * uTexel).rgb;
    gl_FragColor = vec4(c * 0.25, 1.0);
  }`;

// 9-tap Gaussian folded into 5 linear taps.
const BLUR_FS = /* glsl */ `
  uniform sampler2D tSrc; uniform vec2 uDir; varying vec2 vUv;
  void main() {
    vec3 c = texture2D(tSrc, vUv).rgb * 0.227027;
    c += texture2D(tSrc, vUv + uDir * 1.3846154).rgb * 0.3162162;
    c += texture2D(tSrc, vUv - uDir * 1.3846154).rgb * 0.3162162;
    c += texture2D(tSrc, vUv + uDir * 3.2307692).rgb * 0.0702703;
    c += texture2D(tSrc, vUv - uDir * 3.2307692).rgb * 0.0702703;
    gl_FragColor = vec4(c, 1.0);
  }`;

const COMPOSITE_FS = /* glsl */ `
  uniform sampler2D tScene, tB1, tB2;
  uniform float uBloom, uExposure, uSat, uVig, uGrain, uTime;
  uniform vec3 uLift; uniform vec2 uRes;
  varying vec2 vUv;
  vec3 aces(vec3 x) { return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0); }
  vec3 toSRGB(vec3 c) { return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c)); }
  void main() {
    vec3 col = texture2D(tScene, vUv).rgb;
    col += (texture2D(tB1, vUv).rgb * 0.8 + texture2D(tB2, vUv).rgb * 1.2) * uBloom;
    col = aces(col * uExposure + uLift);
    float l = dot(col, vec3(0.2126, 0.7152, 0.0722));
    col = max(mix(vec3(l), col, uSat), 0.0);
    col = toSRGB(col);
    vec2 d = vUv - 0.5;
    col *= 1.0 - uVig * dot(d, d) * 2.2;
    float n = fract(sin(dot(floor(vUv * uRes) + fract(uTime) * 91.7, vec2(12.9898, 78.233))) * 43758.5453);
    col += (n - 0.5) * uGrain;
    gl_FragColor = vec4(col, 1.0);
  }`;

/** Per-frame post settings (from the time-of-day palette). */
export interface PostParams {
  threshold: number;
  bloom: number;
  exposure: number;
}

function target(w: number, h: number, depth = false, samples = 0): THREE.WebGLRenderTarget {
  return new THREE.WebGLRenderTarget(w, h, {
    type: THREE.HalfFloatType,
    format: THREE.RGBAFormat,
    minFilter: THREE.LinearFilter,
    magFilter: THREE.LinearFilter,
    depthBuffer: depth,
    samples,
  });
}

/** HDR scene target + bloom + tonemap composite. */
export class LowPolyPost {
  private readonly fsScene = new THREE.Scene();
  private readonly fsCam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly quad: THREE.Mesh;
  private readonly bright: THREE.ShaderMaterial;
  private readonly down: THREE.ShaderMaterial;
  private readonly blur: THREE.ShaderMaterial;
  private readonly composite: THREE.ShaderMaterial;
  private sceneRT: THREE.WebGLRenderTarget | null = null;
  private half: THREE.WebGLRenderTarget | null = null;
  private q: [THREE.WebGLRenderTarget, THREE.WebGLRenderTarget] | null = null;
  private e: [THREE.WebGLRenderTarget, THREE.WebGLRenderTarget] | null = null;
  private readonly size = new THREE.Vector2();
  private time = 0;
  /** Bloom on/off (debug; the tonemap still runs). */
  bloomOn = true;

  constructor(private readonly samples = 4) {
    const mat = (fs: string, uniforms: Record<string, THREE.IUniform>) =>
      new THREE.ShaderMaterial({ vertexShader: VS, fragmentShader: fs, uniforms, depthTest: false, depthWrite: false });
    this.bright = mat(BRIGHT_FS, { tSrc: { value: null }, uTexel: { value: new THREE.Vector2() }, uThreshold: { value: 1 } });
    this.down = mat(DOWN_FS, { tSrc: { value: null }, uTexel: { value: new THREE.Vector2() } });
    this.blur = mat(BLUR_FS, { tSrc: { value: null }, uDir: { value: new THREE.Vector2() } });
    this.composite = mat(COMPOSITE_FS, {
      tScene: { value: null },
      tB1: { value: null },
      tB2: { value: null },
      uBloom: { value: 0.6 },
      uExposure: { value: 1.1 },
      uSat: { value: POST.saturation },
      uVig: { value: POST.vignette },
      uGrain: { value: POST.grain },
      uTime: { value: 0 },
      uLift: { value: new THREE.Vector3(...POST.lift) },
      uRes: { value: new THREE.Vector2() },
    });
    this.quad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), this.composite);
    this.quad.frustumCulled = false;
    this.fsScene.add(this.quad);
  }

  private resize(w: number, h: number): void {
    for (const t of [this.sceneRT, this.half, ...(this.q ?? []), ...(this.e ?? [])]) t?.dispose();
    this.sceneRT = target(w, h, true, this.samples);
    const hw = Math.max(1, w >> 1), hh = Math.max(1, h >> 1);
    const qw = Math.max(1, w >> 2), qh = Math.max(1, h >> 2);
    const ew = Math.max(1, w >> 3), eh = Math.max(1, h >> 3);
    this.half = target(hw, hh);
    this.q = [target(qw, qh), target(qw, qh)];
    this.e = [target(ew, eh), target(ew, eh)];
  }

  private pass(renderer: THREE.WebGLRenderer, mat: THREE.ShaderMaterial, out: THREE.WebGLRenderTarget | null): void {
    this.quad.material = mat;
    renderer.setRenderTarget(out);
    renderer.render(this.fsScene, this.fsCam);
  }

  /**
   * Compile every material of `scene` (lit by `lights`) for the HDR scene target (the colour space differs from
   * the canvas, so this is the program variant `render` uses) without drawing.
   */
  compile(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, lights: THREE.Scene = scene): void {
    renderer.getDrawingBufferSize(this.size);
    const w = this.size.x, h = this.size.y;
    if (!this.sceneRT || this.sceneRT.width !== w || this.sceneRT.height !== h) this.resize(w, h);
    const prev = renderer.getRenderTarget();
    renderer.setRenderTarget(this.sceneRT);
    renderer.compile(scene, camera, lights);
    renderer.setRenderTarget(prev);
  }

  /** Render `scene` from `camera` through the post stack onto the canvas. */
  render(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, p: PostParams, dt: number): void {
    renderer.getDrawingBufferSize(this.size);
    const w = this.size.x, h = this.size.y;
    if (!this.sceneRT || this.sceneRT.width !== w || this.sceneRT.height !== h) this.resize(w, h);
    const sceneRT = this.sceneRT!, half = this.half!, q = this.q!, e = this.e!;
    this.time = (this.time + dt) % 1000;

    const prevTarget = renderer.getRenderTarget();
    renderer.setRenderTarget(sceneRT);
    renderer.render(scene, camera);

    const c = this.composite.uniforms;
    if (this.bloomOn && p.bloom > 0) {
      const b = this.bright.uniforms;
      b.tSrc.value = sceneRT.texture;
      (b.uTexel.value as THREE.Vector2).set(1 / w, 1 / h);
      b.uThreshold.value = p.threshold;
      this.pass(renderer, this.bright, half);
      const d = this.down.uniforms;
      d.tSrc.value = half.texture;
      (d.uTexel.value as THREE.Vector2).set(1 / half.width, 1 / half.height);
      this.pass(renderer, this.down, q[0]);
      this.blurPair(renderer, q);
      d.tSrc.value = q[0].texture;
      (d.uTexel.value as THREE.Vector2).set(1 / q[0].width, 1 / q[0].height);
      this.pass(renderer, this.down, e[0]);
      this.blurPair(renderer, e);
      c.uBloom.value = p.bloom;
    } else {
      c.uBloom.value = 0;
    }
    c.tScene.value = sceneRT.texture;
    c.tB1.value = q[0].texture;
    c.tB2.value = e[0].texture;
    c.uExposure.value = p.exposure;
    c.uTime.value = this.time;
    (c.uRes.value as THREE.Vector2).set(w, h);
    this.pass(renderer, this.composite, prevTarget);
  }

  /** Horizontal then vertical blur, result back in `rt[0]`. */
  private blurPair(renderer: THREE.WebGLRenderer, rt: [THREE.WebGLRenderTarget, THREE.WebGLRenderTarget]): void {
    const u = this.blur.uniforms;
    u.tSrc.value = rt[0].texture;
    (u.uDir.value as THREE.Vector2).set(1 / rt[0].width, 0);
    this.pass(renderer, this.blur, rt[1]);
    u.tSrc.value = rt[1].texture;
    (u.uDir.value as THREE.Vector2).set(0, 1 / rt[0].height);
    this.pass(renderer, this.blur, rt[0]);
  }

  dispose(): void {
    for (const t of [this.sceneRT, this.half, ...(this.q ?? []), ...(this.e ?? [])]) t?.dispose();
    for (const m of [this.bright, this.down, this.blur, this.composite]) m.dispose();
  }
}
