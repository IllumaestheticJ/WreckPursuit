// Kinematic traffic drawn as the real cars of the traffic fleet (data/fleets/traffic.toml).
//
// Each fleet model's panel mesh (data/panels/meshes/<car>.json) is merged at rest — wheels
// placed on their hubs (data/cars/<car>.json), tandem axles included — into three instanced
// meshes: painted bodywork (tinted per agent from the car's own palette by its variant),
// everything else with baked vertex colours, and glass. So a city full of traffic costs three
// draw calls per model in view. Physical (near-player) agents are full VehicleViews instead.
//
// Coordinates: chassis space (+X right, +Y forward, +Z up); the parent is the scene's world
// group, which maps that to Three's Y-up.
import * as THREE from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import { AGENT_FLAG, AGENT_STRIDE } from "../wasm/layout";
import { loadCarMesh, type CarMeshJson, type MaterialSlot } from "./panels";

/** Instances per model (an agent beyond this is not drawn; the agent cap is 400). */
const MAX_PER_MODEL = 400;

/** Baked colour per non-paint material slot. */
const SLOT_COLOUR: Partial<Record<MaterialSlot, number>> = {
  trim: 0x1b1b1d,
  chrome: 0xc9cdd2,
  metal: 0x8e959c,
  headlight: 0xeeeadc,
  taillight: 0x8a0c10,
  indicator: 0xc97a10,
  lens: 0xd8dce0,
  beacon_red: 0x8a1020,
  beacon_blue: 0x10288a,
  plate: 0xe2ddcc,
  rubber: 0x151515,
  interior: 0x2b2724,
};

interface CarJson {
  wheels: { position_m: [number, number, number] }[];
}

/** Three instanced meshes of one fleet model plus its livery colours. */
interface ModelView {
  paint: THREE.InstancedMesh;
  rest: THREE.InstancedMesh;
  glass: THREE.InstancedMesh;
  colours: THREE.Color[];
  count: number;
}

function partGeometry(positionsMm: number[], indices: number[], offset?: THREE.Vector3, colour?: THREE.Color): THREE.BufferGeometry {
  const n = positionsMm.length / 3;
  const pos = new Float32Array(n * 3);
  for (let i = 0; i < n * 3; i++) pos[i] = positionsMm[i] * 1e-3;
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  g.setIndex(indices);
  if (offset) g.translate(offset.x, offset.y, offset.z);
  g.computeVertexNormals();
  const col = new Float32Array(n * 3);
  const c = colour ?? new THREE.Color(1, 1, 1);
  for (let i = 0; i < n; i++) c.toArray(col, i * 3);
  g.setAttribute("color", new THREE.BufferAttribute(col, 3));
  return g;
}

/** Merge a car mesh at rest into (paint, rest, glass) geometries. */
function bakeModel(mesh: CarMeshJson, car: CarJson): [THREE.BufferGeometry, THREE.BufferGeometry, THREE.BufferGeometry] {
  const paint: THREE.BufferGeometry[] = [];
  const rest: THREE.BufferGeometry[] = [];
  const glass: THREE.BufferGeometry[] = [];
  const accent = new THREE.Color(mesh.palette?.accent ?? 0xe8e8e4);
  const primer = new THREE.Color(mesh.palette?.primer ?? 0x6d6f6a);
  for (const p of mesh.parts) {
    let offset: THREE.Vector3 | undefined;
    if (p.wheel != null) {
      const hub = car.wheels[p.wheel]?.position_m ?? [0, 0, 0];
      offset = new THREE.Vector3(...hub);
      if (p.hub_offset_mm) offset.add(new THREE.Vector3(...p.hub_offset_mm).multiplyScalar(1e-3));
    }
    if (p.material === "paint") {
      paint.push(partGeometry(p.positions_mm, p.indices, offset));
    } else if (p.material === "glass" || p.material === "glass_dark") {
      glass.push(partGeometry(p.positions_mm, p.indices, offset, new THREE.Color(p.material === "glass" ? 0x9fbfcf : 0x1c2228)));
    } else {
      const c =
        p.material === "accent"
          ? accent
          : p.material === "primer"
            ? primer
            : new THREE.Color(SLOT_COLOUR[p.material] ?? 0x1b1b1d);
      rest.push(partGeometry(p.positions_mm, p.indices, offset, c));
    }
  }
  const merge = (gs: THREE.BufferGeometry[]) => (gs.length ? mergeGeometries(gs, false) : new THREE.BufferGeometry());
  return [merge(paint), merge(rest), merge(glass)];
}

/** Every traffic fleet model, instanced. */
export class TrafficModels {
  readonly group = new THREE.Group();
  private models: (ModelView | null)[] = [];
  private carIds: string[] = [];
  private readonly paintMat = new THREE.MeshStandardMaterial({ color: 0xffffff, metalness: 0.3, roughness: 0.45, side: THREE.DoubleSide });
  private readonly restMat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.6, side: THREE.DoubleSide });
  private readonly glassMat = new THREE.MeshStandardMaterial({
    vertexColors: true,
    transparent: true,
    opacity: 0.45,
    roughness: 0.1,
    depthWrite: false,
    side: THREE.DoubleSide,
  });
  private readonly m = new THREE.Matrix4();
  private readonly q = new THREE.Quaternion();
  private readonly p = new THREE.Vector3();
  private readonly one = new THREE.Vector3(1, 1, 1);
  private readonly z = new THREE.Vector3(0, 0, 1);

  /** Use this fleet (car ids in model order); loads each model's mesh once. */
  setFleet(carIds: string[]): void {
    if (carIds.join(",") === this.carIds.join(",")) return;
    this.carIds = carIds;
    for (const mv of this.models) if (mv) for (const im of [mv.paint, mv.rest, mv.glass]) im.removeFromParent();
    this.models = carIds.map(() => null);
    carIds.forEach((id, i) => {
      void Promise.all([loadCarMesh(id), fetch(`data/cars/${id}.json`).then((r) => (r.ok ? (r.json() as Promise<CarJson>) : null))]).then(
        ([mesh, car]) => {
          if (!mesh || !car || this.carIds[i] !== id) return;
          const [pg, rg, gg] = bakeModel(mesh, car);
          const make = (g: THREE.BufferGeometry, mat: THREE.Material) => {
            const im = new THREE.InstancedMesh(g, mat, MAX_PER_MODEL);
            im.count = 0;
            im.frustumCulled = false;
            this.group.add(im);
            return im;
          };
          const paint = make(pg, this.paintMat);
          paint.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(MAX_PER_MODEL * 3), 3);
          const glass = make(gg, this.glassMat);
          glass.renderOrder = 1;
          const colours = [mesh.palette?.base ?? 0x8a8f96, ...(mesh.palette?.alts ?? [])].map((c) => new THREE.Color(c));
          this.models[i] = { paint, rest: make(rg, this.restMat), glass, colours, count: 0 };
        },
      );
    });
  }

  /** True once there is a fleet (callers fall back to boxes otherwise). */
  get active(): boolean {
    return this.carIds.length > 0;
  }

  /** True if the agent's model is ready to draw. */
  ready(model: number): boolean {
    return this.models[model] != null;
  }

  /**
   * Place every kinematic agent ([x, y, z, yaw, flags] per agent, model in flags >> 16).
   * Returns the agents not drawn here (model not loaded yet), as indices, for a fallback.
   */
  update(agents: Float32Array, skipPhysical: boolean, show: boolean): number[] {
    const missing: number[] = [];
    for (const mv of this.models) if (mv) mv.count = 0;
    this.group.visible = show;
    if (!show) return missing;
    const total = agents.length / AGENT_STRIDE;
    for (let a = 0; a < total; a++) {
      const o = a * AGENT_STRIDE;
      const flags = agents[o + 4];
      if (skipPhysical && flags & AGENT_FLAG.physical) continue;
      const model = Math.floor(flags / 65536);
      const mv = this.models[model];
      if (!mv) {
        missing.push(a);
        continue;
      }
      if (mv.count >= MAX_PER_MODEL) continue;
      const variant = Math.floor(flags / 256) % 256;
      this.q.setFromAxisAngle(this.z, agents[o + 3]);
      this.p.set(agents[o], agents[o + 1], agents[o + 2]);
      this.m.compose(this.p, this.q, this.one);
      const k = mv.count++;
      mv.paint.setMatrixAt(k, this.m);
      mv.rest.setMatrixAt(k, this.m);
      mv.glass.setMatrixAt(k, this.m);
      mv.paint.setColorAt(k, mv.colours[variant % mv.colours.length]);
    }
    for (const mv of this.models) {
      if (!mv) continue;
      for (const im of [mv.paint, mv.rest, mv.glass]) {
        im.count = mv.count;
        im.instanceMatrix.needsUpdate = true;
      }
      if (mv.paint.instanceColor) mv.paint.instanceColor.needsUpdate = true;
    }
    return missing;
  }
}
