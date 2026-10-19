// WreckPursuit browser testbed shell: ground plane + one box per vehicle, driven by wp_wasm.
// Sim space is +Z up / +Y forward; Three.js is +Y up, so the scene root is rotated -90° about X.
import * as THREE from "three";
import { initSim, resetVehicle, setInput, stepSim, vehicleInfo, vehicleStates } from "./wasm/sim";
import { installKeyboard, readInput } from "./input";

const hud = document.getElementById("hud")!;

async function main(): Promise<void> {
  await initSim();

  const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.setSize(window.innerWidth, window.innerHeight);
  document.body.appendChild(renderer.domElement);

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x202428);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x333333, 2.0));
  const sun = new THREE.DirectionalLight(0xffffff, 1.5);
  sun.position.set(30, 60, 20);
  scene.add(sun);

  // Everything in sim coordinates goes under `world` (Z-up → Y-up).
  const world = new THREE.Group();
  world.rotation.x = -Math.PI / 2;
  scene.add(world);

  const ground = new THREE.Mesh(
    new THREE.PlaneGeometry(2000, 2000),
    new THREE.MeshStandardMaterial({ color: 0x3a3f44 }),
  );
  world.add(ground);
  const grid = new THREE.GridHelper(2000, 200, 0xaaaaaa, 0x6c7278);
  grid.position.y = 0.02; // GridHelper is natively in Three's XZ ground plane.
  scene.add(grid);

  const boxes = new Map<number, THREE.Object3D>();
  for (const info of vehicleInfo()) {
    const [w, l, h] = info.body_size_m;
    const body = new THREE.Mesh(
      new THREE.BoxGeometry(w, l, h),
      new THREE.MeshStandardMaterial({ color: 0xd04a2a }),
    );
    body.position.z = h / 2;
    // Nose marker so heading is obvious.
    const nose = new THREE.Mesh(
      new THREE.BoxGeometry(w * 0.6, 0.3, 0.1),
      new THREE.MeshStandardMaterial({ color: 0xffee88 }),
    );
    nose.position.set(0, l / 2 - 0.2, h + 0.05);
    const car = new THREE.Group();
    car.add(body, nose);
    world.add(car);
    boxes.set(info.entity, car);
  }

  const camera = new THREE.PerspectiveCamera(60, window.innerWidth / window.innerHeight, 0.1, 5000);
  window.addEventListener("resize", () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(window.innerWidth, window.innerHeight);
  });

  installKeyboard(resetVehicle);

  let last = performance.now();
  const camTarget = new THREE.Vector3();
  const camPos = new THREE.Vector3(0, 6, 12);

  function frame(now: number): void {
    const dt = Math.min((now - last) / 1000, 0.1);
    last = now;
    setInput(readInput());
    stepSim(dt);

    let speed = 0;
    let lines = "";
    for (const { entity, state } of vehicleStates()) {
      const obj = boxes.get(entity);
      if (!obj) continue;
      obj.position.set(state.pos[0], state.pos[1], state.pos[2]);
      obj.quaternion.set(state.rot[0], state.rot[1], state.rot[2], state.rot[3]);
      speed = state.speed_mps;
      lines += `car ${entity}: ${(state.speed_mps * 3.6).toFixed(0)} km/h  gear ${state.gear}  ` +
        `${state.engine_rpm.toFixed(0)} rpm  health ${(state.health * 100).toFixed(0)}%\n`;

      // Simple chase camera behind the first car (computed in sim space, then mapped).
      const worldPos = obj.getWorldPosition(new THREE.Vector3());
      const back = new THREE.Vector3(0, -1, 0).applyQuaternion(obj.getWorldQuaternion(new THREE.Quaternion()));
      const desired = worldPos.clone().addScaledVector(back, 10).add(new THREE.Vector3(0, 4, 0));
      camPos.lerp(desired, 1 - Math.exp(-dt * 4));
      camTarget.lerp(worldPos, 1 - Math.exp(-dt * 8));
    }
    camera.position.copy(camPos);
    camera.lookAt(camTarget);
    renderer.render(scene, camera);
    hud.textContent = `WreckPursuit testbed (stub)\narrows/WASD drive · space handbrake · R reset\n${lines}`;
    (window as unknown as { __wpSpeed: number }).__wpSpeed = speed;
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}

main().catch((e) => {
  hud.textContent = `failed to start: ${e}`;
  console.error(e);
});
