// Weather rendering (lead, weather in the game): rain streaks, wet roads with puddle sheen, spray
// behind fast cars, fog from visibility, cloudy sky, windscreen wipers in mounted cameras and a
// small weather / road-state HUD. Everything reads wp_wasm's weather exports:
//   weather_info()       per frame (WX offsets below)
//   weather_grid()       ~1 Hz, 2 bytes per 8 m cell: wetness, standing water (mm × 40)
//   weather_spray()      per frame, one value per sim vehicle (vehicle_states() order)
// Cost: the rain is one instanced line draw animated on the GPU (no per-frame CPU work besides
// uniforms); spray is one Points draw over a fixed pool; the wet roads are a few extra shader
// instructions on the road materials. Without weather everything here is hidden and the road
// materials render exactly as before (uWetOn = 0, envMapIntensity = 0).
import * as THREE from "three";
import { tagFeature } from "../perf";
import { S, STATE_STRIDE } from "../wasm/layout";

/** Offsets inside weather_info() (crates/wp_wasm/src/weather.rs `info`). */
export const WX = {
  enabled: 0,
  kind: 1,
  intensity: 2,
  rainMmH: 3,
  fogPerM: 4,
  visibilityM: 5,
  cloud: 6,
  sunDimming: 7,
  windX: 8,
  windY: 9,
  roadWetness: 10,
  airC: 11,
  roadC: 12,
  locked: 13,
  waterMm: 14,
  wetness: 15,
  standingMm: 16,
  aquaplane: 17,
} as const;

const FOG_DAY = new THREE.Color(0xa7afb6);
const FOG_NIGHT = new THREE.Color(0x0b0e14);

/** Regime names by `WeatherKind::code`. */
export const WEATHER_NAMES = ["clear", "overcast", "light rain", "heavy rain", "fog"];

/** Standing-water byte scale in weather_grid() (`wp_wasm::weather::STANDING_Q`). */
const STANDING_Q = 40;
/** Rain streak pool; the drawn count scales with the rain rate. */
const RAIN_MAX = 5000;
/** Rain box around the camera (Three units = m): x, y (up), z. */
const RAIN_BOX = new THREE.Vector3(70, 36, 70);
/** Spray particle pool (all cars together). */
const SPRAY_MAX = 1500;
/** Road materials that get wet (data/city/*.greybox.glb material names). */
const WET_MATERIALS = new Set([
  "asphalt",
  "highway_concrete",
  "parking",
  "sidewalk",
  "sidewalk_corner",
  "lot",
  "kerb",
  "bridge",
]);

const RAIN_VERT = /* glsl */ `
  attribute vec3 offset;
  uniform float uTime;
  uniform vec3 uCam;
  uniform vec3 uBox;
  uniform vec3 uVel;
  uniform float uLen;
  varying float vFade;
  void main() {
    vec3 lo = uCam - 0.5 * uBox;
    vec3 p = lo + mod(offset * uBox + uVel * uTime - lo, uBox);
    // position.y: 0 = head, 1 = tail of the streak (against the fall direction).
    p -= normalize(uVel) * uLen * position.y;
    vFade = 1.0 - position.y;
    // Thin out near the box edges so the wrap is invisible.
    vec3 q = abs(p - uCam) / (0.5 * uBox);
    vFade *= 1.0 - smoothstep(0.7, 1.0, max(q.x, q.z));
    gl_Position = projectionMatrix * viewMatrix * vec4(p, 1.0);
  }`;
const RAIN_FRAG = /* glsl */ `
  uniform vec3 uColor;
  uniform float uOpacity;
  varying float vFade;
  void main() { gl_FragColor = vec4(uColor, uOpacity * (0.25 + 0.75 * vFade)); }`;

/** Gradient sky dome for the wet-road reflections. */
function skyDome(): THREE.Scene {
  const geo = new THREE.SphereGeometry(10, 32, 16);
  const pos = geo.attributes.position;
  const colours = new Float32Array(pos.count * 3);
  const sky = new THREE.Color(0x9aa4ae);
  const horizon = new THREE.Color(0xc4ccd3);
  const ground = new THREE.Color(0x1a1d21);
  const c = new THREE.Color();
  for (let i = 0; i < pos.count; i++) {
    const y = pos.getY(i) / 10;
    if (y >= 0) c.copy(horizon).lerp(sky, Math.min(1, y * 2));
    else c.copy(horizon).lerp(ground, Math.min(1, -y * 6));
    c.toArray(colours, i * 3);
  }
  geo.setAttribute("color", new THREE.BufferAttribute(colours, 3));
  const scene = new THREE.Scene();
  scene.add(new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.BackSide })));
  return scene;
}

interface Particle {
  life: number;
  max: number;
}

export class WeatherView {
  private readonly rain: THREE.LineSegments;
  private readonly rainGeo: THREE.InstancedBufferGeometry;
  private readonly rainMat: THREE.ShaderMaterial;
  private readonly sprayGeo = new THREE.BufferGeometry();
  private readonly spray: THREE.Points;
  private readonly sprayMat: THREE.ShaderMaterial;
  private readonly sprayPos = new Float32Array(SPRAY_MAX * 3);
  private readonly sprayVel = new Float32Array(SPRAY_MAX * 3);
  private readonly sprayAlpha = new Float32Array(SPRAY_MAX);
  private readonly particles: Particle[] = Array.from({ length: SPRAY_MAX }, () => ({ life: 0, max: 1 }));
  private sprayNext = 0;
  private readonly fog = new THREE.FogExp2(0x9aa3ab, 0);
  private readonly fogGrey = new THREE.Color();
  /** Last weather HUD text (only rewritten when it changes) and wiper angle. */
  private hudHtml = "";
  private hudShown: boolean | null = null;
  private readonly wetUniforms = {
    uWetGrid: { value: new THREE.DataTexture(new Uint8Array(2), 1, 1, THREE.RGFormat) as THREE.Texture },
    uWetRect: { value: new THREE.Vector4(0, 0, 1, 1) },
    uWetOn: { value: 0 },
    uWetMean: { value: 0 },
  };
  private readonly wetMaterials = new Set<THREE.MeshStandardMaterial>();
  private readonly envMap: THREE.Texture;
  private gridAge = Infinity;
  private time = 0;
  private wiperPhase = 0;
  private readonly hud: HTMLElement;
  private readonly wipers: HTMLElement;
  private readonly blades: HTMLElement[];
  /** Fraction of the rain streak pool drawn (graphics quality; 1 = all). */
  rainFraction = 1;
  /** Last weather_info(), for automation and the HUD. */
  info: Float32Array = new Float32Array(18);

  constructor(
    private readonly scene: THREE.Scene,
    private readonly renderer: THREE.WebGLRenderer,
    overlay: HTMLElement,
  ) {
    // Rain: 2-vertex streak, instanced; all motion in the vertex shader.
    this.rainGeo = new THREE.InstancedBufferGeometry();
    this.rainGeo.setAttribute("position", new THREE.Float32BufferAttribute([0, 0, 0, 0, 1, 0], 3));
    const offsets = new Float32Array(RAIN_MAX * 3);
    // Fixed pseudo-random layout (render only; no sim randomness involved).
    let s = 0x2545f491;
    const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
    for (let i = 0; i < offsets.length; i++) offsets[i] = rnd();
    this.rainGeo.setAttribute("offset", new THREE.InstancedBufferAttribute(offsets, 3));
    this.rainGeo.instanceCount = 0;
    this.rainMat = new THREE.ShaderMaterial({
      vertexShader: RAIN_VERT,
      fragmentShader: RAIN_FRAG,
      transparent: true,
      depthWrite: false,
      uniforms: {
        uTime: { value: 0 },
        uCam: { value: new THREE.Vector3() },
        uBox: { value: RAIN_BOX.clone() },
        uVel: { value: new THREE.Vector3(0, -9, 0) },
        uLen: { value: 0.7 },
        uColor: { value: new THREE.Color(0xc8d2dc) },
        uOpacity: { value: 0.35 },
      },
    });
    this.rain = new THREE.LineSegments(this.rainGeo, this.rainMat);
    this.rain.frustumCulled = false;
    this.rain.visible = false;
    this.rain.renderOrder = 5;
    scene.add(tagFeature(this.rain, "weather"));

    // Spray: one Points cloud in Three's world space (sim positions converted on spawn).
    this.sprayGeo.setAttribute("position", new THREE.BufferAttribute(this.sprayPos, 3));
    this.sprayGeo.setAttribute("alpha", new THREE.BufferAttribute(this.sprayAlpha, 1));
    const sprayMat = new THREE.ShaderMaterial({
      transparent: true,
      depthWrite: false,
      uniforms: { uColor: { value: new THREE.Color(0xdfe5ea) }, uScale: { value: renderer.getPixelRatio() * 420 } },
      vertexShader: /* glsl */ `
        attribute float alpha;
        uniform float uScale;
        varying float vA;
        void main() {
          vec4 mv = modelViewMatrix * vec4(position, 1.0);
          vA = alpha;
          // Capped: a camera right behind a spraying car would otherwise fill the screen.
          gl_PointSize = min(uScale * (0.6 + 1.4 * (1.0 - alpha)) / max(-mv.z, 0.5), 32.0);
          gl_Position = projectionMatrix * mv;
        }`,
      fragmentShader: /* glsl */ `
        uniform vec3 uColor;
        varying float vA;
        void main() {
          float r = length(gl_PointCoord - 0.5) * 2.0;
          gl_FragColor = vec4(uColor, vA * 0.32 * (1.0 - smoothstep(0.3, 1.0, r)));
        }`,
    });
    this.spray = new THREE.Points(this.sprayGeo, sprayMat);
    this.sprayMat = sprayMat;
    this.spray.frustumCulled = false;
    this.spray.renderOrder = 4;
    scene.add(tagFeature(this.spray, "weather"));

    // Reflections for wet roads (intensity 0 when dry, so dry roads look as before): an
    // overcast sky dome, light grey above the horizon, dark below, so wet asphalt reads darker
    // with a sheen toward the horizon and the lights.
    const pmrem = new THREE.PMREMGenerator(renderer);
    this.envMap = pmrem.fromScene(skyDome(), 0).texture;
    pmrem.dispose();

    this.hud = document.createElement("div");
    this.hud.id = "weather-hud";
    this.hud.className = "panel";
    this.hud.style.cssText =
      "position:absolute;top:178px;left:50%;transform:translateX(-50%);font:12px monospace;padding:3px 8px;display:none;pointer-events:none;white-space:nowrap;";
    overlay.appendChild(this.hud);
    this.wipers = document.createElement("div");
    this.wipers.id = "wipers";
    this.wipers.style.cssText = "position:absolute;inset:0;pointer-events:none;overflow:hidden;display:none;";
    this.blades = [0, 1].map((i) => {
      const b = document.createElement("div");
      b.style.cssText =
        `position:absolute;bottom:-2%;left:${i ? 58 : 18}%;width:46vw;height:5px;background:#111;` +
        "border-radius:3px;transform-origin:0 50%;box-shadow:0 0 3px #000;opacity:0.85;";
      this.wipers.appendChild(b);
      return b;
    });
    overlay.appendChild(this.wipers);
  }

  /** Make a loaded city's road materials wet-capable (call once per city scene). */
  attachCity(root: THREE.Object3D): void {
    root.traverse((o) => {
      const m = (o as THREE.Mesh).material as THREE.Material | THREE.Material[] | undefined;
      for (const mat of m ? (Array.isArray(m) ? m : [m]) : []) {
        if (mat instanceof THREE.MeshStandardMaterial && WET_MATERIALS.has(mat.name)) this.patch(mat);
      }
    });
  }

  /** Wet-capable material outside a city (the flat world's ground plane). */
  attachMaterial(mat: THREE.MeshStandardMaterial): void {
    this.patch(mat);
  }

  private patch(mat: THREE.MeshStandardMaterial): void {
    if (this.wetMaterials.has(mat)) return;
    this.wetMaterials.add(mat);
    mat.envMap = this.envMap;
    mat.envMapIntensity = 0;
    const u = this.wetUniforms;
    mat.onBeforeCompile = (sh) => {
      Object.assign(sh.uniforms, u);
      sh.vertexShader =
        "varying vec3 vWetPos;\n" +
        sh.vertexShader.replace(
          "#include <project_vertex>",
          "#include <project_vertex>\n  vWetPos = (modelMatrix * vec4(transformed, 1.0)).xyz;",
        );
      sh.fragmentShader =
        "uniform sampler2D uWetGrid;\nuniform vec4 uWetRect;\nuniform float uWetOn;\nuniform float uWetMean;\nvarying vec3 vWetPos;\n" +
        sh.fragmentShader.replace(
          "#include <roughnessmap_fragment>",
          /* glsl */ `#include <roughnessmap_fragment>
  {
    // Three is +Y up; sim XY = (x, -z).
    vec2 wuv = (vec2(vWetPos.x, -vWetPos.z) - uWetRect.xy) / uWetRect.zw;
    bool inside = all(greaterThanEqual(wuv, vec2(0.0))) && all(lessThanEqual(wuv, vec2(1.0)));
    vec2 wg = inside ? texture2D(uWetGrid, wuv).rg : vec2(uWetMean, 0.0);
    float wet = uWetOn * wg.r;
    float puddle = uWetOn * smoothstep(0.02, 0.2, wg.g);
    diffuseColor.rgb *= mix(1.0, 0.5, wet) * mix(1.0, 0.8, puddle);
    roughnessFactor = mix(roughnessFactor, 0.28, wet);
    roughnessFactor = mix(roughnessFactor, 0.03, puddle);
  }`,
        );
    };
    mat.customProgramCacheKey = () => "wp-wet-road";
    mat.needsUpdate = true;
  }

  /** New session: drop the old grid and particles. */
  reset(): void {
    this.gridAge = Infinity;
    this.sprayAlpha.fill(0);
    for (const p of this.particles) p.life = 0;
    this.sprayGeo.attributes.alpha.needsUpdate = true;
  }

  /**
   * One frame. `info` = weather_info(); `grid` fetches the wetness grid (called at ~1 Hz);
   * `states` / `spray` = vehicle_states() / weather_spray(); `mounted` = hood/bumper/cockpit
   * camera (wipers).
   */
  update(
    dt: number,
    camera: THREE.Camera,
    info: Float32Array,
    night: number,
    grid: () => { meta: Float32Array; bytes: Uint8Array },
    states: Float32Array,
    spray: Float32Array,
    mounted: boolean,
  ): void {
    this.info = info;
    const on = info[WX.enabled] > 0.5;
    this.time = (this.time + dt) % 1000;
    this.updateFog(on, info, night);
    this.updateRain(on, info, camera, night);
    this.updateRoads(on, info, night, dt, grid);
    // Point size follows the resolution scale (graphics quality can change it at run time).
    this.sprayMat.uniforms.uScale.value = this.renderer.getPixelRatio() * 420;
    this.updateSpray(on, dt, states, spray);
    this.updateWipers(on && mounted, info, dt);
    this.updateHud(on, info);
  }

  private updateFog(on: boolean, info: Float32Array, night: number): void {
    if (!on) {
      if (this.scene.fog === this.fog) this.scene.fog = null;
      return;
    }
    this.scene.fog = this.fog;
    // Koschmieder: visibility = 3.912 / extinction. Clear air (800 m) keeps only a light haze.
    const vis = Math.max(info[WX.visibilityM], 30);
    this.fog.density = vis >= 799 ? 0.0012 * info[WX.cloud] : 3.0 / vis;
    const grey = this.fogGrey.copy(FOG_DAY).lerp(FOG_NIGHT, Math.min(1, night * 1.1));
    this.fog.color.copy(grey);
    // Sky follows the fog colour as the weather closes in.
    const bg = this.scene.background;
    if (bg instanceof THREE.Color) bg.lerp(grey, Math.min(1, 0.75 * info[WX.cloud] + (vis < 799 ? 0.4 : 0)));
  }

  private updateRain(on: boolean, info: Float32Array, camera: THREE.Camera, night: number): void {
    const rate = on ? info[WX.rainMmH] : 0;
    this.rain.visible = rate > 0.05;
    if (!this.rain.visible) return;
    // Light drizzle ~15 % of the pool, a 16 mm/h downpour all of it.
    this.rainGeo.instanceCount = Math.round(RAIN_MAX * this.rainFraction * Math.min(1, 0.12 + rate / 14));
    const u = this.rainMat.uniforms;
    u.uTime.value = this.time;
    (u.uCam.value as THREE.Vector3).copy(camera.position);
    // Fall speed ~9 m/s plus the wind (sim XY → Three x, -z).
    (u.uVel.value as THREE.Vector3).set(info[WX.windX], -8.5 - rate * 0.08, -info[WX.windY]);
    u.uLen.value = 0.5 + Math.min(rate, 16) * 0.03;
    u.uOpacity.value = (0.22 + Math.min(rate, 16) * 0.012) * (1 - 0.45 * night);
  }

  private updateRoads(
    on: boolean,
    info: Float32Array,
    night: number,
    dt: number,
    grid: () => { meta: Float32Array; bytes: Uint8Array },
  ): void {
    const u = this.wetUniforms;
    u.uWetOn.value = on ? 1 : 0;
    u.uWetMean.value = on ? info[WX.roadWetness] : 0;
    const env = on ? (0.1 + 0.6 * info[WX.roadWetness]) * (1 - 0.85 * night) : 0;
    for (const m of this.wetMaterials) m.envMapIntensity = env;
    if (!on) return;
    this.gridAge += dt;
    if (this.gridAge < 1) return;
    this.gridAge = 0;
    const { meta, bytes } = grid();
    if (meta.length < 5 || bytes.length < meta[3] * meta[4] * 2) return;
    const [ox, oy, cell, nx, ny] = meta;
    let tex = u.uWetGrid.value as THREE.DataTexture;
    if (tex.image.width !== nx || tex.image.height !== ny) {
      tex.dispose();
      tex = new THREE.DataTexture(new Uint8Array(nx * ny * 2), nx, ny, THREE.RGFormat, THREE.UnsignedByteType);
      tex.magFilter = THREE.LinearFilter;
      tex.minFilter = THREE.LinearFilter;
      tex.unpackAlignment = 1;
      u.uWetGrid.value = tex;
    }
    (tex.image.data as Uint8Array).set(bytes.subarray(0, nx * ny * 2));
    tex.needsUpdate = true;
    u.uWetRect.value.set(ox, oy, nx * cell, ny * cell);
  }

  private updateSpray(on: boolean, dt: number, states: Float32Array, spray: Float32Array): void {
    const pos = this.sprayPos;
    const vel = this.sprayVel;
    // Spawn: a few particles per wheel track per frame, scaled by spray × speed.
    if (on && dt > 0) {
      for (let o = 0, i = 0; o + STATE_STRIDE <= states.length; o += STATE_STRIDE, i++) {
        const s = spray[i] ?? 0;
        const speed = states[o + S.speed];
        if (s < 0.03 || speed < 6) continue;
        const [qx, qy, qz, qw] = [states[o + S.rot], states[o + S.rot + 1], states[o + S.rot + 2], states[o + S.rot + 3]];
        // Chassis +Y (forward) and +X (right) in world.
        const fx = 2 * (qx * qy - qw * qz), fy = 1 - 2 * (qx * qx + qz * qz);
        const rx = 1 - 2 * (qy * qy + qz * qz), ry = 2 * (qx * qy + qw * qz);
        const n = Math.min(12, Math.round(s * speed * dt * 60));
        for (let k = 0; k < n; k++) {
          const p = this.particles[this.sprayNext];
          const j = this.sprayNext * 3;
          this.sprayNext = (this.sprayNext + 1) % SPRAY_MAX;
          const side = k % 2 ? 0.8 : -0.8;
          const sx = states[o + S.pos] - fx * 2.1 + rx * side + (Math.random() - 0.5) * 0.4;
          const sy = states[o + S.pos + 1] - fy * 2.1 + ry * side + (Math.random() - 0.5) * 0.4;
          // Three space: (x, z_up → y, -y → z).
          pos[j] = sx;
          pos[j + 1] = states[o + S.pos + 2] + 0.15;
          pos[j + 2] = -sy;
          const back = 0.25 * speed;
          vel[j] = -fx * back + rx * side * 1.5 + (Math.random() - 0.5) * 1.5;
          vel[j + 1] = 1.2 + Math.random() * 1.8 * s;
          vel[j + 2] = fy * back - ry * side * 1.5 + (Math.random() - 0.5) * 1.5;
          p.max = 0.5 + 0.6 * Math.random();
          p.life = p.max;
        }
      }
    }
    let any = false;
    for (let i = 0; i < SPRAY_MAX; i++) {
      const p = this.particles[i];
      if (p.life <= 0) {
        this.sprayAlpha[i] = 0;
        continue;
      }
      any = true;
      p.life -= dt;
      const j = i * 3;
      vel[j + 1] -= 6 * dt;
      const drag = Math.exp(-2.5 * dt);
      vel[j] *= drag;
      vel[j + 2] *= drag;
      pos[j] += vel[j] * dt;
      pos[j + 1] = Math.max(0.05, pos[j + 1] + vel[j + 1] * dt);
      pos[j + 2] += vel[j + 2] * dt;
      this.sprayAlpha[i] = Math.max(0, p.life / p.max);
    }
    this.spray.visible = any;
    if (any) {
      this.sprayGeo.attributes.position.needsUpdate = true;
      this.sprayGeo.attributes.alpha.needsUpdate = true;
    }
  }

  private updateWipers(show: boolean, info: Float32Array, dt: number): void {
    const rate = info[WX.rainMmH];
    const visible = show && rate > 0.05;
    this.wipers.style.display = visible ? "" : "none";
    if (!visible) return;
    // Intermittent in drizzle, fast in a downpour: sweeps per second.
    const speed = rate > 6 ? 1.1 : rate > 1.5 ? 0.7 : 0.35;
    this.wiperPhase = (this.wiperPhase + dt * speed) % 1;
    const a = -Math.sin(this.wiperPhase * Math.PI) * 78;
    this.blades[0].style.transform = `rotate(${a}deg)`;
    this.blades[1].style.transform = `rotate(${a}deg)`;
  }

  private updateHud(on: boolean, info: Float32Array): void {
    if (on !== this.hudShown) {
      this.hudShown = on;
      this.hud.style.display = on ? "" : "none";
    }
    if (!on) return;
    const name = WEATHER_NAMES[info[WX.kind]] ?? "?";
    const water = info[WX.waterMm];
    const road =
      info[WX.roadC] <= 0 && water > 0.05 ? "icy" :
      info[WX.standingMm] > 0.3 ? `standing water ${info[WX.standingMm].toFixed(1)} mm` :
      info[WX.wetness] > 0.8 ? "wet" :
      info[WX.wetness] > 0.15 ? "damp" : "dry";
    const plane = info[WX.aquaplane];
    const html =
      `${name}${info[WX.locked] > 0.5 ? "" : " (dynamic)"} · ` +
      (info[WX.rainMmH] > 0.05 ? `${info[WX.rainMmH].toFixed(1)} mm/h · ` : "") +
      `vis ${Math.round(info[WX.visibilityM])} m · ${info[WX.airC].toFixed(0)} °C · road <b>${road}</b>` +
      (plane > 0.25 ? ` · <b style="color:#ff5040">AQUAPLANING ${Math.round(plane * 100)}%</b>` : "");
    // DOM writes only on change (it was re-parsed every frame).
    if (html !== this.hudHtml) {
      this.hudHtml = html;
      this.hud.innerHTML = html;
    }
  }
}
