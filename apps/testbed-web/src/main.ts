// WreckPursuit browser testbed: drives wp_wasm, renders vehicles + debug layers, HUD,
// telemetry, scenario menu and replay UI. See docs/integration/testbed.md.
//
// URL parameters (handy for automation): ?scenario=crash/head_on.ron&speed=60 runs a scenario,
// ?bench=24 spawns a 24-car benchmark, ?cam=orbit picks a camera, ?replay=last plays the last
// stored replay, ?city=procedural_12x12 picks the city (default vesper_v2), ?chase=1 starts the
// city chase (?chase=rain the rainy night), ?weather=heavy_rain picks the weather,
// ?style=greybox|lowpoly the art style (default low-poly dark, render/style.ts).
// ?race=<id> starts a street race (&autopilot=1: the AI drives the player's car).
// ?fx=low|med|high sets the crash effects quality (render/fx; default from ?gfx).
// ?menu=1 opens the game menu's title screen on load (as the demo build does) with the developer
// panels off by default; ?dev=0 / ?dev=1 forces the developer panels for this page.
import { loadWasm, sim, type DriverInput, type Scenario, type ScenarioResult } from "./wasm/sim";
import { META_STRIDE, RACE_STRIDE, RI, RS, STATE_STRIDE, S, T, VEHICLE_KIND, W } from "./wasm/layout";
import { POLICE_PAINT, TRAFFIC_PALETTE } from "./render/fleet";
import { actionHeld, getBindings, installKeyboard, readInput, setInputBlocked, toggleHeadlights, type InputSource } from "./input";
import { actionsForKey, bindingsHelp, keyName, padName, type ActionId } from "./bindings";
import { CrashReplayDirector, autoCrashReplay, onAutoCrashReplayChange, setAutoCrashReplay } from "./crash_replay";
import { installTouch, setTouchSteerMode, touchSteerMode } from "./touch";
import { ControlsPanel } from "./ui/controls";
import { layers, store, bytesToBase64, base64ToBytes } from "./settings";
import { SceneView } from "./scene";
import { VehicleView } from "./vehicle_view";
import { clearDebris, meshTimings, panelEvents, preloadCarMeshes, stepDebris } from "./render/panels";
import { LodDirector } from "./render/lod";
import { PoliceToolsView } from "./render/police_tools";
import { PropView } from "./render/props";
import { CameraRig, CAMERAS, CAMERA_MODES, cameraLabel, type CameraMode } from "./cameras";
import { Hud } from "./hud";
import { TelemetryPlots } from "./telemetry";
import { AudioSystem } from "./audio";
import { UiOverlay } from "./ui/overlay";
import { MapOverlay } from "./ui/map";
import { HudLayout } from "./ui/layout";
import * as wasmExports from "./wasm/pkg/wp_wasm.js";
import { defaultGameSettings, hasStoredSettings, loadGameSettings, mountGameMenu, type GameSettings, type PlayChoice } from "../../ui-kit/src/live";
import { gfxMode } from "./render/quality";
import { RunEndView } from "./ui/runEnd";
import { CareerGarageView } from "../../ui-kit/src/garage/CareerGarage";
import { downloadText, keepUnreadable, loadCareerText, saveCareerText, saveFileName } from "../../ui-kit/src/garage/store";
import type { CareerApi, CareerView, RunKind } from "../../ui-kit/src/garage/types";
import { RaceView, type RaceFrame } from "./ui/race";
import { RaceMarker, parseCourse } from "./race_marker";
import { RealTimeMeter } from "./rtf";
import { WeatherView } from "./render/weather";
import { ArtStyle, STYLE_LABELS, type StyleId } from "./render/style";
import { FrameProfiler } from "./perf";
import { CrashFx } from "./render/fx";
import * as THREE from "three";

// v2: canonical simcli replay.bin (v1 held the retired WPR\x01 format).
const LAST_REPLAY_KEY = "wp.testbed.lastReplay.v2";
/** Cities the selector offers (data/city/<id>.greybox.glb); the first is the default. */
const CITY_IDS = ["vesper_v2", "procedural_12x12"];
const COLOURS = [0xd04a2a, 0x2a7fd0, 0x3fb950, 0xd0a92a, 0x9b59b6, 0x1abc9c];
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const params = new URLSearchParams(location.search);

// Per-entity static lattice data for the HUD; cleared whenever a new session starts.
const regions = new Map<number, Uint8Array>();
const rests = new Map<number, Float32Array>();

/** Default car for free drive and the city chase: the quick AWD hot hatch (analog name; 0–100
 * km/h in ~5.4 s, 80 ms dual-clutch shifts). The debug sedan (8.3 s) read as "slow" in the
 * owner's playtest. */
const DEFAULT_CAR = "lindwurm_4t";

interface AppState {
  views: Map<number, VehicleView>;
  player: number;
  focus: number;
  source: InputSource;
  paused: boolean;
  alpha: number;
  lastReplay: Uint8Array | null;
  scenarioFile: string | null;
  scenario: Scenario | null;
  result: ScenarioResult | null;
  input: DriverInput;
  bench: { n: number; frames: number; sim: number; render: number; t0: number; tick0: number } | null;
  /** Frame-time breakdown (ms, smoothed) and real-time factor; read by the HUD and e2e. */
  perf: { simMs: number; viewsMs: number; renderMs: number; frameMs: number; rtf: number };
  /** Per-section frame cost (src/perf.ts): smoothed `ms`, raw `acc` / `frames` for automation. */
  prof: FrameProfiler;
  /** Sim events seen this session, by kind (`TrafficViolation`, `Police`, …). The event feed
   * keeps only 12 lines, so automation waits on these counts instead of the feed text. */
  eventCounts: Record<string, number>;
}

const app: AppState = {
  views: new Map(),
  player: 1,
  focus: 1,
  source: "Keyboard",
  paused: false,
  alpha: 0,
  lastReplay: null,
  scenarioFile: null,
  scenario: null,
  result: null,
  input: { steer: 0, throttle: 0, brake: 0, handbrake: 0, clutch: 0, gearUp: false, gearDown: false, horn: false, headlights: false },
  bench: null,
  perf: { simMs: 0, viewsMs: 0, renderMs: 0, frameMs: 0, rtf: 1 },
  prof: new FrameProfiler(),
  eventCounts: {},
};

async function main(): Promise<void> {
  await loadWasm();
  const overlay = $("overlay");
  // Art style (machine S): low-poly dark by default; the selector / ?style= switch to greybox.
  const STYLE_KEY = "wp.testbed.style.v1";
  const initialStyle: StyleId = (params.get("style") ?? store.get(STYLE_KEY)) === "greybox" ? "greybox" : "lowpoly";
  // The low-poly post stack renders into its own MSAA target (when the graphics quality allows
  // MSAA): no multisampled canvas needed (greybox picked later in the session then draws
  // without antialiasing).
  const scene = new SceneView(document.body, { postAA: initialStyle === "lowpoly" });
  const rig = new CameraRig(scene.renderer.domElement);
  const hud = new Hud(overlay);
  const plots = new TelemetryPlots(overlay);
  const audio = new AudioSystem(scene.world, overlay);
  const ui = new UiOverlay(overlay);
  const map = new MapOverlay(overlay);
  const layout = new HudLayout();
  // Game menu (built below, once everything it drives exists) and its current settings.
  let menu: ReturnType<typeof mountGameMenu> | null = null;
  let gameSettings: GameSettings | null = null;
  // Countersteer strength setter (machine A, wp_wasm); the menu slider is disabled until it lands.
  const countersteerSetter = Object.entries(wasmExports).find(([k, v]) => /countersteer/i.test(k) && typeof v === "function")?.[1] as
    | ((entity: number, strength: number) => void)
    | undefined;
  // Traffic / police at every LOD: instanced fleet + hand-over to full views (render/lod.ts).
  const lod = new LodDirector();
  scene.world.add(lod.fleet.group);
  // Phones / tablets draw less traffic (render/quality.ts touch profile).
  lod.trafficDrawM = scene.quality.trafficDrawM;
  lod.fleet.fullLodM = scene.quality.fullLodM;
  // On-screen touch controls (src/touch.ts): shown on touch devices, read through readInput.
  const touch = installTouch();
  const controls = new ControlsPanel(overlay);
  const refreshHelp = () => hud.setControlsHelp(bindingsHelp(getBindings()));
  controls.onChange = refreshHelp;
  refreshHelp();
  $("controls-open").onclick = () => controls.toggle();
  // Parked cars and cones (playtest row 50): every physical prop is drawn (render/props.ts).
  const props = new PropView();
  props.fullLodM = scene.quality.fullLodM;
  props.maxFull = scene.quality.propFullMax;
  props.drawM = scene.quality.propDrawM;
  scene.world.add(props.group);
  // Spike strips on the road (playtest row 88).
  const policeTools = new PoliceToolsView();
  scene.world.add(policeTools.group);
  // Crash effects (owner row 69): sparks, smoke, glass, skid marks, debris (render/fx/).
  const fx = new CrashFx(scene.quality.fx);
  scene.world.add(fx.group);
  // End-of-run panel (lead): capture / wreck → result + "Repair & continue" / "Restart".
  const runEndEl = document.createElement("div");
  runEndEl.id = "ui-run-end";
  runEndEl.style.cssText = "position:absolute;inset:0;pointer-events:none;";
  overlay.appendChild(runEndEl);
  const runEnd = new RunEndView(runEndEl, {
    repair: () => sim.runRepairContinue(),
    restart: () => restartRun(),
    onOpen: () => ui.setBill(true),
    onClose: () => ui.setBill(false),
    garage: () => (sim.careerRunActive() ? () => openGarage() : null),
  });
  const weather = new WeatherView(scene.scene, scene.renderer, overlay);
  weather.rainFraction = scene.quality.rainFraction;
  weather.attachMaterial(scene.groundMaterial);
  const styleSel = $("style") as HTMLSelectElement;
  for (const [id, label] of Object.entries(STYLE_LABELS)) styleSel.add(new Option(label, id));
  const style = new ArtStyle(scene, initialStyle);
  styleSel.value = style.id;
  const setStyle = (id: StyleId) => {
    style.set(id);
    styleSel.value = id;
    store.set(STYLE_KEY, id);
  };
  styleSel.onchange = () => setStyle(styleSel.value as StyleId);
  scene.onCityLoaded = (root, cityId) => {
    weather.attachCity(root);
    style.attachCity(root, cityId);
  };
  const weatherSel = $("weather") as HTMLSelectElement;
  for (const [id, label] of sim.weatherChoices()) weatherSel.add(new Option(label, id));
  const weatherParam = params.get("weather");
  if (weatherParam && [...weatherSel.options].some((o) => o.value === weatherParam)) weatherSel.value = weatherParam;
  /** Scenario JSON with the selector's weather ("scenario" keeps the scenario's own). */
  const withWeather = (json: string) => (weatherSel.value === "auto" ? json : sim.scenarioWithWeather(json, weatherSel.value));

  // ----- career (machine G): persistent account between runs, garage screen ---------------
  // The career lives in wp_wasm (`career_*`); this file stores it (localStorage, versioned)
  // and starts runs with the career car. See docs/integration/economy.md.
  const persistCareer = () => {
    try {
      saveCareerText(sim.careerSave());
    } catch (e) {
      console.warn("career not saved", e);
    }
  };
  /** B05: the save was written mid-pursuit (the tab was closed during a chase); loading it
   * settled the run as a bust. Shown in the garage once the testbed is up. */
  let abandonedBust: string | null = null;
  {
    const text = loadCareerText();
    let loaded = false;
    if (text) {
      try {
        sim.careerLoad(text);
        loaded = true;
        const bust = sim.careerTakeAbandoned();
        if (bust) {
          const fines = bust.ledger.filter((l) => l.cash_delta < 0).reduce((t, l) => t - l.cash_delta, 0);
          const has = (r: string) => bust.ledger.some((l) => l.reason === r);
          const car = has("CarSeized") ? "your car was seized" : has("CarImpounded") ? "your car is in the pound" : "";
          abandonedBust =
            `Busted: the game was closed during a pursuit, so the police caught you. ` +
            `Fines and fees $${fines.toLocaleString()}${car ? `; ${car}` : ""}.`;
          persistCareer();
        }
      } catch (e) {
        console.warn("career save unreadable; kept aside, starting a new career", e);
        keepUnreadable(text);
      }
    }
    if (!loaded) {
      sim.careerNew("Arcade");
      persistCareer();
    }
  }
  const garageEl = document.createElement("div");
  garageEl.id = "ui-garage";
  garageEl.style.cssText = "position:absolute;inset:0;pointer-events:none;";
  overlay.appendChild(garageEl);
  const act = (f: () => CareerView): CareerView => {
    const v = f();
    persistCareer();
    return v;
  };
  const careerApi: CareerApi = {
    view: () => sim.careerView(),
    mods: (uid) => sim.careerMods(uid),
    quote: (uid, req) => sim.careerQuote(uid, req),
    buy: (id, c) => act(() => sim.careerBuy(id, c)),
    floorCar: () => act(() => sim.careerFloorCar()),
    sell: (uid) => act(() => sim.careerSell(uid)),
    setActive: (uid) => act(() => sim.careerSetActive(uid)),
    install: (uid, id) => act(() => sim.careerInstall(uid, id)),
    uninstall: (uid, id) => act(() => sim.careerUninstall(uid, id)),
    repair: (uid, req) => act(() => sim.careerRepair(uid, req)),
    release: (uid) => act(() => sim.careerRelease(uid)),
    insure: (uid, tier) => act(() => sim.careerInsure(uid, tier)),
    nextNight: () => act(() => sim.careerNextNight()),
    newCareer: (mode) => act(() => sim.careerNew(mode)),
    exportSave: () => downloadText(saveFileName(sim.careerView().day), sim.careerSave()),
    importSave: (text) => act(() => sim.careerLoad(text)),
    startRun: (kind) => void startCareerRun(kind),
    close: () => {
      garage.hide();
      app.paused = false;
    },
  };
  const garage = new CareerGarageView(garageEl, careerApi);
  // Instant crash replay (lead, row 70): offer after a big crash, replay, resume live play.
  const crash = new CrashReplayDirector(
    document.body,
    overlay,
    sim,
    {
      // Not on benches, in the garage, with the full map open or while paused. A wreck's
      // end-of-run panel waits behind the replay (the overlay is hidden while it plays).
      allowed: () => !app.bench && !app.paused && !garage.open && !map.isOpen,
      sessionSwapped: (entering) => {
        swapSessionViews();
        if (!entering) rig.setMode(rig.mode, app.views.get(app.focus)?.root);
      },
      playerRoot: () => app.views.get(app.player)?.root ?? null,
    },
    () => {
      const b = getBindings();
      const k = b.keys.replayCrash[0];
      const p = b.pad.replayCrash[0];
      return [k ? keyName(k) : "", p ? padName(p) : ""].filter(Boolean).join(" / ") || "unbound";
    },
  );
  const autoBox = $("rp-auto-crash") as HTMLInputElement;
  autoBox.checked = autoCrashReplay();
  autoBox.onchange = () => setAutoCrashReplay(autoBox.checked);
  onAutoCrashReplayChange((on) => (autoBox.checked = on));
  /** Garage: ends a career run in progress (its result goes into the career), pauses. */
  const openGarage = (message: string | null = null) => {
    crash.reset();
    if (sim.careerRunActive()) sim.careerEndRun();
    persistCareer();
    runEnd.reset();
    app.paused = true;
    garage.show(message);
  };
  let careerSavedSeq = -1;
  let careerSavedAt = 0;
  // Street races (workstream 15): picker, HUD, results, and the next gate in the world.
  const raceEl = document.createElement("div");
  raceEl.id = "ui-race";
  raceEl.style.cssText = "position:absolute;inset:0;pointer-events:none;";
  overlay.appendChild(raceEl);
  const raceMarker = new RaceMarker(scene.world);
  const raceView = new RaceView(raceEl, {
    start: (id) => startRace(id),
    freeDrive: () => startFreeDrive(undefined, undefined, true),
    list: () => sim.raceList(($("car") as HTMLSelectElement).value || DEFAULT_CAR, ($("mode") as HTMLSelectElement).value),
  });
  const citySel = $("city-map") as HTMLSelectElement;
  const cityParam = params.get("city");
  if (cityParam && CITY_IDS.includes(cityParam)) citySel.value = cityParam;

  // ----- session management --------------------------------------------------------------

  /** Crash replay in / out: the vehicles, debris, contacts and props of the other session. */
  const swapSessionViews = () => {
    for (const v of app.views.values()) v.dispose();
    app.views.clear();
    clearDebris();
    regions.clear();
    rests.clear();
    scene.clearContacts();
    plots.clear();
    syncViews();
    props.set(sim.propsStatic(), lod.models, lod.fleet.mats);
  };

  const rebuildViews = () => {
    crash.reset();
    for (const v of app.views.values()) v.dispose();
    app.views.clear();
    clearDebris();
    fx.reset();
    regions.clear();
    rests.clear();
    scene.clearContacts();
    hud.clearEvents();
    app.eventCounts = {};
    plots.clear();
    ui.reset();
    map.reset();
    runEnd.reset();
    weather.reset();
    // Every model the session can show (player, traffic, police) starts loading now, so cars
    // that appear later never wait for their mesh (playtest row 25).
    lod.models = sim.sessionCarModels();
    void preloadCarMeshes(lod.models.cars);
    lod.log.reset(0);
    syncViews();
    const ids = sim.entities();
    app.player = ids[0] ?? 1;
    app.focus = app.player;
    const sc = sim.scenario();
    scene.setBarriers(sc.barriers ?? []);
    const city = sim.worldInfo()[W.city] > 0.5;
    const cityId = city ? sim.cityId() : "";
    void scene.setCity(cityId || null);
    if (cityId && CITY_IDS.includes(cityId)) citySel.value = cityId;
    scene.signals.setHeads(sim.signalHeads());
    props.set(sim.propsStatic(), lod.models, lod.fleet.mats);
    ($("city") as HTMLInputElement).checked = city;
    ($("mode") as HTMLSelectElement).value = modeName();
    const first = app.views.get(app.player);
    if (first) ($("car") as HTMLSelectElement).value = first.info.car_id;
    if (app.scenarioFile) ($("sc-select") as HTMLSelectElement).value = app.scenarioFile;
    syncAssistBoxes();
    rig.setMode(rig.mode, app.views.get(app.focus)?.root);
    resetRace(sc, ids);
  };

  /** Race sessions (scenario `race_<id>`): driver names, the course, the HUD. */
  const resetRace = (sc: { name: string; mode: string; entities: { car: string }[] }, ids: number[]) => {
    const info = sim.raceInfo();
    const id = info.length > 0 && sc.name.startsWith("race_") ? sc.name.slice(5).replace("+autopilot", "") : "";
    const names = new Map<number, string>();
    let course = null;
    if (id) {
      const card = sim.raceList(sc.entities[0]?.car ?? DEFAULT_CAR, sc.mode).find((c) => c.id === id);
      ids.forEach((e, i) => names.set(e, i === 0 ? "You" : (card?.drivers[i - 1]?.name ?? `#${e}`)));
      course = parseCourse(sim.raceCourse());
    }
    raceMarker.setCourse(course);
    raceView.reset(id, names, course);
  };

  /** Start street race `id` (the picker's "Race" button, `?race=`, "Race again"). */
  const startRace = (
    id: string,
    car = ($("car") as HTMLSelectElement).value || DEFAULT_CAR,
    mode = ($("mode") as HTMLSelectElement).value,
    autopilot = params.get("autopilot") === "1",
  ) => {
    app.scenarioFile = null;
    app.scenario = null;
    app.result = null;
    app.bench = null;
    sim.raceStart(id, car, mode, autopilot);
    rebuildViews();
    applyAssistPreset(app.source);
    rig.setMode(rig.preferred, app.views.get(app.focus)?.root);
  };

  /** Per-frame race state for the HUD and the gate marker. */
  const raceFrame = (cur: Float32Array, meta: Float32Array, tS: number, withReport: boolean): void => {
    const info = sim.raceInfo();
    if (info.length === 0) return;
    const standings = sim.raceStandings();
    const player = info[RI.player];
    let me = -1;
    for (let o = 0; o + RACE_STRIDE <= standings.length; o += RACE_STRIDE) if (standings[o + RS.entity] === player) me = o;
    const yawOf = (o: number) => {
      const [qx, qy, qz, qw] = [cur[o + S.rot], cur[o + S.rot + 1], cur[o + S.rot + 2], cur[o + S.rot + 3]];
      return Math.atan2(-2 * (qx * qy - qw * qz), 1 - 2 * (qx * qx + qz * qz));
    };
    let pose: RaceFrame["player"] = null;
    const others: RaceFrame["others"] = [];
    for (let o = 0, i = 0; o < cur.length; o += STATE_STRIDE, i++) {
      const id = cur[o + S.entity];
      const kind = meta[i * META_STRIDE + 1];
      if (id === player) pose = { pos: [cur[o + S.pos], cur[o + S.pos + 1]], yaw: yawOf(o) };
      else if (kind === VEHICLE_KIND.aiRacer || kind === VEHICLE_KIND.police)
        others.push([id, cur[o + S.pos], cur[o + S.pos + 1], yawOf(o), kind === VEHICLE_KIND.police ? 2 : 1]);
    }
    const report = withReport || raceView.report ? sim.raceResult() : null;
    raceView.update({ info, standings, report, player: pose, others }, performance.now());
    const done = me < 0 || standings[me + RS.status] > 0 || info[RI.phase] === 2;
    const next = done ? -1 : standings[me + RS.nextGate];
    const gates = info[RI.gates];
    const closed = info[RI.closed] > 0.5;
    const final = closed ? next === 0 && standings[me + RS.lap] >= info[RI.laps] : next === gates - 1;
    raceMarker.update(next, final, tS);
  };

  /** One view per sim vehicle — scenario cars plus whatever the sim spawns itself (police
   * units, promoted traffic) — created and disposed as vehicles come and go. Promoted traffic
   * keeps the livery it had as an instanced agent (`trafficPaint`: entity → colour). */
  const syncViews = (meta: Float32Array = sim.vehicleMeta(), trafficPaint: Map<number, number> = new Map()) => {
    const live = new Set<number>();
    for (let o = 0; o < meta.length; o += META_STRIDE) {
      const id = meta[o];
      live.add(id);
      if (app.views.has(id)) continue;
      const kind = meta[o + 1];
      const colour =
        kind === VEHICLE_KIND.police
          ? POLICE_PAINT
          : kind === VEHICLE_KIND.traffic
            ? (trafficPaint.get(id) ?? TRAFFIC_PALETTE[0])
            : COLOURS[(id - 1) % COLOURS.length];
      const v = new VehicleView(id, colour);
      scene.world.add(v.root);
      scene.world.add(v.velocity);
      app.views.set(id, v);
    }
    for (const [id, v] of app.views) {
      if (live.has(id)) continue;
      v.dispose();
      app.views.delete(id);
      regions.delete(id);
      rests.delete(id);
      if (app.focus === id) app.focus = app.player;
    }
  };

  const isReplay = () => sim.worldInfo()[W.replay] > 0.5;
  const modeName = () => (sim.worldInfo()[W.mode] > 0.5 ? "Simulation" : "Arcade");

  const startFreeDrive = (
    car = ($("car") as HTMLSelectElement).value || DEFAULT_CAR,
    mode = ($("mode") as HTMLSelectElement).value,
    city = ($("city") as HTMLInputElement).checked,
    hour?: number,
  ) => {
    app.scenarioFile = null;
    app.scenario = null;
    app.result = null;
    app.bench = null;
    let json = city ? sim.freeDriveCityJson(car, mode, citySel.value) : sim.freeDriveJson(car, mode, false);
    // Optional start hour (automation: day/night screenshots).
    if (hour !== undefined) json = JSON.stringify({ ...JSON.parse(json), time_of_day_h: hour });
    sim.init(1, withWeather(json), false);
    app.scenario = null;
    rebuildViews();
    applyAssistPreset(app.source);
  };

  /** City chase preset: procedural city at 23:00, police + traffic, player already speeding.
   * `rain` = the rainy-night variant (downpour, soaked road; the weather selector can still
   * override it). */
  const startCityChase = (
    car = ($("car") as HTMLSelectElement).value || DEFAULT_CAR,
    mode = ($("mode") as HTMLSelectElement).value,
    rain = false,
  ) => {
    app.scenarioFile = null;
    app.scenario = null;
    app.result = null;
    app.bench = null;
    const json = rain ? sim.cityChaseRainJson(car, mode, citySel.value) : sim.cityChaseCityJson(car, mode, citySel.value);
    sim.init(0, withWeather(json), false);
    rebuildViews();
    applyAssistPreset(app.source);
    // Back to the viewer's own angle (near chase unless they picked another one).
    rig.setMode(rig.preferred, app.views.get(app.focus)?.root);
  };

  /** A career run: the city chase or a free drive (city box) with the career car, its saved
   * damage and mods. Opens the garage instead when the car cannot go out. */
  const startCareerRun = (kind: RunKind): boolean => {
    const json =
      kind === "chase"
        ? sim.cityChaseCityJson(DEFAULT_CAR, "Arcade", citySel.value)
        : ($("city") as HTMLInputElement).checked
          ? sim.freeDriveCityJson(DEFAULT_CAR, "Arcade", citySel.value)
          : sim.freeDriveJson(DEFAULT_CAR, "Arcade", false);
    app.scenarioFile = null;
    app.scenario = null;
    app.result = null;
    app.bench = null;
    try {
      sim.careerStartRun(withWeather(json));
    } catch (e) {
      openGarage(String(e).replace(/^(Error: )?wp_wasm: /, ""));
      return false;
    }
    garage.hide();
    app.paused = false;
    persistCareer();
    rebuildViews();
    applyAssistPreset(app.source);
    if (kind === "chase") rig.setMode(rig.preferred, app.views.get(app.focus)?.root);
    return true;
  };

  /** "Restart" on the end-of-run panel: the same session again (the account carries over). */
  const restartRun = () => {
    const sc = sim.scenario();
    if (sim.careerRunActive()) return void startCareerRun(sc.name === "city_chase" ? "chase" : "free");
    const car = sc.entities[0]?.car;
    if (sc.name === "city_chase") startCityChase(car, sc.mode);
    else if (sc.name === "city_chase_rainy_night") startCityChase(car, sc.mode, true);
    else if (raceView.raceId) startRace(raceView.raceId, car, sc.mode);
    else startFreeDrive(car, sc.mode);
  };

  const runScenario = async (file: string, speedKmh?: number) => {
    const text = await (await fetch(`scenarios/${file}`)).text();
    let json = sim.scenarioFromRon(text);
    if (speedKmh && speedKmh > 0) json = sim.scenarioWithSpeed(json, speedKmh / 3.6);
    app.scenarioFile = file;
    app.result = null;
    app.bench = null;
    sim.init(0, withWeather(json), true);
    app.scenario = sim.scenario();
    rebuildViews();
  };

  // Owner default (playtest row 29): TCS, ESC and automatic gears start OFF; any assist box the
  // viewer ticks or unticks is remembered (localStorage) and wins over the preset.
  const ASSIST_KEY = "wp.assistOverrides";
  const savedAssistOverrides = (): Record<string, boolean> => {
    try {
      return JSON.parse(store.get(ASSIST_KEY) ?? "{}") as Record<string, boolean>;
    } catch {
      return {};
    }
  };
  const assistOverrides = (): Record<string, boolean> => ({ tcs: false, esc: false, auto_gears: false, ...savedAssistOverrides() });
  /** Touch play (owner request row 71): the gamepad (analogue) preset plus automatic gears,
   * traction control and stronger steer help; boxes the viewer ticked still win. */
  const TOUCH_ASSISTS: Record<string, unknown> = { tcs: true, auto_gears: true, auto_clutch: true, steer_speed_sensitivity: 1.0, countersteer_help: 0.6 };
  // One-click presets (Rookie / Street / Sim, GDD §6): damage mode + assists together. The
  // chosen preset is remembered per viewer and replaces the per-box overrides; ticking a box
  // (or changing an assist in the game menu) afterwards turns it into a custom setup.
  const PRESET_KEY = "wp.assistPreset";
  /** Presets are defined for keyboard / gamepad / wheel; touch plays with the gamepad set. */
  const presetSource = (s: InputSource) => (s === "Touch" ? "Gamepad" : s);
  /** True while the developer panel (assist boxes, preset row) pushes its state into the game
   * menu: the panel already set the assists, so the menu only records them. */
  let panelToMenu = false;
  const tellMenu = (patch: Partial<GameSettings>) => {
    panelToMenu = true;
    try {
      menu?.update(patch);
    } finally {
      panelToMenu = false;
    }
  };
  const storedPreset = (): string | null => {
    const p = store.get(PRESET_KEY);
    return p && sim.assistPresets().includes(p) ? p : null;
  };
  const applyAssistPreset = (source: InputSource) => {
    if (isReplay()) return;
    try {
      const named = storedPreset();
      if (named) {
        sim.setAssists(app.player, sim.assistPresetNamed(named, presetSource(source)).assists);
      } else {
        const preset = sim.assistPreset(source === "Touch" ? "Gamepad" : source) as unknown as Record<string, unknown>;
        // `steer_help` (game menu) is not a profile field: off zeroes the speed-sensitive steering.
        const chosen = (source === "Touch" ? { ...TOUCH_ASSISTS, ...savedAssistOverrides() } : assistOverrides()) as Record<string, unknown>;
        const { steer_help: steerHelp, ...overrides } = chosen;
        Object.assign(preset, overrides);
        if (steerHelp === false) preset.steer_speed_sensitivity = 0;
        sim.setAssists(app.player, preset as never);
        if (countersteerSetter && gameSettings) countersteerSetter(app.player, gameSettings.assists.countersteer);
      }
      syncAssistBoxes();
    } catch (e) {
      console.warn(e);
    }
  };
  const choosePreset = (name: string) => {
    if (isReplay()) return;
    const p = sim.assistPresetNamed(name, presetSource(app.source));
    // Keep the game menu's toggles in step.
    const a = p.assists as unknown as Record<string, unknown>;
    if (menu) tellMenu({
      damageMode: p.mode as GameSettings["damageMode"],
      assists: { ...menu.settings.assists, abs: Boolean(a.abs), tcs: Boolean(a.tcs), esc: Boolean(a.esc), autoGears: Boolean(a.auto_gears),
        steerHelp: Number(a.steer_speed_sensitivity) > 0, countersteer: Number(a.countersteer_help) },
    });
    ($("mode") as HTMLSelectElement).value = p.mode;
    sim.setDamageMode(p.mode);
    sim.setAssists(app.player, p.assists);
    store.set(PRESET_KEY, name);
    store.set(ASSIST_KEY, "{}");
    syncAssistBoxes();
  };
  const syncPresetButtons = () => {
    const active = isReplay() ? "" : sim.assistPresetOf(app.player, presetSource(app.source));
    document.querySelectorAll<HTMLButtonElement>("button.preset").forEach((b) => {
      b.setAttribute("aria-pressed", String(b.dataset.p === active));
    });
    $("preset-state").textContent = isReplay() ? "" : active ? "" : "custom";
  };

  // Clutch automatic / manual (playtest row 59): the AssistProfile's `auto_clutch`. Manual uses
  // the player's clutch binding directly (DriverInput.clutch). Default stays automatic; a change is
  // remembered with the other assist overrides.
  const clutchSel = $("clutch-mode") as HTMLSelectElement;
  const syncAssistBoxes = () => {
    if (isReplay()) return;
    const a = sim.assists(app.player) as unknown as Record<string, unknown>;
    document.querySelectorAll<HTMLInputElement>("#assist-row input").forEach((cb) => {
      cb.checked = Boolean(a[cb.dataset.a!]);
    });
    clutchSel.value = a.auto_clutch === false ? "manual" : "auto";
    syncPresetButtons();
  };

  // ----- replay ---------------------------------------------------------------------------

  const stopRecording = (): Uint8Array => {
    const bytes = sim.recordStop();
    app.lastReplay = bytes;
    store.set(LAST_REPLAY_KEY, bytesToBase64(bytes));
    return bytes;
  };

  const loadReplay = (bytes: Uint8Array) => {
    app.lastReplay = bytes;
    sim.replayLoad(bytes);
    sim.replayPlay(Number(($("rp-speed") as HTMLSelectElement).value));
    rebuildViews();
    if (rig.mode === "chase") rig.setMode("cinematic", app.views.get(app.focus)?.root);
  };

  const download = (bytes: Uint8Array | string, name: string, type: string) => {
    const blob = new Blob([bytes as BlobPart], { type });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  };

  // ----- controls -------------------------------------------------------------------------

  const carSel = $("car") as HTMLSelectElement;
  for (const id of sim.carIds()) carSel.add(new Option(id, id));
  carSel.onchange = () => startFreeDrive(carSel.value);

  const scSel = $("sc-select") as HTMLSelectElement;
  try {
    const files: string[] = await (await fetch("scenarios/index.json")).json();
    for (const f of files) scSel.add(new Option(f, f));
  } catch {
    scSel.add(new Option("(scenario folder not served)", ""));
  }
  $("sc-run").onclick = () => scSel.value && runScenario(scSel.value, Number(($("sc-speed") as HTMLInputElement).value));
  $("sc-free").onclick = () => startFreeDrive();
  $("sc-chase").onclick = () => startCityChase();
  $("sc-chase-rain").onclick = () => startCityChase(undefined, undefined, true);
  /** Restart whatever is running with the selected weather (weather is part of the scenario,
   * so replays carry it). */
  weatherSel.onchange = () => {
    if (isReplay()) return;
    const name = sim.scenario()?.name;
    if (sim.careerRunActive()) {
      sim.careerEndRun();
      void startCareerRun(name === "city_chase" ? "chase" : "free");
    } else if (app.scenarioFile) void runScenario(app.scenarioFile);
    else if (name === "city_chase") startCityChase();
    else if (name === "city_chase_rainy_night") startCityChase(undefined, undefined, true);
    else startFreeDrive();
  };
  const careerOn = () => ($("career") as HTMLInputElement).checked;
  $("sc-free").onclick = () => (careerOn() ? startCareerRun("free") : startFreeDrive());
  $("sc-chase").onclick = () => (careerOn() ? startCareerRun("chase") : startCityChase());
  $("sc-garage").onclick = () => openGarage();
  addEventListener("pagehide", persistCareer);
  $("sc-races").onclick = () => raceView.openPicker();
  ($("mode") as HTMLSelectElement).onchange = (e) => {
    // Live switch (Sim::set_mode_table), logged as a replay op; existing damage is kept.
    // A career run keeps the career's mode (pick it with "New career" in the garage).
    if (isReplay() || sim.careerRunActive()) return void (($("mode") as HTMLSelectElement).value = modeName());
    sim.setDamageMode((e.target as HTMLSelectElement).value);
    syncPresetButtons();
    menu?.update({ damageMode: (e.target as HTMLSelectElement).value as GameSettings["damageMode"] });
  };
  ($("city") as HTMLInputElement).onchange = () => startFreeDrive();
  citySel.onchange = () => {
    if (isReplay()) return;
    const name = sim.scenario()?.name;
    if (sim.careerRunActive()) {
      // A career run restarts in the new city with the career car (the run so far is settled).
      sim.careerEndRun();
      void startCareerRun(name === "city_chase" ? "chase" : "free");
    } else if (name === "city_chase") startCityChase();
    else if (name === "city_chase_rainy_night") startCityChase(undefined, undefined, true);
    else if (($("city") as HTMLInputElement).checked) startFreeDrive();
  };
  document.querySelectorAll<HTMLInputElement>("#assist-row input").forEach((cb) => {
    cb.onchange = () => {
      if (isReplay()) return;
      const a = sim.assists(app.player) as unknown as Record<string, unknown>;
      a[cb.dataset.a!] = cb.checked;
      sim.setAssists(app.player, a as never);
      // A hand-picked box turns the preset into a custom setup: remember the current boxes.
      const o = assistOverrides();
      if (storedPreset()) for (const k of ["abs", "tcs", "esc", "auto_gears"]) o[k] = Boolean(a[k]);
      o[cb.dataset.a!] = cb.checked;
      store.set(ASSIST_KEY, JSON.stringify(o));
      store.set(PRESET_KEY, "");
      syncPresetButtons();
      const key = ({ abs: "abs", tcs: "tcs", esc: "esc", auto_gears: "autoGears" } as const)[cb.dataset.a as "abs" | "tcs" | "esc" | "auto_gears"];
      if (menu && key) tellMenu({ assists: { ...menu.settings.assists, [key]: cb.checked } });
    };
  });
  clutchSel.onchange = () => {
    if (isReplay()) return;
    clutchSel.blur(); // hand the keys back to the game
    const manual = clutchSel.value === "manual";
    const a = sim.assists(app.player) as unknown as Record<string, unknown>;
    a.auto_clutch = !manual;
    sim.setAssists(app.player, a as never);
    const o = assistOverrides();
    o.auto_clutch = !manual;
    store.set(ASSIST_KEY, JSON.stringify(o));
    store.set(PRESET_KEY, "");
    syncPresetButtons();
  };
  document.querySelectorAll<HTMLButtonElement>("button.preset").forEach((b) => {
    b.onclick = () => choosePreset(b.dataset.p!);
  });
  document.querySelectorAll<HTMLButtonElement>(".bench").forEach((b) => {
    b.onclick = () => startBench(Number(b.dataset.n));
  });

  $("rec").onclick = () => !isReplay() && sim.recordStart();
  $("rec-stop").onclick = () => !isReplay() && stopRecording();
  $("rp-last").onclick = () => {
    const b64 = store.get(LAST_REPLAY_KEY);
    if (b64) loadReplay(base64ToBytes(b64));
    else if (app.lastReplay) loadReplay(app.lastReplay);
  };
  ($("rp-file") as HTMLInputElement).onchange = async (e) => {
    const f = (e.target as HTMLInputElement).files?.[0];
    if (f) loadReplay(new Uint8Array(await f.arrayBuffer()));
  };
  $("rp-dl").onclick = () => {
    const bytes = app.lastReplay ?? (sim.recording() ? stopRecording() : null);
    if (bytes) download(bytes, `wreckpursuit-${Date.now()}.replay.bin`, "application/octet-stream");
  };
  $("rp-csv").onclick = () => {
    const bytes = app.lastReplay;
    if (bytes) download(sim.replayCsv(bytes, app.focus, 4), `telemetry-${app.focus}.csv`, "text/csv");
  };
  $("rp-exit").onclick = () => (app.scenarioFile ? runScenario(app.scenarioFile) : startFreeDrive());
  $("rp-play").onclick = () => togglePause();
  $("rp-step").onclick = () => frameStep(4);
  $("rp-back").onclick = () => frameStep(-4);
  ($("rp-speed") as HTMLSelectElement).onchange = (e) => {
    if (isReplay() && !app.paused) sim.replayPlay(Number((e.target as HTMLSelectElement).value));
  };
  const scrub = $("rp-scrub") as HTMLInputElement;
  scrub.oninput = () => {
    if (!isReplay() || crash.playing) return;
    sim.replaySeek(Number(scrub.value));
    plots.clear();
  };

  const togglePause = () => {
    app.paused = !app.paused;
    if (isReplay()) sim.replayPlay(app.paused ? 0 : Number(($("rp-speed") as HTMLSelectElement).value));
  };
  const frameStep = (ticks: number) => {
    // Crash replays play a fixed window (a backwards seek would re-simulate the session).
    if (!isReplay() || crash.playing) return;
    app.paused = true;
    sim.replayPlay(0);
    const st = sim.replayStatus();
    if (st) sim.replaySeek(Math.max(0, st.tick + ticks));
  };

  const startBench = (n: number) => {
    // Spawns happen before the first tick, so they become part of the (replayable) scenario.
    startFreeDrive("debug_sedan", ($("mode") as HTMLSelectElement).value, false);
    // No crash replays on benches (and no snapshot cost in the numbers).
    sim.crashDetectEnable(false);
    const cars = sim.carIds();
    for (let i = 1; i < n; i++) {
      const col = i % 6;
      const row = Math.floor(i / 6);
      const id = sim.spawn(cars[i % cars.length], [(col - 2.5) * 8, row * 12 + 12, 0], 0);
      sim.setInput(id, { ...app.input, throttle: 0.4 + 0.1 * (i % 5), steer: ((i % 7) - 3) / 6 });
    }
    sim.setInput(app.player, { ...app.input, throttle: 0.6, steer: 0.3 });
    syncViews();
    app.bench = { n, frames: 0, sim: 0, render: 0, t0: performance.now(), tick0: -1 };
    rig.setMode("orbit", app.views.get(app.player)?.root, n > 1 ? [-35, -30, 40] : undefined);
  };

  // ----- keyboard -------------------------------------------------------------------------

  /** Run a rebindable edge action (keyboard keydown or gamepad button press). */
  const doAction = (id: ActionId): void => {
    switch (id) {
      case "cameraCycle":
        rig.cycle(app.views.get(app.focus)?.root);
        break;
      case "reset":
        // Reset in place (playtest row 65): upright on the nearest clear lane, repaired outside a
        // career run; heat, pursuit and race progress carry on.
        if (!isReplay()) {
          const r = sim.resetInPlace(app.player);
          hud.note(`${(sim.worldInfo()[W.tick] / 240).toFixed(2)}s reset in place${r.on_lane ? "" : " (no lane near)"} — ${r.repaired ? "repaired" : "career: repairs are bought in the garage"}`);
        }
        break;
      case "headlights":
        toggleHeadlights();
        break;
      case "pause":
        // Replays: play / pause. Driving: the pause menu (gamepad Start / P).
        if (isReplay() || !menu) togglePause();
        else if (canOpenMenu()) menu.openPause();
        break;
      case "map":
        map.toggle();
        break;
      case "followNext": {
        const ids = [...app.views.keys()];
        app.focus = ids[(ids.indexOf(app.focus) + 1) % ids.length];
        rig.setMode(rig.mode, app.views.get(app.focus)?.root);
        break;
      }
      case "repairBill":
        ui.toggleBill();
        break;
      case "replayCrash":
        crash.action();
        break;
      case "help":
        hud.toggleHelp();
        break;
      case "controls":
        controls.toggle();
        break;
      case "mute":
        audio.toggleMute();
        break;
      default:
        // Analogue / held actions and gears are read every frame by readInput.
        break;
    }
  };

  installKeyboard((e) => {
    if (e.repeat) return;
    // Rebindable actions win over the fixed keys (debug layers, camera angles, frame step).
    const actions = actionsForKey(getBindings(), e.code);
    if (actions.length) {
      // A game key does not also scroll, open browser find (F3, /) or move focus (Tab).
      e.preventDefault();
      for (const a of actions) doAction(a);
      return;
    }
    const layer = layers.byKey(e.code);
    if (layer) return layers.toggle(layer.id);
    const n = Number(e.key);
    if (n >= 1 && n <= CAMERA_MODES.length) return rig.choose(CAMERA_MODES[n - 1], app.views.get(app.focus)?.root);
    switch (e.code) {
      case "Period":
        frameStep(4);
        break;
      case "Comma":
        frameStep(-4);
        break;
      case "BracketLeft":
      case "BracketRight": {
        const sel = $("rp-speed") as HTMLSelectElement;
        sel.selectedIndex = Math.max(0, Math.min(sel.options.length - 1, sel.selectedIndex + (e.code === "BracketLeft" ? -1 : 1)));
        sel.dispatchEvent(new Event("change"));
        break;
      }
    }
  });

  // ----- game menu (UI kit, playtest row 68) ---------------------------------------------
  // Main / pause / settings for players. The developer panels stay behind Settings → Developer
  // (on by default in the testbed preview, off in the demo build and with ?menu=1).
  const demo = document.body.classList.contains("demo");
  const menuOnLoad = demo || params.get("menu") === "1";
  const devParam = params.get("dev");
  const devDefault = !menuOnLoad;
  // First run: seed the menu from what this browser already chose (assist boxes, audio panel, gfx).
  const seed = (): GameSettings => {
    const d = defaultGameSettings(devDefault);
    if (hasStoredSettings()) return d;
    // A touch device starts with the touch assists (auto gears, TCS, steer help), not the
    // desktop "off" defaults; the menu then stores them as this browser's choice.
    const o = touch?.visible ? { ...assistOverrides(), tcs: true, auto_gears: true, ...savedAssistOverrides() } : assistOverrides();
    const a = sim.assistPreset(app.source === "Touch" ? "Gamepad" : app.source) as unknown as Record<string, unknown>;
    d.assists = { ...d.assists, abs: Boolean(o.abs ?? a.abs), tcs: Boolean(o.tcs), esc: Boolean(o.esc), autoGears: Boolean(o.auto_gears) };
    d.volumes = audio.mix();
    d.gfx = gfxMode();
    return d;
  };
  const DEBUG_LAYERS = ["lattice", "stress", "tireForces", "suspension", "slipColour", "contacts", "cgVelocity", "intents", "components", "regions", "events", "telemetry", "inputs"];
  const applySettings = (next: GameSettings, prev: GameSettings | null) => {
    gameSettings = next;
    const dev = devParam === "1" ? true : devParam === "0" ? false : next.developer;
    document.body.classList.toggle("wp-player", !dev);
    if (dev) document.body.classList.remove("demo");
    layers.suppress(dev ? [] : DEBUG_LAYERS);
    // Graphics (an explicit ?gfx= wins on load).
    if (prev || !params.get("gfx")) scene.quality.setMode(next.gfx);
    const drawParam = Number(params.get("draw"));
    scene.drawDistanceM = next.drawDistanceM > 0 ? next.drawDistanceM : drawParam > 0 ? drawParam : scene.quality.drawDistanceM;
    weather.rainFraction = scene.quality.rainFraction;
    audio.setMix(next.volumes);
    map.hud.setUnits(next.units);
    // Assists go through the same overrides as the developer panel's boxes.
    if (!prev || JSON.stringify(prev.assists) !== JSON.stringify(next.assists)) {
      const o = assistOverrides() as Record<string, boolean>;
      Object.assign(o, { abs: next.assists.abs, tcs: next.assists.tcs, esc: next.assists.esc, auto_gears: next.assists.autoGears, steer_help: next.assists.steerHelp });
      store.set(ASSIST_KEY, JSON.stringify(o));
      if (prev && !panelToMenu) {
        // An assist changed in the game menu is a custom setup, like a hand-ticked box.
        store.set(PRESET_KEY, "");
        syncPresetButtons();
        applyAssistPreset(app.source);
      }
    }
    const modeSel = $("mode") as HTMLSelectElement;
    if (!prev) modeSel.value = next.damageMode;
    else if (prev.damageMode !== next.damageMode && !isReplay() && !sim.careerRunActive()) {
      modeSel.value = next.damageMode;
      sim.setDamageMode(next.damageMode);
    }
    if (next.camera && (!prev || prev.camera !== next.camera) && CAMERA_MODES.includes(next.camera as CameraMode)) {
      rig.choose(next.camera as CameraMode, app.views.get(app.focus)?.root);
    }
    layout.update();
  };
  /** Something else owns the screen or the Esc key right now. */
  const canOpenMenu = () => !menu?.isOpen && !map.isOpen && !controls.isOpen && !garage.open && !runEnd.open && !raceView.cards && !isReplay();
  let menuPausedBefore = false;
  let menuShown = false;
  menu = mountGameMenu(overlay, {
    settings: loadGameSettings(seed()),
    developerDefault: devDefault,
    countersteer: !!countersteerSetter,
    cameras: CAMERAS.filter((c) => c.mode !== "orbit").map((c) => ({ id: c.mode, label: c.label })),
    host: {
      play: (choice: PlayChoice) => {
        // Quick play runs outside the career; the career lives behind Garage.
        if (choice === "chase") startCityChase();
        else if (choice === "chaseRain") startCityChase(undefined, undefined, true);
        else if (choice === "free") startFreeDrive(undefined, undefined, true);
        else if (choice === "races") raceView.openPicker();
        else openGarage();
      },
      restart: () => restartRun(),
      garage: () => openGarage(),
      quitToMain: () => {
        if (sim.careerRunActive()) sim.careerEndRun();
        persistCareer();
        runEnd.reset();
      },
      openControls: () => controls.open(),
      controlsOpen: () => controls.isOpen,
      apply: applySettings,
      onOpenChange: (open) => {
        // Pause the sim (and a replay's playback) while any menu screen is up; block driving input.
        if (open && !menuShown) {
          menuPausedBefore = app.paused;
          app.paused = true;
          if (isReplay()) sim.replayPlay(0);
        } else if (!open && menuShown) {
          app.paused = menuPausedBefore;
          if (isReplay() && !app.paused) sim.replayPlay(Number(($("rp-speed") as HTMLSelectElement).value));
        }
        menuShown = open;
        setInputBlocked(open);
      },
    },
  });
  // Esc opens the pause menu, unless the map, a panel or the garage takes it (they close on Esc).
  // Capture phase and registered before them, so this sees the state before they react.
  window.addEventListener(
    "keydown",
    (e) => {
      if (e.code !== "Escape" || e.repeat || !canOpenMenu()) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      menu!.openPause();
    },
    { capture: true },
  );
  if (demo) {
    // The demo page's own start menu is replaced by the game menu.
    document.getElementById("demo-menu")?.remove();
    document.getElementById("demo-open")?.remove();
  }

  // ----- start ----------------------------------------------------------------------------

  // A remembered preset also picks the damage mode new sessions start in.
  const remembered = storedPreset();
  if (remembered) ($("mode") as HTMLSelectElement).value = sim.assistPresetNamed(remembered, presetSource(app.source)).mode;

  const camParam = params.get("cam") as CameraMode | null;
  if (camParam && CAMERA_MODES.includes(camParam)) rig.mode = camParam;
  const scParam = params.get("scenario");
  if (scParam) {
    scSel.value = scParam;
    await runScenario(scParam, Number(params.get("speed") ?? 0));
  } else if (params.get("race")) {
    startRace(params.get("race")!, DEFAULT_CAR);
  } else if (params.get("chase")) {
    startCityChase("debug_sedan", undefined, params.get("chase") === "rain");
  } else {
    startFreeDrive(DEFAULT_CAR);
  }
  if (params.get("bench")) startBench(Number(params.get("bench")));
  // First frame only once the session's car meshes are in (bounded: a slow or missing file
  // falls back to the proxy and swaps in when it arrives).
  await Promise.race([preloadCarMeshes(lod.models.cars), new Promise((r) => setTimeout(r, 5000))]);
  if (params.get("replay") === "last") $("rp-last").click();
  if (menuOnLoad) menu.openMain();

  // ----- frame loop -----------------------------------------------------------------------

  let last = performance.now();
  let fps = 60;
  let simMs = 0;
  let renderMs = 0;
  let frameNo = 0;
  const rtf = new RealTimeMeter();
  const sirenPos = new THREE.Vector3();
  const NO_TEL = new Float32Array(0);

  function frame(now: number): void {
    const wallDt = (now - last) / 1000;
    // Cap one frame's catch-up at the session's own limit (MAX_TICKS_PER_STEP = 0.25 s): a
    // tighter cap turned every slow frame into lost sim time (slow motion).
    const dt = Math.min(wallDt, 0.25);
    last = now;
    fps += (1 / Math.max(dt, 1e-3) - fps) * 0.05;
    scene.quality.frame(wallDt * 1000);
    const prof = app.prof;
    prof.begin();
    let replay = isReplay();

    // Touch controls: hidden under menus / full-screen panels (they sit above #overlay), only
    // map + pause while paused or with the full map open.
    touch?.setCovered(!!menu?.isOpen || controls.isOpen || garage.open || runEnd.open || !!raceView.cards);
    touch?.setDriving(!replay && !app.paused && !map.isOpen && !app.bench);
    const { input, source, edges } = readInput(dt);
    for (const a of edges) doAction(a);
    app.input = input;
    if (source !== app.source) {
      app.source = source;
      if (!app.scenario && !app.bench) applyAssistPreset(source);
    }
    // The full city map pauses driving input (the sim keeps running): coast with light brakes.
    if (!replay && !app.bench) sim.setInput(app.player, map.isOpen ? { ...input, throttle: 0, steer: 0, brake: 0.35, handbrake: 0, gearUp: false, gearDown: false } : input);

    prof.lap("input");
    const t0 = performance.now();
    crash.beforeStep(app.paused);
    if (!app.paused || replay) app.alpha = sim.step(app.paused && !replay ? 0 : dt);
    const t1 = performance.now();
    simMs += (t1 - t0 - simMs) * 0.1;
    // Crash replay: leave at the end of its window; live: a new crash, the offer, auto play.
    crash.afterStep();
    if (!sim.crashReplayActive() && !isReplay()) {
      const c = sim.crashTake();
      if (c) crash.onCrash(c);
      crash.liveFrame(sim.worldInfo()[W.tick]);
    }
    replay = isReplay();
    // One world_info() per frame after the step (it was fetched ~7 times a frame).
    const wi = sim.worldInfo();
    rtf.push(wallDt, wi[W.tick], !app.paused && !replay && !app.bench);
    prof.lap("sim");

    const meta = sim.vehicleMeta();
    const trafficDraw = sim.trafficDraw();
    const policeDraw = sim.policeDraw();
    syncViews(meta, lod.paintOfEntity(trafficDraw));
    const tS = wi[W.tick] / 240;
    for (let o = 0; o < meta.length; o += META_STRIDE) app.views.get(meta[o])?.setMeta(meta[o + 2], tS);
    const cur = sim.states();
    const prev = sim.statesPrev();
    const forcesOn = layers.on("tireForces");
    let focusTel: Float32Array | null = null;
    let focusRec = -1;
    for (let o = 0, i = 0; o < cur.length; o += STATE_STRIDE, i++) {
      const id = cur[o + S.entity];
      const view = app.views.get(id);
      if (!view) continue;
      // vehicle_states_prev() lists the same entities in the same (ascending) order.
      view.focused = id === app.focus;
      // Telemetry: the focus car (HUD, plots) and, with the force arrows on, cars that draw
      // them (background traffic / police only when focused).
      const tel = view.focused || (forcesOn && !view.background) ? sim.telemetry(id) : NO_TEL;
      view.update(cur, o, prev.length > o ? prev : cur, o, app.alpha, tel, app.paused && !replay ? 0 : dt);
      if (id === app.focus) {
        focusTel = tel;
        focusRec = o;
      }
    }
    prof.lap("views");
    // One siren light, on the siren car nearest the camera (sim space: Three (x, y, z) = (x, -z, y)).
    {
      const c = rig.camera.position;
      let best: VehicleView | null = null;
      let bestD = Infinity;
      for (const v of app.views.values()) {
        if (!v.sirenOn) continue;
        const p = v.root.position;
        const d = (p.x - c.x) ** 2 + (p.y + c.z) ** 2 + (p.z - c.y) ** 2;
        if (d < bestD) [best, bestD] = [v, d];
      }
      if (best) scene.setSiren(best.sirenPosition(sirenPos), best.sirenColour, best.sirenIntensity);
      else scene.setSiren(null);
    }
    const events = sim.eventsDrain();
    for (const ev of events) {
      const kind = Object.keys(ev)[0];
      if (kind) app.eventCounts[kind] = (app.eventCounts[kind] ?? 0) + 1;
    }
    scene.addEvents(events);
    panelEvents(events);
    stepDebris(app.paused && !replay ? 0 : dt);
    if (!crash.playing) hud.pushEvents(events, wi[W.tick]);
    prof.lap("events");
    if (focusTel && focusRec >= 0) {
      const fv = app.views.get(app.focus);
      if (fv) fx.componentHealth(app.focus, focusTel, T.componentBase, fv.info.components);
    }
    fx.frame(events, cur, app.views, app.paused && !replay ? 0 : dt, wi[W.night]);
    prof.lap("fx");
    // The game UI is hidden during a crash replay (and must not count its events twice).
    if (!crash.playing) {
      ui.frame(wi[W.tick], app.player, events, wi[W.hours], wi[W.mode] > 0.5 ? "Simulation" : "Arcade");
      map.frame(app.player, now);
      if (!replay) runEnd.update(sim.runResult(), now);
      // Career autosave: after each settlement (bust, escape, wreck) and every 20 s of a run.
      if (!replay && sim.careerRunActive()) {
        const seq = runEnd.report?.seq ?? 0;
        if (seq !== careerSavedSeq || now - careerSavedAt > 20000) {
          careerSavedSeq = seq;
          careerSavedAt = now;
          persistCareer();
        }
      }
    }
    if (raceView.raceId && !crash.playing) raceFrame(cur, meta, tS, frameNo % 6 === 0);
    prof.lap("ui");
    scene.update(dt, sim.intents());
    scene.updateHelicopters(policeDraw, tS);
    const wx = sim.weatherInfo();
    scene.setTimeOfDay(wi[W.night], wx[0] > 0.5 ? wx[7] : 1);
    prof.lap("traffic");
    weather.update(
      app.paused && !replay ? 0 : dt,
      rig.camera,
      wx,
      wi[W.night],
      () => ({ meta: sim.weatherGridMeta(), bytes: sim.weatherGrid() }),
      cur,
      sim.weatherSpray(),
      rig.mode === "hood" || rig.mode === "bumper" || rig.mode === "cockpit",
    );
    prof.lap("weather");
    scene.signals.update(sim.signalStates(), wi[W.night], rig.camera.position);
    prof.lap("traffic");

    const focusView = app.views.get(app.focus) ?? null;
    if (focusTel) {
      plots.push(wi[W.tick] / 240, focusTel);
      if (layers.on("telemetry")) plots.draw();
    }
    plots.canvas.style.display = layers.on("telemetry") ? "" : "none";
    prof.lap("ui");
    rig.lookBack = actionHeld("lookBack");
    if (crash.playing) crash.camera(rig.camera, wi[W.tick], dt);
    else
      rig.update(
        focusView?.root ?? null,
        focusView?.info.body_size_m ?? [2, 4.5, 1.4],
        focusRec >= 0 ? cur[focusRec + S.speed] : 0,
        dt,
        rig.mode === "cinematic" ? nearestPolice(focusView) : null,
      );
    prof.lap("camera");
    audio.update(rig.camera, sim.lastEventsJson(), app.paused && !replay ? 0 : dt, app.focus);
    prof.lap("audio");
    // Traffic and police after the camera moved: the fleet picks mesh detail by camera distance.
    lod.update(trafficDraw, policeDraw, app.views, rig.camera, app.views.get(app.player)?.root.position ?? null, tS);
    props.update(sim.propsMoved(), rig.camera);
    policeTools.update(() => sim.mapDynamic(app.player), (x, y) => scene.groundZ(x, y), now);
    prof.lap("traffic");

    const inCity = wi[W.city] > 0.5;
    const cityId = inCity ? sim.cityId() : null;
    if (cityId) style.ensureCity(cityId, () => sim.mapStaticJson());
    style.update(dt, wi[W.hours], wi[W.night], wx[0] > 0.5 ? wx[7] : 1, { on: wx[0] > 0.5, cloud: wx[6] }, rig.camera, cityId);
    const r0 = performance.now();
    scene.render(rig.camera);
    const r1 = performance.now();
    prof.lap("render");
    renderMs += (r1 - r0 - renderMs) * 0.1;
    const p = app.perf;
    p.simMs = simMs;
    p.renderMs = renderMs;
    p.viewsMs += (r0 - t1 - p.viewsMs) * 0.1;
    p.frameMs += (wallDt * 1000 - p.frameMs) * 0.1;
    p.rtf = rtf.factor;

    if (!crash.playing && (frameNo++ % 6 === 0 || !focusView)) {
      hud.update({
        info: focusView?.info ?? null,
        tel: focusTel,
        // Show what the sim actually received (scripted / replayed input included).
        input: focusTel
          ? { ...app.input, throttle: focusTel[T.inThrottle], brake: focusTel[T.inBrake], steer: focusTel[T.inSteer], handbrake: focusTel[T.inHandbrake] }
          : app.input,
        source: replay ? "replay" : app.source,
        assists: replay ? null : sim.assists(app.player),
        mode: wi[W.mode] > 0.5 ? "Simulation" : "Arcade",
        rack: focusView ? sim.ffb(app.focus)[0] : 0,
        fps,
        simMs,
        renderMs,
        viewsMs: p.viewsMs,
        rtf: rtf.factor,
        gfx: scene.quality.label(),
        tick: wi[W.tick],
        hours: wi[W.hours],
        hash: sim.stateHash(),
        vehicles: app.views.size,
        replay,
        recording: wi[W.recording] > 0.5,
        energy: focusView ? sim.regionEnergy(app.focus) : null,
        regionsOfNodes: focusView ? regionsCache(app.focus) : null,
        rest: focusView ? restCache(app.focus) : null,
        camera: cameraLabel(rig.mode) + (rig.lookBack && rig.mode !== "orbit" && rig.mode !== "cinematic" ? " (look back)" : ""),
        units: gameSettings?.units,
      });
      updatePanels(replay);
    }
    layout.frame(now);
    prof.lap("ui");
    prof.end();
    if (app.bench) benchTick(t1 - t0, r1 - r0);
    requestAnimationFrame(frame);
  }

  /** Nearest police car to `view` (framed by the TV camera during pursuits). */
  const nearestPolice = (view: VehicleView | null) => {
    if (!view) return null;
    let best: VehicleView | null = null;
    let bestD = Infinity;
    for (const v of app.views.values()) {
      if (v === view || v.info.kind !== "Police") continue;
      const d = v.root.position.distanceToSquared(view.root.position);
      if (d < bestD) [best, bestD] = [v, d];
    }
    return best?.root ?? null;
  };

  const regionsCache = (id: number) => regions.get(id) ?? regions.set(id, sim.latticeRegions(id)).get(id)!;
  const restCache = (id: number) => rests.get(id) ?? rests.set(id, sim.latticeRest(id)).get(id)!;

  const esc = (t: string) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  const updatePanels = (replay: boolean) => {
    const st = sim.replayStatus();
    const status = $("rp-status");
    if (replay && st) {
      scrub.min = "0";
      scrub.max = String(st.end_tick);
      if (document.activeElement !== scrub) scrub.value = String(st.tick);
      const ok = st.drift_count === 0;
      const atEnd = st.tick >= st.end_tick;
      status.innerHTML =
        `replay "${esc(st.scenario)}" ${st.mode} · tick ${st.tick}/${st.end_tick} · ${(st.tick / 240).toFixed(2)} s · x${app.paused ? 0 : st.speed}` +
        `<br>tick hashes verified ${st.verified_ticks}/${st.tick} · ` +
        (ok
          ? `<span style="color:#2ecc40">no drift</span>`
          : `<span style="color:#ff4136">DRIFT ${st.drift_count} ticks, first ${st.drift_ticks.slice(0, 5).join(",")}</span>`) +
        (atEnd ? ` · final ${st.state_hash === st.recorded_final_hash ? "✓ matches" : "✗ differs"} ${st.state_hash}` : "");
    } else {
      status.textContent = sim.recording()
        ? `recording… (tick ${sim.worldInfo()[W.tick]})`
        : app.lastReplay
          ? `last replay ${app.lastReplay.length} bytes (stored; simcli replay <file> verifies it)`
          : "no replay loaded";
    }
    const res = sim.scenarioResult();
    const out = $("sc-result");
    if (res && !replay) {
      app.result = res;
      if (res.done) ui.sessionEnded();
      const row = (r: { expect: string; actual: string; passed: boolean }, pending: boolean) =>
        `<div class="${pending ? "pending" : r.passed ? "pass" : "fail"}">${r.passed ? "✓" : "✗"} ${esc(r.expect)} → ${esc(r.actual)}${pending ? " (pending)" : ""}</div>`;
      const sum = res.summary;
      out.innerHTML =
        `<b>${esc(res.name)}</b> ` +
        (res.done
          ? (res.pass ? '<span class="pass">PASS</span>' : '<span class="fail">FAIL</span>') +
            (res.matches_simcli === true
              ? ' <span class="pass" title="live end-of-script hash equals the headless simcli run">= simcli</span>'
              : res.matches_simcli === false
                ? ' <span class="fail">≠ simcli (live run diverged)</span>'
                : "")
          : `running ${((res.end_tick - res.tick) / 240).toFixed(1)} s left`) +
        (sum ? sum.expectations.map((r) => row(r, false)).join("") + sum.pending.map((r) => row(r, true)).join("") : "");
    } else if (!replay) {
      out.innerHTML = "";
    }
  };

  const benchTick = (simFrameMs: number, renderFrameMs: number) => {
    const b = app.bench!;
    const elapsed = performance.now() - b.t0;
    if (elapsed < 1000) return; // warm-up
    const tick = sim.worldInfo()[W.tick];
    if (b.tick0 < 0) {
      b.tick0 = tick;
      return;
    }
    b.frames++;
    b.sim += simFrameMs;
    b.render += renderFrameMs;
    // Normalise sim cost to one 60 Hz frame (4 physics ticks) so slow-rendering browsers,
    // which run more ticks per rendered frame, report comparable numbers.
    const ticks = Math.max(tick - b.tick0, 1);
    const result = {
      cars: b.n,
      frames: b.frames,
      ticks,
      simMsPer60HzFrame: (b.sim / ticks) * 4,
      renderMs: b.render / b.frames,
      fps: (b.frames * 1000) / (elapsed - 1000),
    };
    $("bench-out").textContent =
      `${b.n} cars: sim ${result.simMsPer60HzFrame.toFixed(3)} ms/60Hz-frame · render ${result.renderMs.toFixed(2)} ms · ${result.fps.toFixed(0)} fps`;
    (window as unknown as { __wpBench: unknown }).__wpBench = result;
  };

  // Automation hook (Playwright e2e, docs screenshots).
  (window as unknown as { __wp: unknown }).__wp = {
    sim,
    app,
    scene,
    /** Controls panel (rebinding) and the live bindings. */
    controls,
    /** Instant crash replay director (`crash.phase`, `crash.last`, `crash.accept()`). */
    crash,
    /** Settings hook (pause menu): play crash replays without asking. */
    setAutoCrashReplay,
    bindings: getBindings,
    /** City map HUD (UI kit): `map.hud.stats()`, `map.toggle()`. */
    map,
    /** Touch controls (src/touch.ts; null with `?touch=0`) and the steering-mode switch the
     * pause menu / settings use. */
    touch,
    setTouchSteerMode,
    touchSteerMode,
    /** Game menu (UI kit): `menu.openPause()`, `menu.screen`, `menu.settings`. */
    menu,
    /** HUD layout pass (src/ui/layout.ts). */
    layout,
    ready: true,
    stopRecording,
    loadReplay,
    runScenario,
    startFreeDrive,
    startCityChase,
    startBench,
    weather,
    /** Art style (render/style.ts): `style.id`, `style.baseFogDensity`, `style.lampCount(city)`. */
    style,
    /** Switch the art style ("greybox" | "lowpoly"; automation, docs shots). */
    setStyle,
    /** Pick a weather choice and restart the session with it (automation, docs shots). */
    setWeather: (choice: string) => {
      weatherSel.value = choice;
      weatherSel.dispatchEvent(new Event("change"));
    },
    setCamera: (m: CameraMode, offset?: [number, number, number]) => rig.setMode(m, app.views.get(app.focus)?.root, offset),
    cameraModes: CAMERA_MODES,
    rig,
    lookAt: (from: [number, number, number], to: [number, number, number]) => rig.lookAt(from, to),
    /** Camera near/far planes, m (docs overview shots; default 0.1 / 3000). */
    setClip: (near: number, far: number) => {
      rig.camera.near = near;
      rig.camera.far = far;
      rig.camera.updateProjectionMatrix();
    },
    toggleLayer: (id: string) => layers.toggle(id),
    ui,
    runEnd,
    restartRun,
    garage,
    careerApi,
    openGarage,
    startCareerRun,
    persistCareer,
    startRace,
    raceView,
    raceMarker,
    lookFrom: (eye: [number, number, number], target: [number, number, number]) => rig.lookFrom(eye, target),
    /** LOD director: `lod.log.start()` records per-frame draw states, `lod.log.summary()`. */
    lod,
    /** Parked cars / cones: `props.stats(rig.camera)`, `props.drawnPose(i)` (render/props.ts). */
    props,
    /** Car mesh fetch + parse times (performance.now() ms) per car id. */
    meshTimings,
    /** Crash effects: `fx.stats()`, `fx.setQuality("low" | "med" | "high")` (render/fx). */
    fx,
  };
  requestAnimationFrame(frame);
  if (abandonedBust) openGarage(abandonedBust);
}

main().catch((e) => {
  $("error").textContent = `failed to start: ${e}\n\nDid you run \`npm run wasm\` (or \`just web\`)?`;
  console.error(e);
});
