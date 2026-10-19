// WreckPursuit browser testbed: drives wp_wasm, renders vehicles + debug layers, HUD,
// telemetry, scenario menu and replay UI. See docs/integration/testbed.md.
//
// URL parameters (handy for automation): ?scenario=crash/head_on.ron&speed=60 runs a scenario,
// ?bench=24 spawns a 24-car benchmark, ?cam=orbit picks a camera, ?replay=last plays the last
// stored replay.
import { loadWasm, sim, type DriverInput, type Scenario, type ScenarioResult } from "./wasm/sim";
import { STATE_STRIDE, S, T, W } from "./wasm/layout";
import { installKeyboard, readInput, toggleHeadlights, type InputSource } from "./input";
import { layers, store, bytesToBase64, base64ToBytes } from "./settings";
import { SceneView } from "./scene";
import { VehicleView } from "./vehicle_view";
import { CameraRig, CAMERA_MODES, type CameraMode } from "./cameras";
import { Hud } from "./hud";
import { TelemetryPlots } from "./telemetry";
import { AudioSystem } from "./audio";

const LAST_REPLAY_KEY = "wp.testbed.lastReplay.v1";
const COLOURS = [0xd04a2a, 0x2a7fd0, 0x3fb950, 0xd0a92a, 0x9b59b6, 0x1abc9c];
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const params = new URLSearchParams(location.search);

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
};

async function main(): Promise<void> {
  await loadWasm();
  const overlay = $("overlay");
  const scene = new SceneView(document.body);
  const rig = new CameraRig(scene.renderer.domElement);
  const hud = new Hud(overlay);
  const plots = new TelemetryPlots(overlay);
  const audio = new AudioSystem(scene.world, overlay);

  // ----- session management --------------------------------------------------------------

  const rebuildViews = () => {
    for (const v of app.views.values()) v.dispose();
    app.views.clear();
    scene.clearContacts();
    plots.clear();
    syncViews();
    const ids = sim.entities();
    app.player = ids[0] ?? 1;
    app.focus = app.player;
    const sc = sim.scenario();
    scene.setBarriers(isReplay() ? [] : sc.barriers ?? []);
    ($("mode") as HTMLSelectElement).value = modeName();
    const first = app.views.get(app.player);
    if (first) ($("car") as HTMLSelectElement).value = first.info.car_id;
    if (app.scenarioFile) ($("sc-select") as HTMLSelectElement).value = app.scenarioFile;
    syncAssistBoxes();
    rig.setMode(rig.mode, app.views.get(app.focus)?.root);
  };

  const syncViews = () => {
    for (const id of sim.entities()) {
      if (app.views.has(id)) continue;
      const v = new VehicleView(id, COLOURS[(id - 1) % COLOURS.length]);
      scene.world.add(v.root);
      scene.world.add(v.velocity);
      app.views.set(id, v);
    }
  };

  const isReplay = () => sim.worldInfo()[W.replay] > 0.5;
  const modeName = () => (sim.worldInfo()[W.mode] > 0.5 ? "Simulation" : "Arcade");

  const startFreeDrive = (
    car = ($("car") as HTMLSelectElement).value || "debug_sedan",
    mode = ($("mode") as HTMLSelectElement).value,
  ) => {
    app.scenarioFile = null;
    app.scenario = null;
    app.result = null;
    app.bench = null;
    sim.init(1, JSON.stringify({ name: "free_drive", car, mode, seed: 1 }));
    rebuildViews();
    applyAssistPreset(app.source);
  };

  const runScenario = async (file: string, speedKmh?: number) => {
    const text = await (await fetch(`/scenarios/${file}`)).text();
    const sc = sim.scenarioFromRon(text);
    if (speedKmh && speedKmh > 0) {
      if (sc.cruise_kmh != null) sc.cruise_kmh = speedKmh;
      for (const o of sc.others) if (o.cruise_kmh != null) o.cruise_kmh = speedKmh;
    }
    app.scenarioFile = file;
    app.scenario = sc;
    app.result = null;
    app.bench = null;
    sim.init(0, JSON.stringify(sc));
    rebuildViews();
  };

  const applyAssistPreset = (source: InputSource) => {
    if (isReplay()) return;
    try {
      sim.setAssists(app.player, sim.assistPreset(source));
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
    const files: string[] = await (await fetch("/scenarios/index.json")).json();
    for (const f of files) scSel.add(new Option(f, f));
  } catch {
    scSel.add(new Option("(scenario folder not served)", ""));
  }
  $("sc-run").onclick = () => scSel.value && runScenario(scSel.value, Number(($("sc-speed") as HTMLInputElement).value));
  $("sc-free").onclick = () => startFreeDrive();
  ($("mode") as HTMLSelectElement).onchange = (e) => {
    sim.setDamageMode((e.target as HTMLSelectElement).value);
    rebuildViews();
    if (!app.scenario) applyAssistPreset(app.source);
  };
  document.querySelectorAll<HTMLInputElement>("#assist-row input").forEach((cb) => {
    cb.onchange = () => {
      if (isReplay()) return;
      const a = sim.assists(app.player) as unknown as Record<string, unknown>;
      a[cb.dataset.a!] = cb.checked;
      sim.setAssists(app.player, a as never);
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
    if (bytes) download(bytes, `wreckpursuit-${Date.now()}.wpreplay`, "application/octet-stream");
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
    if (st) sim.replaySeek(Math.max(st.start_tick, st.tick + ticks));
  };

  const startBench = (n: number) => {
    startFreeDrive("debug_sedan");
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
    if (n >= 1 && n <= CAMERA_MODES.length) return rig.setMode(CAMERA_MODES[n - 1], app.views.get(app.focus)?.root);
    switch (e.code) {
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
      case "F1":
      case "Slash":
        e.preventDefault();
        hud.toggleHelp();
        break;
    }
  });

  // ----- start ----------------------------------------------------------------------------

  const camParam = params.get("cam") as CameraMode | null;
  if (camParam && CAMERA_MODES.includes(camParam)) rig.mode = camParam;
  const scParam = params.get("scenario");
  if (scParam) {
    scSel.value = scParam;
    await runScenario(scParam, Number(params.get("speed") ?? 0));
  } else {
    startFreeDrive("debug_sedan");
  }
  if (params.get("bench")) startBench(Number(params.get("bench")));
  if (params.get("replay") === "last") $("rp-last").click();
  void scene.loadCity();

  // ----- frame loop -----------------------------------------------------------------------

  let last = performance.now();
  let fps = 60;
  let simMs = 0;
  let renderMs = 0;
  let frameNo = 0;

  function frame(now: number): void {
    const dt = Math.min((now - last) / 1000, 0.1);
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

    syncViews();
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
    scene.addEvents(events);
    scene.update(dt, sim.intents());
    const wi = sim.worldInfo();
    scene.setTimeOfDay(wi[W.night]);

    const focusView = app.views.get(app.focus) ?? null;
    if (focusTel) {
      plots.push(wi[W.tick] / 240, focusTel);
      if (layers.on("telemetry")) plots.draw();
    }
    plots.canvas.style.display = layers.on("telemetry") ? "" : "none";
    rig.update(focusView?.root ?? null, focusView?.info.body_size_m ?? [2, 4.5, 1.4], focusRec >= 0 ? cur[focusRec + S.speed] : 0, dt);
    audio.update(rig.camera, events, app.paused && !replay ? 0 : dt, app.focus);

    const r0 = performance.now();
    scene.render(rig.camera);
    const r1 = performance.now();
    renderMs += (r1 - r0 - renderMs) * 0.1;

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
        tick: wi[W.tick],
        hours: wi[W.hours],
        hash: sim.stateHash(),
        vehicles: app.views.size,
        replay,
        recording: wi[W.recording] > 0.5,
        energy: focusView ? sim.regionEnergy(app.focus) : null,
        regionsOfNodes: focusView ? regionsCache(app.focus) : null,
        rest: focusView ? restCache(app.focus) : null,
        camera: rig.mode,
      });
      updatePanels(replay);
    }
    if (app.bench) benchTick(t1 - t0, r1 - r0);
    requestAnimationFrame(frame);
  }

  const regions = new Map<number, Uint8Array>();
  const rests = new Map<number, Float32Array>();
  const regionsCache = (id: number) => regions.get(id) ?? regions.set(id, sim.latticeRegions(id)).get(id)!;
  const restCache = (id: number) => rests.get(id) ?? rests.set(id, sim.latticeRest(id)).get(id)!;

  const updatePanels = (replay: boolean) => {
    const st = sim.replayStatus();
    const status = $("rp-status");
    if (replay && st) {
      scrub.min = String(st.start_tick);
      scrub.max = String(st.end_tick);
      if (document.activeElement !== scrub) scrub.value = String(st.tick);
      const ok = st.drift_ticks.length === 0;
      const atEnd = st.tick >= st.end_tick;
      status.innerHTML =
        `replay "${st.scenario}" ${st.mode} · tick ${st.tick}/${st.end_tick} · ${((st.tick - st.start_tick) / 240).toFixed(2)} s · x${app.paused ? 0 : st.speed}` +
        `<br>hashes verified ${st.verified_hashes}/${st.stored_hashes} · ` +
        (ok ? `<span style="color:#2ecc40">no drift</span>` : `<span style="color:#ff4136">DRIFT at ticks ${st.drift_ticks.slice(0, 5).join(",")}</span>`) +
        (atEnd ? ` · final ${st.state_hash === st.recorded_final_hash ? "✓ matches" : "✗ differs"} ${st.state_hash}` : "");
    } else {
      status.textContent = sim.recording()
        ? `recording… (tick ${sim.worldInfo()[W.tick]})`
        : app.lastReplay
          ? `last replay ${app.lastReplay.length} bytes (stored)`
          : "no replay loaded";
    }
    const res = sim.scenarioResult();
    const out = $("sc-result");
    if (res && !replay) {
      app.result = res;
      out.innerHTML =
        `<b>${res.name}</b> ${res.done ? (res.pass ? '<span class="pass">PASS</span>' : '<span class="fail">FAIL</span>') : `running ${((res.end_tick - res.tick) / 240).toFixed(1)} s left`}` +
        res.checks
          .map(
            (c) =>
              `<div class="${c.pending ? "pending" : c.pass ? "pass" : "fail"}">${c.pass ? "✓" : "✗"} ${c.metric} = ${c.value?.toFixed(3) ?? "?"} in [${c.min}, ${c.max}]${c.pending ? " (pending physics)" : ""}</div>`,
          )
          .join("");
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
    ready: true,
    stopRecording,
    loadReplay,
    runScenario,
    startFreeDrive,
    startBench,
    setCamera: (m: CameraMode, offset?: [number, number, number]) => rig.setMode(m, app.views.get(app.focus)?.root, offset),
    toggleLayer: (id: string) => layers.toggle(id),
  };
  requestAnimationFrame(frame);
}

main().catch((e) => {
  $("error").textContent = `failed to start: ${e}\n\nDid you run \`npm run wasm\` (or \`just web\`)?`;
  console.error(e);
});
