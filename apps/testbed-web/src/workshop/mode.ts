// Garage workshop mode: the career car on a lift in a garage bay, every part clickable.
//
// The car is a real sandbox session (wp_wasm `career_workshop_session`: the car with its mods
// and saved dents, paused), drawn by the normal VehicleView, so what you see is what you drive.
// This module adds the bay (room.ts), hides the parts of empty slots, picks parts under the
// mouse (PanelBody.partAt → slotmap.ts), highlights slots, moves the camera between preset
// views and mounts the overlay (ui-kit garage/Workshop.tsx). Every part change rebuilds the
// session, since mods change the car's definition. A loose engine stands on the engine stand
// beside the car (engine.ts), drawn from its internals and clickable part by part.
import * as THREE from "three";
import { WorkshopOverlay, type WorkshopCam, type WorkshopHost } from "../../../ui-kit/src/garage/Workshop";
import type { WorkshopApi, WorkshopScreen } from "../../../ui-kit/src/garage/types";
import type { CameraRig } from "../cameras";
import { DEBUG_LAYERS, layers } from "../settings";
import type { SceneView } from "../scene";
import type { VehicleView } from "../vehicle_view";
import { sim } from "../wasm/sim";
import { buildEngine, buildStand, STAND_CRANK_Z } from "./engine";
import { GarageRoom, LIFT_HEIGHT_M } from "./room";
import { loadCarMesh } from "../render/panels";
import { loadVisuals } from "../render/cosmetics/visuals";
import { Bay, buildHardware, HARDWARE_SLOTS, layoutOf } from "./hardware";
import { partHosts } from "./mounts";
import { pickSlot, REGION_SLOT, slotOfPart, slotsNeeded } from "./slotmap";
import { withSounds } from "./sfx";
import type { GarageSound } from "../audio/garage_sfx";

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
  /** Garage sounds (audio/garage_sfx.ts): one per workshop action, and the room tone while open. */
  sound?(s: GarageSound): void;
  ambience?(on: boolean): void;
}

/** Camera presets in car space: eye and target (x right, y forward, z up, metres). */
const CAMS: Record<WorkshopCam, { eye: [number, number, number]; at: [number, number, number]; lift: boolean | null }> = {
  overview: { eye: [-3.3, 4.3, 2.3], at: [0, 0, 0.6], lift: false },
  engine: { eye: [0.3, 4.2, 2.2], at: [0, 1.4, 0.55], lift: false },
  rear: { eye: [2.3, -4.7, 1.8], at: [0, -1.5, 0.55], lift: false },
  under: { eye: [-2.6, -2.2, -0.3], at: [0, 0.2, 0.15], lift: true },
  wheels: { eye: [-2.7, 3.3, 0.8], at: [-0.8, 1.3, 0.35], lift: null },
  stand: { eye: [2.05, -1.75, 1.85], at: [2.9, -0.65, 0.95], lift: false },
};

/** Where the engine stand stands in the bay (car space, metres). */
const STAND_AT = new THREE.Vector3(2.9, -0.9, 0);
const STAND_PREFIX = "stand:";

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
  /** Hidden hardware drawn on the car (hardware.ts), and the look the removed panels were applied to. */
  private hardware: THREE.Group | null = null;
  private hardwareGlow: THREE.Mesh[] = [];
  private lookFor: unknown = null;
  private removedRegions: ReadonlySet<string> = new Set();
  private liftTarget = 0;
  private liftT = 0;
  private readonly ray = new THREE.Raycaster();
  private readonly ndc = new THREE.Vector2();
  private down: { x: number; y: number } | null = null;
  private lastCam: WorkshopCam = "overview";
  private readonly listeners: [string, (e: PointerEvent) => void][] = [];
  /** The engine stand and the loose engine on it (in the room's floor group). */
  private stand: THREE.Group | null = null;
  private engine: THREE.Group | null = null;
  private engineKey = "";
  private standGlow: THREE.Mesh[] = [];
  private readonly glowMat = new THREE.MeshBasicMaterial({ color: 0x39ff88, transparent: true, opacity: 0.45, depthTest: false, depthWrite: false });

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
      const s = this.slotAt(e);
      if (s?.startsWith(STAND_PREFIX)) this.overlay.state.set({ standSelected: s.slice(STAND_PREFIX.length) });
      else this.overlay.state.set({ selected: s });
    });
    on("pointermove", (e) => {
      if (this.down) return;
      const s = this.slotAt(e);
      const stand = s?.startsWith(STAND_PREFIX) ? s.slice(STAND_PREFIX.length) : null;
      const car = stand ? null : s;
      const st = this.overlay.state;
      if (car !== st.hover || stand !== st.standHover) st.set({ hover: car, standHover: stand });
      canvas.style.cursor = s ? "pointer" : "";
    });
    on("pointerleave", () => {
      const st = this.overlay.state;
      if (st.hover || st.standHover) st.set({ hover: null, standHover: null });
    });
    const api: WorkshopApi = {
      screen: () => sim.careerWorkshop(uid),
      remove: (slot) => this.after(sim.careerRemovePart(uid, slot)),
      fit: (slot, part) => this.after(sim.careerFitPart(uid, slot, part)),
      buy: (kind, grade) => this.after(sim.careerBuyPart(uid, kind, grade), false),
      sell: (part) => this.after(sim.careerSellPart(uid, part), false),
      strip: (assembly, slot) => this.after(sim.careerStripPart(uid, assembly, slot), false),
      build: (assembly, slot, part) => this.after(sim.careerBuildPart(uid, assembly, slot, part), false),
      clearWay: (slot) => this.after(sim.careerClearWay(uid, slot)),
      mods: () => sim.careerMods(uid),
      install: (id) => (sim.careerInstall(uid, id), this.after(sim.careerWorkshop(uid))),
      uninstall: (id) => (sim.careerUninstall(uid, id), this.after(sim.careerWorkshop(uid))),
      mechanic: (slot, source, sellOld) => {
        const r = "part" in source
          ? sim.careerMechanicInstall(uid, slot, source.part, null, sellOld)
          : sim.careerMechanicInstall(uid, slot, 0, source.buy, sellOld);
        this.after(r.workshop);
        return r;
      },
      paintShop: {
        car: () => {
          const car = sim.careerView().cars.find((c) => c.uid === uid);
          if (!car) throw new Error("the car is gone");
          return car;
        },
        mods: () => sim.careerMods(uid),
        respray: (paint) => {
          sim.careerRespray(uid, paint);
          // The paint is saved on the car; no rebuild needed, just repaint the view.
          this.deps.playerView()?.setPaint(paint);
          return this.after(sim.careerWorkshop(uid), false);
        },
        install: (id) => (sim.careerInstall(uid, id), this.after(sim.careerWorkshop(uid))),
        uninstall: (id) => (sim.careerUninstall(uid, id), this.after(sim.careerWorkshop(uid))),
        previewPaint: (paint) => {
          const car = sim.careerView().cars.find((c) => c.uid === uid);
          const hex = paint ?? car?.paint;
          if (hex !== undefined) this.deps.playerView()?.setPaint(hex);
        },
        previewMods: (mods) => {
          const real = sim.careerView().cars.find((c) => c.uid === uid)?.mods.map((m) => m.id) ?? [];
          this.deps.playerView()?.setLook(mods ?? real, true);
        },
      },
    };
    const host: WorkshopHost = {
      highlight: (slots) => this.highlight(slots),
      highlightStand: (slots) => this.highlightStand(slots),
      camera: (p) => this.camera(p),
      lift: (up) => (this.liftTarget = up ? 1 : 0),
      changed: () => {},
      back: () => this.deps.back(),
      drive: () => this.deps.drive(),
      carName: this.deps.carName(this.screen.def_id),
    };
    this.overlay.show(withSounds(api, (s) => this.deps.sound?.(s)), host);
    this.deps.ambience?.(true);
  }

  /** Close the workshop (the caller then shows another screen or starts a run). */
  close(): void {
    if (!this.active) return;
    this.active = false;
    this.deps.ambience?.(false);
    this.overlay.hide();
    this.room?.dispose();
    this.room = null;
    this.stand = this.engine = null;
    this.engineKey = "";
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
    this.placeEngine();
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
    // The cosmetic look is built after the panels: give it the removed panels when it arrives.
    if (view && view === this.placedFor && view.look && view.look !== this.lookFor) {
      this.lookFor = view.look;
      view.look.setRemovedRegions(this.removedRegions);
      this.highlight(this.highlighted);
    }
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
    if (!this.stand) {
      this.stand = buildStand();
      this.stand.position.copy(STAND_AT);
      this.room.floorGroup.add(this.stand);
      this.placeEngine();
    }
    this.room.root.position.set(p.x, p.y, 0);
    this.room.root.rotation.set(0, 0, yaw);
    this.room.setLift(this.liftT);
    // Parts of empty slots are not drawn, nor what is bolted to an empty slot's panel: the lamps
    // set into a bumper leave with it.
    const fitted = this.fittedSlots();
    const hosts = this.hostsOf(view);
    view.panels.hideParts((def, c) => slotsNeeded(def, c.x, hosts.get(def), fitted).some((s) => !fitted.has(s)));
    // On the lift the flat belly pan comes off, so the drivetrain, tank and rack show from below.
    view.panels.hideParts((def) => def.name === "underbody");
    this.removedRegions = new Set(Object.entries(REGION_SLOT).filter(([, slot]) => !fitted.has(slot)).map(([region]) => region));
    this.lookFor = null;
    this.hardware = null;
    this.hardwareGlow = [];
    void this.placeHardware(view);
    this.highlight(this.highlighted);
    // A rebuilt session resets the camera rig: back to the current preset (the lift stays put).
    this.camera(first ? "overview" : this.lastCam, first);
  }

  /**
   * Draw the hardware of the fitted slots nothing else draws on this car (battery, diff, tank...),
   * so every part has a place to be found, picked and taken off.
   */
  private async placeHardware(view: VehicleView): Promise<void> {
    const id = view.info.car_id;
    const [mesh, def, visuals] = await Promise.all([loadCarMesh(id), carDef(id), loadVisuals()]);
    if (!mesh || view !== this.placedFor || !this.active) return;
    const bay = new Bay(mesh, view.info.wheels);
    const centre = bay.engine.getCenter(new THREE.Vector3());
    const lay = layoutOf((def?.drivetrain as { layout?: unknown } | undefined)?.layout, centre.y > 0);
    const meshSlots = new Set(view.panels.partDefs().map((d) => slotOfPart(d.def, d.centre.x)));
    const draw = new Set(
      (this.screen?.slots ?? [])
        .filter((s) => HARDWARE_SLOTS.includes(s.id) && !s.parent && !meshSlots.has(s.id))
        // An aftermarket grade with cosmetic pieces is drawn by the look.
        .filter((s) => {
          const v = s.fitted?.grade ? visuals?.mods[s.fitted.grade] : undefined;
          return !(Array.isArray(v) && v.length > 0);
        })
        .map((s) => s.id),
    );
    const hw = buildHardware(bay, lay, this.screen?.slots ?? [], draw);
    view.root.add(hw);
    this.hardware = hw;
    this.highlight(this.highlighted);
  }

  /** Slots holding a part now. */
  private fittedSlots(): Set<string> {
    return new Set((this.screen?.slots ?? []).filter((s) => s.fitted).map((s) => s.id));
  }

  /** Host panel of each bolt-on mesh part of the view's model (mounts.ts). */
  private hostsOf(view: VehicleView) {
    return partHosts(view.info.car_id, view.panels.partDefs().map((d) => d.def));
  }

  /** Put the first loose engine of the screen on the stand (rebuilt when its parts change). */
  private placeEngine(): void {
    if (!this.stand) return;
    const st = this.screen?.stands[0] ?? null;
    const grade = st ? (this.screen?.inventory.find((p) => p.part.uid === st.uid)?.part.grade ?? null) : null;
    const key = st ? JSON.stringify([st.uid, grade, st.slots.map((s) => [s.id, s.fitted?.grade, s.fitted?.condition])]) : "";
    if (key === this.engineKey) return;
    this.engineKey = key;
    this.engine?.removeFromParent();
    this.standGlow = [];
    this.engine = null;
    if (!st) return;
    const e = buildEngine(st, grade);
    // The block's back face bolts to the stand plate; the crank sits at the plate's height.
    const blockLen = new THREE.Box3().setFromObject(e.children[0]).getSize(new THREE.Vector3()).y;
    e.position.set(0, -0.24 + blockLen / 2 + 0.04, STAND_CRANK_Z);
    this.stand.add(e);
    this.engine = e;
  }

  private slotAt(e: PointerEvent): string | null {
    const r = this.deps.scene.renderer.domElement.getBoundingClientRect();
    this.ndc.set(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1);
    this.ray.setFromCamera(this.ndc, this.deps.rig.camera);
    const onStand = this.engine ? this.ray.intersectObject(this.engine, true).find((h) => h.object.visible && !this.standGlow.includes(h.object as THREE.Mesh)) : undefined;
    const view = this.deps.playerView();
    if (!view?.panels.ready) return null;
    // Bolt-on cosmetic pieces (bull bar, wing, cage...) in front of the panel hit win.
    const hit = view.panels.partHit(this.ray);
    const look = view.look?.slotAt(this.ray);
    const hw = this.hardware ? this.ray.intersectObject(this.hardware, true).find((h) => h.object.visible && !this.hardwareGlow.includes(h.object as THREE.Mesh)) : undefined;
    let piece = look;
    if (hw && (!piece || hw.distance < piece.distance)) {
      let o: THREE.Object3D | null = hw.object;
      while (o && !o.userData.slot) o = o.parent;
      piece = { slot: (o?.userData.slot as string) ?? null, distance: hw.distance };
    }
    if (onStand && (!hit || onStand.distance < hit.distance) && (!piece || onStand.distance < piece.distance)) {
      let o: THREE.Object3D | null = onStand.object;
      while (o && !o.userData.slot) o = o.parent;
      return o ? STAND_PREFIX + (o.userData.slot as string) : null;
    }
    if (piece && (!hit || piece.distance < hit.distance)) return piece.slot;
    if (!hit) return null;
    const centre = view.panels.partDefs().find((d) => d.def === hit.def)?.centre;
    return pickSlot(hit.def, centre?.x ?? 0, this.fittedSlots());
  }

  private highlight(slots: string[]): void {
    this.highlighted = slots;
    const view = this.deps.playerView();
    if (!view?.panels.ready) return;
    // A slot lights up with everything that comes off with it (lamps in a bumper, its splitter).
    const set = new Set(slots);
    for (const s of this.screen?.slots ?? []) if (set.has(s.id)) for (const t of s.takes_along ?? []) set.add(t);
    const fitted = this.fittedSlots();
    const hosts = this.hostsOf(view);
    view.panels.highlightParts(set.size ? (def, c) => slotsNeeded(def, c.x, hosts.get(def), fitted).some((s) => set.has(s)) : null);
    view.look?.highlight(set);
    for (const g of this.hardwareGlow) g.removeFromParent();
    this.hardwareGlow = [];
    for (const part of this.hardware?.children ?? []) {
      if (!set.has(part.userData.slot as string)) continue;
      part.traverse((o) => {
        if (!(o instanceof THREE.Mesh) || this.hardwareGlow.includes(o)) return;
        const g = new THREE.Mesh(o.geometry, this.glowMat);
        g.renderOrder = 10;
        o.add(g);
        this.hardwareGlow.push(g);
      });
    }
  }

  /** Glow over the stand engine's parts in `slots` (empty = none). */
  private highlightStand(slots: string[]): void {
    for (const g of this.standGlow) g.removeFromParent();
    this.standGlow = [];
    if (!this.engine || !slots.length) return;
    const set = new Set(slots);
    for (const part of this.engine.children) {
      if (!set.has(part.userData.slot as string)) continue;
      part.traverse((o) => {
        if (!(o instanceof THREE.Mesh) || this.standGlow.includes(o)) return;
        const g = new THREE.Mesh(o.geometry, this.glowMat);
        g.renderOrder = 10;
        o.add(g);
        this.standGlow.push(g);
      });
    }
  }

  /** Automation: screen point over the centre of the stand engine's part in `slot`, or null. */
  pointOfStandPart(slot: string): { x: number; y: number } | null {
    const part = this.engine?.children.find((c) => c.userData.slot === slot);
    if (!part) return null;
    const r = this.deps.scene.renderer.domElement.getBoundingClientRect();
    const c = new THREE.Box3().setFromObject(part).getCenter(new THREE.Vector3());
    const w = c.project(this.deps.rig.camera);
    return { x: r.left + ((w.x + 1) / 2) * r.width, y: r.top + ((1 - w.y) / 2) * r.height };
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

  /** Automation / review shots: look from `eye` at `at`, both in car space (lift as it is). */
  lookAt(eye: [number, number, number], at: [number, number, number]): void {
    const view = this.deps.playerView();
    if (!view) return;
    const toSim = (v: [number, number, number]): [number, number, number] => {
      const w = view.root.localToWorld(new THREE.Vector3(v[0], v[1], v[2]));
      return [w.x, -w.z, w.y];
    };
    this.deps.rig.lookFrom(toSim(eye), toSim(at));
  }

  /** Last preset (automation). */
  get view(): WorkshopCam {
    return this.lastCam;
  }

  /** Automation: slots drawn as hardware on the car (hardware.ts). */
  hardwareSlots(): string[] {
    return (this.hardware?.children ?? []).map((c) => c.userData.slot as string | undefined).filter((s): s is string => !!s);
  }

  /**
   * Automation: fitted top-level slots drawn by nothing on the car (no mesh part, look piece or
   * hardware), so there is nowhere to click them. Should be empty once the hardware is placed.
   */
  undrawnSlots(): string[] {
    const view = this.deps.playerView();
    if (!view) return [];
    const drawn = new Set(view.panels.partDefs().map((d) => slotOfPart(d.def, d.centre.x)));
    view.root.traverse((o) => {
      if (o.userData.slot) drawn.add(o.userData.slot as string);
    });
    for (const s of this.hardwareSlots()) drawn.add(s);
    return (this.screen?.slots ?? []).filter((s) => s.fitted && !s.parent && !drawn.has(s.id)).map((s) => s.id);
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

const carDefs = new Map<string, Promise<Record<string, unknown> | null>>();

/** The car definition JSON (data/cars/<id>.json), fetched once per model. */
function carDef(id: string): Promise<Record<string, unknown> | null> {
  let p = carDefs.get(id);
  if (!p) {
    p = fetch(`data/cars/${id}.json`)
      .then((r) => (r.ok ? (r.json() as Promise<Record<string, unknown>>) : null))
      .catch(() => null);
    carDefs.set(id, p);
  }
  return p;
}
