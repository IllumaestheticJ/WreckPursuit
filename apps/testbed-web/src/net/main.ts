// Multiplayer page (net.html): connects to wp-server, predicts the own car through the wasm
// client, interpolates everyone else, and switches to the captain map for the captain role.
//   http://localhost:5173/net.html?server=ws://localhost:8080&name=me
import * as THREE from "three";
import initWasm from "../wasm/pkg/wp_wasm.js";
import * as net from "./wasm";
import type { RenderVehicle } from "./wasm";
import { Connection } from "./connection";
import { installKeyboard, readInput } from "./keyboard";
import { renderOverlay } from "./overlay";
import { LobbyPanel } from "./rolePicker";
import { CaptainMap } from "./captainMap";

const COLORS: Record<string, number> = {
  racer: 0xd04a2a,
  police: 0x2a5ad0,
  ai_police: 0x4a7ad0,
  traffic: 0x8a8a8a,
};

async function main(): Promise<void> {
  const params = new URLSearchParams(location.search);
  const url = params.get("server") ?? `ws://${location.hostname}:8080`;
  const name = params.get("name") ?? `web${Math.floor(Math.random() * 1000)}`;
  await initWasm();
  net.create(name, 0);

  const overlay = document.getElementById("overlay")!;
  const panel = document.getElementById("panel")!;

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.setSize(window.innerWidth, window.innerHeight);
  document.body.prepend(renderer.domElement);
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x202428);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x333333, 2.0));
  const sun = new THREE.DirectionalLight(0xffffff, 1.5);
  sun.position.set(30, 60, 20);
  scene.add(sun);
  // Sim space is +Z up; Three is +Y up.
  const world = new THREE.Group();
  world.rotation.x = -Math.PI / 2;
  scene.add(world);
  world.add(new THREE.Mesh(new THREE.PlaneGeometry(4000, 4000),
    new THREE.MeshStandardMaterial({ color: 0x3a3f44 })));
  const grid = new THREE.GridHelper(4000, 400, 0xaaaaaa, 0x6c7278);
  grid.position.y = 0.02;
  scene.add(grid);
  const camera = new THREE.PerspectiveCamera(60, window.innerWidth / window.innerHeight, 0.1, 5000);
  window.addEventListener("resize", () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(window.innerWidth, window.innerHeight);
  });

  const cars = new Map<number, THREE.Group>();
  function carObject(v: RenderVehicle): THREE.Group {
    let obj = cars.get(v.entity);
    if (obj) return obj;
    const [w, l, h] = v.body_size_m;
    obj = new THREE.Group();
    const body = new THREE.Mesh(new THREE.BoxGeometry(w, l, h),
      new THREE.MeshStandardMaterial({ color: v.own ? 0xffaa22 : COLORS[v.kind] ?? 0xffffff }));
    body.position.z = h / 2;
    const nose = new THREE.Mesh(new THREE.BoxGeometry(w * 0.6, 0.3, 0.1),
      new THREE.MeshStandardMaterial({ color: 0xffee88 }));
    nose.position.set(0, l / 2 - 0.2, h + 0.05);
    obj.add(body, nose);
    world.add(obj);
    cars.set(v.entity, obj);
    return obj;
  }

  let connState = "connecting";
  const conn = new Connection(url, (s) => (connState = s));
  conn.open();
  window.addEventListener("beforeunload", () => conn.close());
  installKeyboard();

  let ownCar: RenderVehicle | undefined;
  const lobby = new LobbyPanel(panel, () => ownCar);
  const captain = new CaptainMap(document.body);
  captain.visible = false;

  const camPos = new THREE.Vector3(0, 6, 12);
  const camTarget = new THREE.Vector3();
  let last = performance.now();
  let lastUi = 0;

  function frame(now: number): void {
    const dt = Math.min((now - last) / 1000, 0.1);
    last = now;
    net.update(now, readInput());
    conn.flush();

    const st = net.status();
    const isCaptain = st.role === "police_captain";
    captain.visible = isCaptain;
    renderer.domElement.style.display = isCaptain ? "none" : "block";

    if (isCaptain) {
      captain.draw(net.captainMap());
    } else {
      const list = net.render(now);
      const seen = new Set<number>();
      ownCar = undefined;
      for (const v of list) {
        seen.add(v.entity);
        const obj = carObject(v);
        obj.position.set(v.pos[0], v.pos[1], v.pos[2]);
        obj.quaternion.set(v.rot[0], v.rot[1], v.rot[2], v.rot[3]);
        if (v.own) ownCar = v;
      }
      for (const [e, obj] of cars) {
        if (!seen.has(e)) {
          world.remove(obj);
          cars.delete(e);
        }
      }
      const follow = ownCar ? cars.get(ownCar.entity) : cars.values().next().value;
      if (follow) {
        const p = follow.getWorldPosition(new THREE.Vector3());
        const back = new THREE.Vector3(0, -1, 0).applyQuaternion(follow.getWorldQuaternion(new THREE.Quaternion()));
        const desired = p.clone().addScaledVector(back, 10).add(new THREE.Vector3(0, 4, 0));
        camPos.lerp(desired, 1 - Math.exp(-dt * 4));
        camTarget.lerp(p, 1 - Math.exp(-dt * 8));
      }
      camera.position.copy(camPos);
      camera.lookAt(camTarget);
      renderer.render(scene, camera);
    }

    if (now - lastUi > 200) {
      lastUi = now;
      renderOverlay(overlay, st, net.stats(), connState);
      lobby.update(st);
    }
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}

main().catch((e) => {
  document.getElementById("overlay")!.textContent = `failed to start: ${e}`;
  console.error(e);
});
