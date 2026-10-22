// Garage workshop mode: the career car on a lift in a garage bay, every part clickable.
//
// The car is a real sandbox session (wp_wasm `career_workshop_session`: the car with its mods
// and saved dents, paused), drawn by the normal VehicleView, so what you see is what you drive.
// This module adds the bay (room.ts), hides the parts of empty slots, picks parts under the
// mouse (PanelBody.partAt → slotmap.ts), highlights slots, moves the camera between preset
// views and mounts the overlay (ui-kit garage/Workshop.tsx). Every part change rebuilds the
// session, since mods change the car's definition.
import * as THREE from "three";
import { WorkshopOverlay, type WorkshopCam, type WorkshopHost } from "../../../ui-kit/src/garage/Workshop";
import type { WorkshopApi, WorkshopScreen } from "../../../ui-kit/src/garage/types";
import type { CameraRig } from "../cameras";
import { DEBUG_LAYERS, layers } from "../settings";
import type { SceneView } from "../scene";
import type { VehicleView } from "../vehicle_view";
import { sim } from "../wasm/sim";
import { GarageRoom, LIFT_HEIGHT_M } from "./room";
import { slotOfPart } from "./slotmap";

/** What the workshop needs from the testbed. */
export interface WorkshopDeps {
  scene: SceneView;
  rig: CameraRig;
  overlayEl: HTMLElement;
  /** The player's vehicle view of the current session (null until built). */
  playerView(): VehicleView | null;
  /** Install a fresh session's views showing career car `uid`'s cosmetic look (main.ts rebuildViews). */
  rebuildViews(uid: number): void;
  /** Flat free-drive scenario JSON (the lot the car stands on). */
  lotScenario(): string;
  /** Save the career after a change. */
  persist(): void;
  /** Leave to the career garage screen. */
  back(): void;
  /** Leave and start a free drive with the car. */
  drive(): void;
  /** Display name of a car model. */
  carName(defId: string): string;
}

/** Camera presets in car space: eye and target (x right, y forward, z up, metres). */
const CAMS: Record<WorkshopCam, { eye: [number, number, number]; at: [number, number, number]; lift: boolean | null }> = {
  overview: { eye: [-3.3, 4.3, 2.3], at: [0, 0, 0.6], lift: false },
  engine: { eye: [0.3, 4.2, 2.2], at: [0, 1.4, 0.55], lift: false },
  under: { eye: [-2.6, -2.2, -0.3], at: [0, 0.2, 0.15], lift: true },
  wheels: { eye: [-2.7, 3.3, 0.8], at: [-0.8, 1.3, 0.35], lift: null },
};

export class WorkshopMode {
  /** The workshop is open. */
  active = false;
  private car = 0;
  private screen: WorkshopScreen | null = null;
  private room: GarageRoom | null = null;
  private readonly overlay: WorkshopOverlay;
  /** The player view the room / hidden parts were set up for. */
  private placedFor: VehicleView | null = null;
  private highlighted: string[] = [];
  private liftTarget = 0;
  private liftT = 0;
  private readonly ray = new THREE.Raycaster();
  private readonly ndc = new THREE.Vector2();
  private down: { x: number; y: number } | null = null;
  private lastCam: WorkshopCam = "overview";
  private readonly listeners: [string, (e: PointerEvent) => void][] = [];

  constructor(private readonly deps: WorkshopDeps) {
    this.overlay = new WorkshopOverlay(deps.overlayEl);
  }

  /** Open the workshop on career car `uid` (throws when the car cannot be shown). */
  open(uid: number): void {
    this.close();
    this.car = uid;
    this.screen = sim.careerWorkshop(uid);
    this.loadSession();
    this.active = true;
    this.deps.scene.setWorkshop(true);
    layers.hold(DEBUG_LAYERS);
    document.body.classList.add("wp-workshop");
    const canvas = this.deps.scene.renderer.domElement;
    const on = (type: string, f: (e: PointerEvent) => void) => {
      canvas.addEventListener(type, f as EventListener);
      this.listeners.push([type, f]);
    };
    on("pointerdown", (e) => (this.down = { x: e.clientX, y: e.clientY }));
    on("pointerup", (e) => {
      const d = this.down;
      this.down = null;
      if (!d || Math.hypot(e.clientX - d.x, e.clientY - d.y) > 5 || e.button !== 0) return;
      this.overlay.state.set({ selected: this.slotAt(e) });
    });
    on("pointermove", (e) => {
      if (this.down) return;
      const s = this.slotAt(e);
      if (s !== this.overlay.state.hover) this.overlay.state.set({ hover: s });
      canvas.style.cursor = s ? "pointer" : "";
    });
    const api: WorkshopApi = {
      screen: () => sim.careerWorkshop(uid),
      remove: (slot) => this.after(sim.careerRemovePart(uid, slot)),
      fit: (slot, part) => this.after(sim.careerFitPart(uid, slot, part)),
      buy: (kind, grade) => this.after(sim.careerBuyPart(uid, kind, grade), false),
      sell: (part) => this.after(sim.careerSellPart(uid, part), false),
      strip: (assembly, slot) => this.after(sim.careerStripPart(uid, assembly, slot), false),
      build: (assembly, slot, part) => this.after(sim.careerBuildPart(uid, assembly, slot, part), false),
    };
    const host: WorkshopHost = {
      highlight: (slots) => this.highlight(slots),
      camera: (p) => this.camera(p),
      lift: (up) => (this.liftTarget = up ? 1 : 0),
      changed: () => {},
      back: () => this.deps.back(),
      drive: () => this.deps.drive(),
      carName: this.deps.carName(this.screen.def_id),
    };
    this.overlay.show(api, host);
  }

  /** Close the workshop (the caller then shows another screen or starts a run). */
  close(): void {
    if (!this.active) return;
    this.active = false;
    this.overlay.hide();
    this.room?.dispose();
    this.room = null;
    this.placedFor = null;
    this.deps.scene.setWorkshop(false);
    layers.hold([]);
    document.body.classList.remove("wp-workshop");
    const canvas = this.deps.scene.renderer.domElement;
    for (const [t, f] of this.listeners) canvas.removeEventListener(t, f as EventListener);
    this.listeners.length = 0;
    canvas.style.cursor = "";
  }

  /** A part change happened: save, and rebuild the car when its build changed. */
  private after(next: WorkshopScreen, rebuild = true): WorkshopScreen {
    this.screen = next;
    this.deps.persist();
    if (rebuild) this.loadSession();
    return next;
  }

  private loadSession(): void {
    sim.careerWorkshopSession(this.car, this.deps.lotScenario());
    this.deps.rebuildViews(this.car);
    this.placedFor = null;
  }

  /** Per frame (after the views updated): place the bay once the car is built, ease the lift. */
  frame(dt: number): void {
    if (!this.active) return;
    const view = this.deps.playerView();
    if (view && view !== this.placedFor && view.panels.ready) this.place(view);
    const k = 1 - Math.exp(-dt * 2.5);
    if (Math.abs(this.liftTarget - this.liftT) > 1e-3) {
      this.liftT += (this.liftTarget - this.liftT) * k;
      this.room?.setLift(this.liftT);
    }
  }

  /** Room around the car, hidden parts of empty slots, camera on the overview. */
  private place(view: VehicleView): void {
    const first = !this.room;
    this.placedFor = view;
    view.root.updateWorldMatrix(true, false);
    // Car position and heading in the sim world frame (the world group is sim space).
    const world = this.deps.scene.world;
    world.updateWorldMatrix(true, false);
    const p = world.worldToLocal(view.root.getWorldPosition(new THREE.Vector3()));
    const fwd = world
      .worldToLocal(view.root.localToWorld(new THREE.Vector3(0, 1, 0)))
      .sub(p);
    const yaw = Math.atan2(fwd.y, fwd.x) - Math.PI / 2;
    if (!this.room) {
      const size = view.info.body_size_m ?? [1.8, 4.5, 1.4];
      this.room = new GarageRoom(size[1], size[0]);
      world.add(this.room.root);
    }
    this.room.root.position.set(p.x, p.y, 0);
    this.room.root.rotation.set(0, 0, yaw);
    this.room.setLift(this.liftT);
    const empty = new Set((this.screen?.slots ?? []).filter((s) => !s.fitted).map((s) => s.id));
    if (empty.size) view.panels.hideParts((def, c) => empty.has(slotOfPart(def, c.x) ?? ""));
    this.highlight(this.highlighted);
    // A rebuilt session resets the camera rig: back to the current preset (the lift stays put).
    this.camera(first ? "overview" : this.lastCam, first);
  }

  private slotAt(e: PointerEvent): string | null {
    const view = this.deps.playerView();
    if (!view?.panels.ready) return null;
    const r = this.deps.scene.renderer.domElement.getBoundingClientRect();
    this.ndc.set(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1);
    this.ray.setFromCamera(this.ndc, this.deps.rig.camera);
    // Bolt-on cosmetic pieces (bull bar, wing, cage...) in front of the panel hit win.
    const hit = view.panels.partHit(this.ray);
    const piece = view.look?.slotAt(this.ray);
    if (piece && (!hit || piece.distance < hit.distance)) return piece.slot;
    if (!hit) return null;
    const centre = view.panels.partDefs().find((d) => d.def === hit.def)?.centre;
    return slotOfPart(hit.def, centre?.x ?? 0);
  }

  private highlight(slots: string[]): void {
    this.highlighted = slots;
    const view = this.deps.playerView();
    if (!view?.panels.ready) return;
    const set = new Set(slots);
    view.panels.highlightParts(set.size ? (def, c) => set.has(slotOfPart(def, c.x) ?? "") : null);
    view.look?.highlight(set);
  }

  /** Move to a preset view (raises or lowers the lift for it unless `moveLift` is false). */
  camera(preset: WorkshopCam, moveLift = true): void {
    this.lastCam = preset;
    const c = CAMS[preset];
    if (moveLift && c.lift !== null) this.liftTarget = c.lift ? 1 : 0;
    const view = this.deps.playerView();
    if (!view) return;
    // The car never moves: "lift up" lowers the bay, so an underside view looks from below z = 0.
    const lift = c.lift ? LIFT_HEIGHT_M : 0;
    const toSim = (v: [number, number, number]): [number, number, number] => {
      const w = view.root.localToWorld(new THREE.Vector3(v[0], v[1], v[2]));
      // Three (x, y, z) → sim (x, -z, y).
      return [w.x, -w.z, w.y];
    };
    const eye = toSim([c.eye[0], c.eye[1], c.eye[2] + (preset === "under" ? -lift * 0.4 : 0)]);
    this.deps.rig.lookFrom(eye, toSim(c.at));
  }

  /** Last preset (automation). */
  get view(): WorkshopCam {
    return this.lastCam;
  }

  /** Current screen (automation / tests). */
  current(): WorkshopScreen | null {
    return this.screen;
  }

  /** Automation: the slot under a screen point, or null. */
  slotAtPoint(x: number, y: number): string | null {
    return this.slotAt({ clientX: x, clientY: y } as PointerEvent);
  }

  /** Automation: screen point (CSS px) over the centre of a slot's first visible part, or null. */
  pointOfSlot(slot: string): { x: number; y: number } | null {
    const view = this.deps.playerView();
    if (!view?.panels.ready) return null;
    const r = this.deps.scene.renderer.domElement.getBoundingClientRect();
    for (const { def, centre } of view.panels.partDefs()) {
      if (slotOfPart(def, centre.x) !== slot || def.wheel != null) continue;
      const w = view.root.localToWorld(centre.clone()).project(this.deps.rig.camera);
      const pt = { x: r.left + ((w.x + 1) / 2) * r.width, y: r.top + ((1 - w.y) / 2) * r.height };
      if (this.slotAtPoint(pt.x, pt.y) === slot) return pt;
    }
    return null;
  }
}
