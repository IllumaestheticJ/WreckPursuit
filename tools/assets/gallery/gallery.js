// Car gallery: renders one car's generated mesh (data/panels/meshes/<car>.json) at rest with
// its wheels, in its palette, from a few camera angles, day or night. Driven by
// tools/assets/gallery.mjs (headless) via window.showCar(id, view, night).
// Chassis space: +X right, +Y forward, +Z up (the root group maps it to Three's Y-up).
import * as THREE from "three";

const W = 960;
const H = 600;
const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
renderer.setSize(W, H);
renderer.setPixelRatio(1);
document.body.appendChild(renderer.domElement);
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(30, W / H, 0.1, 200);
const hemi = new THREE.HemisphereLight(0xffffff, 0x404040, 2.2);
const sun = new THREE.DirectionalLight(0xffffff, 2.2);
sun.position.set(-6, 10, 8);
scene.add(hemi, sun);
const ground = new THREE.Mesh(new THREE.PlaneGeometry(200, 200), new THREE.MeshStandardMaterial({ color: 0x4a4f55, roughness: 1 }));
ground.rotation.x = -Math.PI / 2;
scene.add(ground);
const root = new THREE.Group();
root.rotation.x = -Math.PI / 2; // chassis +Z up → Three +Y up; chassis +Y forward → Three −Z
scene.add(root);

const hex = (n) => new THREE.Color(n);
function materials(pal, night) {
  const std = (o) => new THREE.MeshStandardMaterial({ side: THREE.DoubleSide, ...o });
  const glow = (c, e, on) => std({ color: c, emissive: on ? e : 0x000000, emissiveIntensity: on ? 1.6 : 0, roughness: 0.3 });
  return {
    paint: std({ color: hex(pal.base), metalness: 0.3, roughness: 0.45 }),
    accent: std({ color: hex(pal.accent), metalness: 0.3, roughness: 0.45 }),
    primer: std({ color: hex(pal.primer), metalness: 0.0, roughness: 0.95 }),
    glass: std({ color: 0x8fb3c4, transparent: true, opacity: 0.45, metalness: 0.1, roughness: 0.1, depthWrite: false }),
    glass_dark: std({ color: 0x1c2228, transparent: true, opacity: 0.85, metalness: 0.2, roughness: 0.1 }),
    trim: std({ color: 0x1b1b1d, roughness: 0.85 }),
    chrome: std({ color: 0xe4e8ec, metalness: 0.55, roughness: 0.18 }),
    metal: std({ color: 0x9aa1a8, metalness: 0.45, roughness: 0.4 }),
    headlight: glow(0xf4f1e6, 0xfff2c0, night),
    taillight: glow(0xb0141a, 0xff1a10, night),
    indicator: glow(0xd9820f, 0xff9a10, false),
    lens: std({ color: 0xe8ecef, roughness: 0.15, metalness: 0.2 }),
    beacon_red: glow(0x8a1020, 0xff2040, night),
    beacon_blue: glow(0x10288a, 0x2060ff, night),
    plate: std({ color: 0xece8d8, roughness: 0.6 }),
    rubber: std({ color: 0x151515, roughness: 0.95 }),
    interior: std({ color: 0x2b2724, roughness: 1 }),
  };
}

function geom(part) {
  const g = new THREE.BufferGeometry();
  const pos = new Float32Array(part.positions_mm.length);
  for (let i = 0; i < pos.length; i++) pos[i] = part.positions_mm[i] * 1e-3;
  g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  g.setIndex(part.indices);
  g.computeVertexNormals();
  return g;
}

let current = null;
window.showCar = async (id, view, night) => {
  if (current) root.remove(current);
  const [mesh, car] = await Promise.all([
    fetch(`/data/panels/meshes/${id}.json`).then((r) => r.json()),
    fetch(`/data/cars/${id}.json`).then((r) => r.json()),
  ]);
  const mats = materials(mesh.palette, night);
  const g = new THREE.Group();
  for (const p of mesh.parts) {
    const m = new THREE.Mesh(geom(p), mats[p.material] ?? mats.trim);
    if (p.material === "glass") m.renderOrder = 1;
    if (p.wheel != null) {
      const w = car.wheels[p.wheel];
      m.position.set(...w.position_m);
      if (p.hub_offset_mm) m.position.add(new THREE.Vector3(...p.hub_offset_mm).multiplyScalar(1e-3));
      m.rotation.x = 0.4; // show spokes off-axis
    }
    g.add(m);
  }
  root.add(g);
  current = g;
  const [w, l, h] = car.body_size_m;
  const d = Math.max(l, 2.2 * h) * 1.55 + 2;
  const target = new THREE.Vector3(0, h * 0.42, 0);
  // Chassis +Y forward → Three −Z; +X right → Three +X.
  const views = {
    front34: [-0.8, 0.45, -1.0], // front-left three-quarter
    rear34: [0.85, 0.42, 0.95],
    side: [-1.0, 0.18, 0.0],
    front: [0.0, 0.22, -1.0],
    rear: [0.0, 0.25, 1.0],
    top: [-0.25, 1.4, -0.25],
  };
  const v = new THREE.Vector3(...views[view]).normalize().multiplyScalar(d);
  camera.position.copy(target).add(v);
  camera.lookAt(target);
  scene.background = hex(night ? 0x0a0d14 : 0x9cb8d0);
  hemi.intensity = night ? 0.25 : 2.2;
  sun.intensity = night ? 0.15 : 2.2;
  renderer.render(scene, camera);
  return { parts: mesh.parts.length, tris: mesh.tri_count };
};
window.galleryReady = true;
