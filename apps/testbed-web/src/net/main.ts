// Multiplayer page (net.html): connects to wp-server, predicts the own car through the wasm
// client, interpolates everyone else, and switches to the captain map for the captain role.
//   http://localhost:5173/net.html?server=ws://localhost:8080&name=me[&role=racer]
// Each car shows its replicated damage lattice as points (dents appear where it was hit).
// `window.__wpNet` is a small hook for the two-tab e2e check (e2e/net_two_tabs.mjs).
import * as THREE from "three";
import initWasm from "../wasm/pkg/wp_wasm.js";
import * as net from "./wasm";
import type { DamageInfo, GameEvent, NetInput, RenderVehicle, RoleName } from "./wasm";
import { Connection } from "./connection";
import { installKeyboard, readInput } from "./keyboard";
import { renderOverlay } from "./overlay";
import { LobbyPanel } from "./rolePicker";
import { CaptainMap } from "./captainMap";

/** Contract `traffic_flags` bits of a `TrafficAgentSnapshot` (wp_contracts net.rs). */
const TRAFFIC_FLAG = { hazards: 2, left: 4, right: 8 } as const;
/** Indicator blink (as the main testbed: 1.5 Hz, 50 % duty, every car in phase). */
const indicatorOn = (nowMs: number) => ((nowMs / 1000) * 1.5) % 1 < 0.5;

const COLORS: Record<string, number> = {
  racer: 0xd04a2a,
  police: 0x2a5ad0,
  ai_police: 0x4a7ad0,
  traffic: 0x8a8a8a,
  ai_racer: 0xd08a2a,
};

async function main(): Promise<void> {
  const params = new URLSearchParams(location.search);
  const url = params.get("server") ?? `ws://${location.hostname}:8080`;
  const name = params.get("name") ?? `web${Math.floor(Math.random() * 1000)}`;
  const autoRole = params.get("role") as RoleName | null;
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
    // Translucent body so the replicated damage lattice inside shows its dents.
    const body = new THREE.Mesh(new THREE.BoxGeometry(w, l, h),
      new THREE.MeshStandardMaterial({
        color: v.own ? 0xffaa22 : COLORS[v.kind] ?? 0xffffff,
        transparent: true,
        opacity: 0.35,
        depthWrite: false,
      }));
    body.position.z = h / 2;
    const nose = new THREE.Mesh(new THREE.BoxGeometry(w * 0.6, 0.3, 0.1),
      new THREE.MeshStandardMaterial({ color: 0xffee88 }));
    nose.position.set(0, l / 2 - 0.2, h + 0.05);
    // The replicated damage lattice: refreshed from the client's replica a few times a second.
    const pts = new THREE.Points(new THREE.BufferGeometry(),
      new THREE.PointsMaterial({ color: 0xffffff, size: 0.09 }));
    pts.name = "lattice";
    obj.add(body, nose, pts);
    world.add(obj);
    cars.set(v.entity, obj);
    return obj;
  }

  function refreshLattices(): void {
    for (const [e, obj] of cars) {
      const pts = obj.getObjectByName("lattice") as THREE.Points | undefined;
      const nodes = net.latticeNodes(e);
      if (!pts || nodes.length === 0) continue;
      pts.geometry.setAttribute("position", new THREE.BufferAttribute(nodes, 3));
      pts.geometry.computeBoundingSphere();
    }
  }

  // Test hook (e2e/net_two_tabs.mjs): read state, steer the car.
  let lastRender: RenderVehicle[] = [];
  let testInput: NetInput | null = null;
  (window as unknown as { __wpNet: unknown }).__wpNet = {
    status: () => net.status(),
    stats: () => net.stats(),
    render: () => lastRender,
    damage: (): DamageInfo[] => net.damage(),
    requestRole: (role: RoleName) => net.requestRole(role, performance.now()),
    setInput: (i: NetInput | null) => (testInput = i),
    dropConnection: () => conn.dropForTest(),
    connState: () => connState,
  };

  // Kinematic traffic (display only, extrapolated by the client).
  const trafficCars = new Map<number, THREE.Mesh>();
  const trafficGeo = new THREE.BoxGeometry(1.8, 4.5, 1.4);
  const trafficMat = new THREE.MeshStandardMaterial({ color: COLORS.traffic });
  // Indicator lamps on the box corners (children 0, 1 left front / rear, 2, 3 right): lit amber
  // or dark lens, blinking from the snapshot's LEFT_SIGNAL / RIGHT_SIGNAL / HAZARDS flags.
  const lampGeo = new THREE.BoxGeometry(0.2, 0.24, 0.14);
  const lampOn = new THREE.MeshBasicMaterial({ color: new THREE.Color(0xffa030).multiplyScalar(1.6) });
  const lampOff = new THREE.MeshBasicMaterial({ color: 0x3a2a12 });
  const addLamps = (m: THREE.Mesh): void => {
    for (const x of [-0.85, 0.85]) {
      for (const y of [2.18, -2.18]) {
        const lamp = new THREE.Mesh(lampGeo, lampOff);
        lamp.position.set(x, y, -0.15);
        m.add(lamp);
      }
    }
  };
  const radio: string[] = [];
  function onEvent(e: GameEvent): void {
    if ("Police" in e && typeof e.Police === "object" && "Radio" in e.Police) {
      const r = e.Police.Radio as { text: string };
      radio.push(r.text);
      while (radio.length > 3) radio.shift();
    }
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
  let roleRequested = false;

  function frame(now: number): void {
    const dt = Math.min((now - last) / 1000, 0.1);
    last = now;
    net.update(now, testInput ?? readInput());
    conn.flush();

    const st = net.status();
    if (autoRole && st.phase === "joined" && st.role === "spectator" && !roleRequested) {
      roleRequested = true;
      net.requestRole(autoRole, now);
    }
    const isCaptain = st.role === "police_captain";
    captain.visible = isCaptain;
    renderer.domElement.style.display = isCaptain ? "none" : "block";

    if (isCaptain) {
      captain.draw(net.captainMap());
    } else {
      const list = net.render(now);
      lastRender = list;
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
      const agents = net.traffic(now);
      const live = new Set<number>();
      for (const a of agents) {
        live.add(a.id);
        let m = trafficCars.get(a.id);
        if (!m) {
          m = new THREE.Mesh(trafficGeo, trafficMat);
          addLamps(m);
          world.add(m);
          trafficCars.set(a.id, m);
        }
        m.position.set(a.pos[0], a.pos[1], a.pos[2] + 0.7);
        m.rotation.set(0, 0, a.yaw);
        const blink = indicatorOn(now);
        const hz = (a.flags & TRAFFIC_FLAG.hazards) !== 0;
        const left = blink && (hz || (a.flags & TRAFFIC_FLAG.left) !== 0);
        const right = blink && (hz || (a.flags & TRAFFIC_FLAG.right) !== 0);
        const lamps = m.children as THREE.Mesh[];
        lamps[0].material = lamps[1].material = left ? lampOn : lampOff;
        lamps[2].material = lamps[3].material = right ? lampOn : lampOff;
      }
      for (const [id, m] of trafficCars) {
        if (!live.has(id)) {
          world.remove(m);
          trafficCars.delete(id);
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
      for (const e of net.events()) onEvent(e);
      if (!isCaptain) refreshLattices();
      const reply = st.captain_replies[st.captain_replies.length - 1];
      if (reply) captain.lastReply = `#${reply.seq} ${reply.ok ? "ok" : JSON.stringify(reply.error)}`;
      renderOverlay(overlay, st, net.stats(), connState, radio, trafficCars.size);
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
