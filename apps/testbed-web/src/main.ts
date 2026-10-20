// WreckPursuit browser testbed: drives wp_wasm, renders vehicles + debug layers, HUD,
// telemetry, scenario menu and replay UI. See docs/integration/testbed.md.
//
// URL parameters (handy for automation): ?scenario=crash/head_on.ron&speed=60 runs a scenario,
// ?bench=24 spawns a 24-car benchmark, ?cam=orbit picks a camera, ?replay=last plays the last
// stored replay, ?city=procedural_12x12 picks the city (default vesper_v2), ?chase=1 starts the
// city chase.
import { loadWasm, sim, type DriverInput, type Scenario, type ScenarioResult } from "./wasm/sim";
import { META_STRIDE, STATE_STRIDE, S, T, VEHICLE_KIND, W } from "./wasm/layout";
import { installKeyboard, isHeld, readInput, toggleHeadlights, type InputSource } from "./input";
import { layers, store, bytesToBase64, base64ToBytes } from "./settings";
import { SceneView } from "./scene";
import { VehicleView } from "./vehicle_view";
import { clearDebris, panelEvents, stepDebris } from "./render/panels";
import { CameraRig, CAMERA_MODES, cameraLabel, type CameraMode } from "./cameras";
import { Hud } from "./hud";
import { TelemetryPlots } from "./telemetry";
import { AudioSystem } from "./audio";
import { UiOverlay } from "./ui/overlay";
import { RunEndView } from "./ui/runEnd";
import { RealTimeMeter } from "./rtf";

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
  eventCounts: {},
};

async function main(): Promise<void> {
  await loadWasm();
  const overlay = $("overlay");
  const scene = new SceneView(document.body);
  const rig = new CameraRig(scene.renderer.domElement);
  const hud = new Hud(overlay);
  const plots = new TelemetryPlots(overlay);
  const audio = new AudioSystem(scene.world, overlay);
  const ui = new UiOverlay(overlay);
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
  });
  const citySel = $("city-map") as HTMLSelectElement;
  const cityParam = params.get("city");
  if (cityParam && CITY_IDS.includes(cityParam)) citySel.value = cityParam;

  // ----- session management --------------------------------------------------------------

  const rebuildViews = () => {
    for (const v of app.views.values()) v.dispose();
    app.views.clear();
    clearDebris();
    regions.clear();
    rests.clear();
    scene.clearContacts();
    hud.clearEvents();
    app.eventCounts = {};
    plots.clear();
    ui.reset();
    runEnd.reset();
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
    ($("city") as HTMLInputElement).checked = city;
    ($("mode") as HTMLSelectElement).value = modeName();
    const first = app.views.get(app.player);
    if (first) ($("car") as HTMLSelectElement).value = first.info.car_id;
    if (app.scenarioFile) ($("sc-select") as HTMLSelectElement).value = app.scenarioFile;
    syncAssistBoxes();
    rig.setMode(rig.mode, app.views.get(app.focus)?.root);
  };

  /** One view per sim vehicle — scenario cars plus whatever the sim spawns itself (police
   * units, promoted traffic) — created and disposed as vehicles come and go. */
  const syncViews = (meta: Float32Array = sim.vehicleMeta()) => {
    const live = new Set<number>();
    for (let o = 0; o < meta.length; o += META_STRIDE) {
      const id = meta[o];
      live.add(id);
      if (app.views.has(id)) continue;
      const kind = meta[o + 1];
      const colour =
        kind === VEHICLE_KIND.police ? 0xe9ebf0 : kind === VEHICLE_KIND.traffic ? 0x7a8a99 : COLOURS[(id - 1) % COLOURS.length];
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
    sim.init(1, json, false);
    app.scenario = null;
    rebuildViews();
    applyAssistPreset(app.source);
  };

  /** City chase preset: procedural city at 23:00, police + traffic, player already speeding. */
  const startCityChase = (
    car = ($("car") as HTMLSelectElement).value || DEFAULT_CAR,
    mode = ($("mode") as HTMLSelectElement).value,
  ) => {
    app.scenarioFile = null;
    app.scenario = null;
    app.result = null;
    app.bench = null;
    sim.init(0, sim.cityChaseCityJson(car, mode, citySel.value), false);
    rebuildViews();
    applyAssistPreset(app.source);
    // Back to the viewer's own angle (near chase unless they picked another one).
    rig.setMode(rig.preferred, app.views.get(app.focus)?.root);
  };

  /** "Restart" on the end-of-run panel: the same session again (the account carries over). */
  const restartRun = () => {
    const sc = sim.scenario();
    const car = sc.entities[0]?.car;
    if (sc.name === "city_chase") startCityChase(car, sc.mode);
    else startFreeDrive(car, sc.mode);
  };

  const runScenario = async (file: string, speedKmh?: number) => {
    const text = await (await fetch(`scenarios/${file}`)).text();
    let json = sim.scenarioFromRon(text);
    if (speedKmh && speedKmh > 0) json = sim.scenarioWithSpeed(json, speedKmh / 3.6);
    app.scenarioFile = file;
    app.result = null;
    app.bench = null;
    sim.init(0, json, true);
    app.scenario = sim.scenario();
    rebuildViews();
  };

  // Owner default (playtest row 29): TCS, ESC and automatic gears start OFF; any assist box the
  // viewer ticks or unticks is remembered (localStorage) and wins over the preset.
  const ASSIST_KEY = "wp.assistOverrides";
  const assistOverrides = (): Record<string, boolean> => {
    const base: Record<string, boolean> = { tcs: false, esc: false, auto_gears: false };
    try {
      return { ...base, ...(JSON.parse(store.get(ASSIST_KEY) ?? "{}") as Record<string, boolean>) };
    } catch {
      return base;
    }
  };
  const applyAssistPreset = (source: InputSource) => {
    if (isReplay()) return;
    try {
      const preset = sim.assistPreset(source) as unknown as Record<string, unknown>;
      Object.assign(preset, assistOverrides());
      sim.setAssists(app.player, preset as never);
      syncAssistBoxes();
    } catch (e) {
      console.warn(e);
    }
  };

  const syncAssistBoxes = () => {
    if (isReplay()) return;
    const a = sim.assists(app.player) as unknown as Record<string, unknown>;
    document.querySelectorAll<HTMLInputElement>("#assist-row input").forEach((cb) => {
      cb.checked = Boolean(a[cb.dataset.a!]);
    });
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
  ($("mode") as HTMLSelectElement).onchange = (e) => {
    // Live switch (Sim::set_mode_table), logged as a replay op; existing damage is kept.
    if (isReplay()) return void (($("mode") as HTMLSelectElement).value = modeName());
    sim.setDamageMode((e.target as HTMLSelectElement).value);
  };
  ($("city") as HTMLInputElement).onchange = () => startFreeDrive();
  citySel.onchange = () => {
    if (sim.scenario()?.name === "city_chase") startCityChase();
    else if (($("city") as HTMLInputElement).checked) startFreeDrive();
  };
  document.querySelectorAll<HTMLInputElement>("#assist-row input").forEach((cb) => {
    cb.onchange = () => {
      if (isReplay()) return;
      const a = sim.assists(app.player) as unknown as Record<string, unknown>;
      a[cb.dataset.a!] = cb.checked;
      sim.setAssists(app.player, a as never);
      const o = assistOverrides();
      o[cb.dataset.a!] = cb.checked;
      store.set(ASSIST_KEY, JSON.stringify(o));
    };
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
    if (!isReplay()) return;
    sim.replaySeek(Number(scrub.value));
    plots.clear();
  };

  const togglePause = () => {
    app.paused = !app.paused;
    if (isReplay()) sim.replayPlay(app.paused ? 0 : Number(($("rp-speed") as HTMLSelectElement).value));
  };
  const frameStep = (ticks: number) => {
    if (!isReplay()) return;
    app.paused = true;
    sim.replayPlay(0);
    const st = sim.replayStatus();
    if (st) sim.replaySeek(Math.max(0, st.tick + ticks));
  };

  const startBench = (n: number) => {
    // Spawns happen before the first tick, so they become part of the (replayable) scenario.
    startFreeDrive("debug_sedan", ($("mode") as HTMLSelectElement).value, false);
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

  installKeyboard((e) => {
    if (e.repeat) return;
    const layer = layers.byKey(e.code);
    if (layer) return layers.toggle(layer.id);
    const n = Number(e.key);
    if (n >= 1 && n <= CAMERA_MODES.length) return rig.choose(CAMERA_MODES[n - 1], app.views.get(app.focus)?.root);
    switch (e.code) {
      case "KeyC":
        rig.cycle(app.views.get(app.focus)?.root);
        break;
      case "KeyR":
        if (!isReplay()) sim.reset(app.player);
        break;
      case "KeyO":
        toggleHeadlights();
        break;
      case "KeyP":
        togglePause();
        break;
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
      case "Tab": {
        e.preventDefault();
        const ids = [...app.views.keys()];
        app.focus = ids[(ids.indexOf(app.focus) + 1) % ids.length];
        rig.setMode(rig.mode, app.views.get(app.focus)?.root);
        break;
      }
      case "KeyF":
        ui.toggleBill();
        break;
      case "F1":
      case "Slash":
        e.preventDefault();
        hud.toggleHelp();
        break;
    }
  });

  // ----- start ----------------------------------------------------------------------------

  rig.onPadNext = () => rig.cycle(app.views.get(app.focus)?.root);
  const camParam = params.get("cam") as CameraMode | null;
  if (camParam && CAMERA_MODES.includes(camParam)) rig.mode = camParam;
  const scParam = params.get("scenario");
  if (scParam) {
    scSel.value = scParam;
    await runScenario(scParam, Number(params.get("speed") ?? 0));
  } else if (params.get("chase")) {
    startCityChase("debug_sedan");
  } else {
    startFreeDrive(DEFAULT_CAR);
  }
  if (params.get("bench")) startBench(Number(params.get("bench")));
  if (params.get("replay") === "last") $("rp-last").click();

  // ----- frame loop -----------------------------------------------------------------------

  let last = performance.now();
  let fps = 60;
  let simMs = 0;
  let renderMs = 0;
  let frameNo = 0;
  const rtf = new RealTimeMeter();

  function frame(now: number): void {
    const wallDt = (now - last) / 1000;
    // Cap one frame's catch-up at the session's own limit (MAX_TICKS_PER_STEP = 0.25 s): a
    // tighter cap turned every slow frame into lost sim time (slow motion).
    const dt = Math.min(wallDt, 0.25);
    last = now;
    fps += (1 / Math.max(dt, 1e-3) - fps) * 0.05;
    const replay = isReplay();

    const { input, source } = readInput(dt);
    app.input = input;
    if (source !== app.source) {
      app.source = source;
      if (!app.scenario && !app.bench) applyAssistPreset(source);
    }
    if (!replay && !app.bench) sim.setInput(app.player, input);

    const t0 = performance.now();
    if (!app.paused || replay) app.alpha = sim.step(app.paused && !replay ? 0 : dt);
    const t1 = performance.now();
    simMs += (t1 - t0 - simMs) * 0.1;
    rtf.push(wallDt, sim.worldInfo()[W.tick], !app.paused && !replay && !app.bench);

    const meta = sim.vehicleMeta();
    syncViews(meta);
    let physicalTraffic = false;
    const tS = sim.worldInfo()[W.tick] / 240;
    for (let o = 0; o < meta.length; o += META_STRIDE) {
      app.views.get(meta[o])?.setMeta(meta[o + 2], tS);
      physicalTraffic ||= meta[o + 1] === VEHICLE_KIND.traffic;
    }
    const cur = sim.states();
    const prev = sim.statesPrev();
    let focusTel: Float32Array | null = null;
    let focusRec = -1;
    for (let o = 0, i = 0; o < cur.length; o += STATE_STRIDE, i++) {
      const id = cur[o + S.entity];
      const view = app.views.get(id);
      if (!view) continue;
      // vehicle_states_prev() lists the same entities in the same (ascending) order.
      const tel = sim.telemetry(id);
      view.update(cur, o, prev.length > o ? prev : cur, o, app.alpha, tel, app.paused && !replay ? 0 : dt);
      if (id === app.focus) {
        focusTel = tel;
        focusRec = o;
      }
    }
    const events = sim.eventsDrain();
    for (const ev of events) {
      const kind = Object.keys(ev)[0];
      if (kind) app.eventCounts[kind] = (app.eventCounts[kind] ?? 0) + 1;
    }
    scene.addEvents(events);
    panelEvents(events);
    stepDebris(app.paused && !replay ? 0 : dt);
    hud.pushEvents(events, sim.worldInfo()[W.tick]);
    {
      const w = sim.worldInfo();
      ui.frame(w[W.tick], app.player, events, w[W.hours], modeName());
      if (!replay) runEnd.update(sim.runResult(), now);
    }
    scene.update(dt, sim.intents());
    // Promoted (physical) agents are drawn as sim vehicles once the sim spawns them.
    scene.updateAgents(sim.trafficAgents(), physicalTraffic);
    scene.updatePoliceUnits(sim.policeUnits(), tS);
    const wi = sim.worldInfo();
    scene.setTimeOfDay(wi[W.night]);
    scene.updateHelicopterBeams(sim.policeHelicopters(), wi[W.night]);
    scene.signals.update(sim.signalStates(), wi[W.night]);

    const focusView = app.views.get(app.focus) ?? null;
    if (focusTel) {
      plots.push(wi[W.tick] / 240, focusTel);
      if (layers.on("telemetry")) plots.draw();
    }
    plots.canvas.style.display = layers.on("telemetry") ? "" : "none";
    rig.lookBack = isHeld("KeyV");
    rig.update(
      focusView?.root ?? null,
      focusView?.info.body_size_m ?? [2, 4.5, 1.4],
      focusRec >= 0 ? cur[focusRec + S.speed] : 0,
      dt,
      rig.mode === "cinematic" ? nearestPolice(focusView) : null,
    );
    audio.update(rig.camera, events, app.paused && !replay ? 0 : dt, app.focus);

    const r0 = performance.now();
    scene.render(rig.camera);
    const r1 = performance.now();
    renderMs += (r1 - r0 - renderMs) * 0.1;
    const p = app.perf;
    p.simMs = simMs;
    p.renderMs = renderMs;
    p.viewsMs += (r0 - t1 - p.viewsMs) * 0.1;
    p.frameMs += (wallDt * 1000 - p.frameMs) * 0.1;
    p.rtf = rtf.factor;

    if (frameNo++ % 6 === 0 || !focusView) {
      hud.update({
        info: focusView?.info ?? null,
        tel: focusTel,
        // Show what the sim actually received (scripted / replayed input included).
        input: focusTel
          ? { ...app.input, throttle: focusTel[T.inThrottle], brake: focusTel[T.inBrake], steer: focusTel[T.inSteer], handbrake: focusTel[T.inHandbrake] }
          : app.input,
        source: replay ? "replay" : app.source,
        assists: replay ? null : sim.assists(app.player),
        mode: modeName(),
        rack: focusView ? sim.ffb(app.focus)[0] : 0,
        fps,
        simMs,
        renderMs,
        viewsMs: p.viewsMs,
        rtf: rtf.factor,
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
      });
      updatePanels(replay);
    }
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
    ready: true,
    stopRecording,
    loadReplay,
    runScenario,
    startFreeDrive,
    startCityChase,
    startBench,
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
    lookFrom: (eye: [number, number, number], target: [number, number, number]) => rig.lookFrom(eye, target),
  };
  requestAnimationFrame(frame);
}

main().catch((e) => {
  $("error").textContent = `failed to start: ${e}\n\nDid you run \`npm run wasm\` (or \`just web\`)?`;
  console.error(e);
});
