/**
 * Captain map: a 2D canvas command view over `wp_police`'s captain model.
 *
 * - Layers: districts / buildings, roads, heat map, incidents, tools, suspects, units.
 * - Pan: drag empty space. Zoom: mouse wheel (around the cursor) or +/-.
 * - Select a unit by clicking it (or 1-9). Drag a unit onto a suspect to send it (Shift = intercept)
 *   or onto an incident to respond. Right-click opens the command radial.
 * - Every action is a `captain` UiCommand carrying `wp_police::CaptainCommand` (mirrored in
 *   `model/police.ts`). Tools target a racer, optionally on a road segment, exactly as E's API.
 */
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useSend, useSlice } from "../../model/GameModel";
import type { CaptainCommand, UnitOrder } from "../../model/police";
import type { CaptainState, CityMap, Incident, PoliceUnit, Suspect, Vec2 } from "../../model/types";
import { Badge, Button, Panel, useEscape } from "../../ui/kit";
import { mmss, words } from "../../ui/format";
import { targetPos } from "../../mock/police";
import "./captain.css";

type Layer = "zones" | "roads" | "heat" | "incidents" | "tools" | "suspects" | "units" | "labels";
const ALL_LAYERS: Layer[] = ["zones", "roads", "heat", "incidents", "tools", "suspects", "units", "labels"];

interface Camera {
  cx: number;
  cy: number;
  /** Pixels per metre. */
  z: number;
}

type Hit =
  | { kind: "unit"; unit: PoliceUnit }
  | { kind: "suspect"; suspect: Suspect }
  | { kind: "incident"; incident: Incident }
  | { kind: "ground"; pos: Vec2; road: number | null };

interface Radial {
  x: number;
  y: number;
  hit: Hit;
}

/** Theme colours read from CSS (canvas cannot use var()). */
function themeColors(el: Element | null): Record<string, string> {
  const cs = el ? getComputedStyle(el) : null;
  const v = (n: string, d: string) => cs?.getPropertyValue(n).trim() || d;
  return {
    scan: v("--scan", "#3dff8f"),
    warn: v("--warn", "#ffb21e"),
    danger: v("--danger", "#ff3d3d"),
    info: v("--info", "#5ab8ff"),
    ink: v("--ink", "#e6efe9"),
    ink3: v("--ink-3", "#6c7d73"),
    polA: v("--police-a", "#ff2e55"),
    polB: v("--police-b", "#3b7dff"),
  };
}

const ZONE_TINT: Record<string, string> = {
  Core: "rgba(90,184,255,0.07)",
  Commercial: "rgba(90,184,255,0.045)",
  Residential: "rgba(230,239,233,0.025)",
  Suburb: "rgba(230,239,233,0.015)",
  Industrial: "rgba(255,178,30,0.04)",
  Port: "rgba(255,178,30,0.06)",
  Building: "rgba(230,239,233,0.05)",
};

const ASSIGNMENT_TONE: Record<string, "danger" | "warn" | "info" | "dim"> = {
  pursue: "danger",
  intercept: "danger",
  "helo-track": "danger",
  respond: "warn",
  roadblock: "info",
  spikes: "info",
  "out-of-service": "dim",
};

/** Nearest road (id) to a world point, within `maxM`. */
function nearestRoad(map: CityMap, p: Vec2, maxM: number): number | null {
  let best: number | null = null;
  let bestD = maxM;
  for (const r of map.roads) {
    for (let i = 0; i + 1 < r.points.length; i++) {
      const [a, b] = [r.points[i], r.points[i + 1]];
      const abx = b[0] - a[0];
      const aby = b[1] - a[1];
      const t = Math.max(0, Math.min(1, ((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / Math.max(abx * abx + aby * aby, 1e-6)));
      const d = Math.hypot(a[0] + abx * t - p[0], a[1] + aby * t - p[1]);
      if (d < bestD) {
        bestD = d;
        best = r.id;
      }
    }
  }
  return best;
}

/** Captain map screen. */
export function CaptainMap() {
  const send = useSend();
  const captain = useSlice((s) => s.captain);
  const map = useSlice((s) => s.map);
  const tick = useSlice((s) => s.replay.tick);
  const tps = useSlice((s) => s.replay.ticksPerSecond);
  const nowS = tick / tps;
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [cam, setCam] = useState<Camera>(() => ({ cx: 0, cy: 0, z: 0.5 }));
  const [layers, setLayers] = useState<Layer[]>(ALL_LAYERS);
  const [selected, setSelected] = useState<number | null>(null);
  const [drag, setDrag] = useState<{ unit: number; to: [number, number] } | null>(null);
  const [radial, setRadial] = useState<Radial | null>(null);
  const [follow, setFollow] = useState(false);
  const panRef = useRef<{ x: number; y: number; cam: Camera } | null>(null);
  const fittedFor = useRef<string | null>(null);

  useEscape(() => {
    if (radial) setRadial(null);
    else if (selected !== null) setSelected(null);
    else send({ type: "back" });
  });

  // Size the canvas to its container (HiDPI aware).
  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      setSize({ w: Math.max(200, r.width), h: Math.max(200, r.height) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Fit the road network once per map (the zone polygons can extend well past it).
  useEffect(() => {
    if (fittedFor.current === map.id || size.w <= 200) return;
    fittedFor.current = map.id;
    const pts = map.roads.flatMap((r) => r.points);
    const xs = pts.map((p) => p[0]);
    const ys = pts.map((p) => p[1]);
    const [x0, x1, y0, y1] = pts.length ? [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)] : [-500, 500, -500, 500];
    setCam({ cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, z: Math.min(size.w / (x1 - x0), size.h / (y1 - y0)) * 0.92 });
  }, [size.w, size.h, map]);

  const top = topSuspect(captain);

  // Follow the priority (or most wanted) suspect.
  useEffect(() => {
    if (follow && top) setCam((c) => ({ ...c, cx: top.pos[0], cy: top.pos[1] }));
  }, [follow, top?.pos[0], top?.pos[1]]);

  const toScreen = (p: Vec2): [number, number] => [(p[0] - cam.cx) * cam.z + size.w / 2, -(p[1] - cam.cy) * cam.z + size.h / 2];
  const toWorld = (x: number, y: number): Vec2 => [(x - size.w / 2) / cam.z + cam.cx, -(y - size.h / 2) / cam.z + cam.cy];

  const hitTest = (x: number, y: number): Hit => {
    const r2 = 14 * 14;
    const near = (p: Vec2) => {
      const [sx, sy] = toScreen(p);
      return (sx - x) ** 2 + (sy - y) ** 2 <= r2;
    };
    if (layers.includes("units")) for (const u of captain.units) if (near(u.pos)) return { kind: "unit", unit: u };
    if (layers.includes("suspects")) for (const s of captain.suspects) if (near(s.pos)) return { kind: "suspect", suspect: s };
    if (layers.includes("incidents")) for (const i of [...captain.incidents].reverse()) if (near(i.pos)) return { kind: "incident", incident: i };
    const pos = toWorld(x, y);
    return { kind: "ground", pos, road: nearestRoad(map, pos, 20 / cam.z + 10) };
  };

  const roadPath = useMemo(() => buildRoadPath(map), [map]);
  const heatCanvas = useHeatCanvas(captain.heat, map, layers.includes("heat"));

  // Draw.
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    if (c.width !== Math.round(size.w * dpr)) c.width = Math.round(size.w * dpr);
    if (c.height !== Math.round(size.h * dpr)) c.height = Math.round(size.h * dpr);
    const ctx = c.getContext("2d");
    if (!ctx) return;
    draw(ctx, dpr, size, cam, map, captain, roadPath, heatCanvas, layers, selected, drag, themeColors(wrap.current), nowS);
  });

  const order = (unit: number, o: UnitOrder) => send({ type: "captain", command: { AssignUnit: { unit, order: o } } });

  const onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    setRadial(null);
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const hit = hitTest(x, y);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    if (hit.kind === "unit") {
      setSelected(hit.unit.id);
      setDrag({ unit: hit.unit.id, to: [x, y] });
    } else {
      panRef.current = { x, y, cam };
      if (hit.kind === "ground") setSelected(null);
    }
  };
  const onPointerMove = (e: PointerEvent) => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    if (drag) setDrag({ ...drag, to: [x, y] });
    else if (panRef.current) {
      const p = panRef.current;
      setFollow(false);
      setCam({ ...p.cam, cx: p.cam.cx - (x - p.x) / p.cam.z, cy: p.cam.cy + (y - p.y) / p.cam.z });
    }
  };
  const onPointerUp = (e: PointerEvent) => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    if (drag) {
      const unit = captain.units.find((u) => u.id === drag.unit);
      const [ux, uy] = unit ? toScreen(unit.pos) : [x, y];
      if (unit && Math.hypot(x - ux, y - uy) > 18) {
        const hit = hitTest(x, y);
        if (hit.kind === "suspect") order(unit.id, e.shiftKey ? { Intercept: { target: hit.suspect.id } } : { Pursue: { target: hit.suspect.id } });
        else if (hit.kind === "incident") order(unit.id, { Respond: { incident: hit.incident.id } });
        else setRadial({ x, y, hit });
      }
      setDrag(null);
    }
    panRef.current = null;
  };
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const before = toWorld(x, y);
    const z = Math.min(8, Math.max(0.15, cam.z * Math.exp(-e.deltaY * 0.0015)));
    // Keep the point under the cursor fixed.
    setCam({ z, cx: before[0] - (x - size.w / 2) / z, cy: before[1] + (y - size.h / 2) / z });
  };
  const onContext = (e: MouseEvent) => {
    e.preventDefault();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    setRadial({ x, y, hit: hitTest(x, y) });
  };

  // Keyboard: 1-9 select unit, D disengage, P patrol, +/- zoom.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === "INPUT") return;
      const n = Number(e.key);
      if (n >= 1 && n <= 9 && captain.units[n - 1]) setSelected(captain.units[n - 1].id);
      else if ((e.key === "d" || e.key === "D") && selected !== null) order(selected, "Disengage");
      else if ((e.key === "p" || e.key === "P") && selected !== null) order(selected, "Patrol");
      else if (e.key === "+" || e.key === "=") setCam((c) => ({ ...c, z: Math.min(8, c.z * 1.25) }));
      else if (e.key === "-") setCam((c) => ({ ...c, z: Math.max(0.15, c.z / 1.25) }));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [captain.units, selected]);

  const sel = captain.units.find((u) => u.id === selected) ?? null;
  const rd = captain.readiness;

  return (
    <div class="wp-screen wp-captain">
      <div class="wp-captain-top">
        <div>
          <div class="wp-mono wp-captain-kicker">
            // METRO PD · CAPTAIN'S DESK · {mmss(nowS)} · {captain.source === "wp_police" ? "RECORDED wp_police" : "MOCK POLICE"}
          </div>
          <h1 class="wp-captain-title">Command map</h1>
        </div>
        <div class="wp-row">
          {ALL_LAYERS.map((l) => (
            <Button key={l} size="sm" pressed={layers.includes(l)} onClick={() => setLayers((ls) => (ls.includes(l) ? ls.filter((x) => x !== l) : [...ls, l]))}>
              {l}
            </Button>
          ))}
          <Button size="sm" pressed={follow} onClick={() => setFollow((f) => !f)}>
            Follow
          </Button>
          <Button size="sm" onClick={() => send({ type: "back" })} hotkey="Esc">
            Back
          </Button>
        </div>
      </div>
      <div class="wp-captain-body">
        <div
          class="wp-captain-canvas"
          ref={wrap}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onWheel={onWheel}
          onContextMenu={onContext}
          data-dragging={drag ? "true" : undefined}
        >
          <canvas ref={canvas} style={{ width: `${size.w}px`, height: `${size.h}px` }} aria-label="Captain map: drag units to assign, right-click for commands" role="img" />
          {radial && <RadialMenu radial={radial} captain={captain} selected={sel} onClose={() => setRadial(null)} />}
          <div class="wp-captain-hint wp-mono">drag unit → suspect (shift: intercept) / incident · right-click: commands · wheel: zoom · 1-9 select · D disengage · P patrol</div>
        </div>
        <aside class="wp-captain-side">
          <Panel title="Units" tag={`${captain.units.filter((u) => u.assignment !== "out-of-service").length}/${captain.units.length}`}>
            <ul class="wp-unit-list">
              {captain.units.map((u, i) => (
                <li key={u.id}>
                  <button type="button" class="wp-unit-row" aria-pressed={u.id === selected} onClick={() => setSelected(u.id)} data-off={u.assignment === "out-of-service"}>
                    <span class="wp-mono wp-dim">{i + 1}</span>
                    <span>
                      <b>{u.callsign}</b> {u.human && <Badge tone="info">Player</Badge>}
                      <div class="wp-mono wp-dim" style={{ fontSize: "0.75em" }}>
                        {u.kind === "Helicopter" ? "air" : "car"} · {describeUnit(u, captain)}
                        {u.pendingOrder && <span class="wp-warn"> · sent: {describeOrder(u.pendingOrder)}</span>}
                      </div>
                    </span>
                    <Badge tone={ASSIGNMENT_TONE[u.assignment] ?? "dim"}>{u.assignment}</Badge>
                  </button>
                </li>
              ))}
            </ul>
            {sel && (
              <div class="wp-row" style={{ marginTop: "0.5em" }}>
                <Button size="sm" onClick={() => order(sel.id, "Patrol")}>
                  Patrol
                </Button>
                <Button size="sm" onClick={() => order(sel.id, "Disengage")}>
                  Disengage
                </Button>
                {top && (
                  <Button size="sm" variant="danger" onClick={() => order(sel.id, { Pursue: { target: top.id } })}>
                    Pursue {top.name}
                  </Button>
                )}
              </div>
            )}
          </Panel>
          <Panel title="Suspects" tag={`${captain.suspects.length}`}>
            {captain.suspects.length === 0 && <p class="wp-dim" style={{ margin: 0 }}>No racers on the street.</p>}
            {captain.suspects.map((s) => (
              <div class="wp-suspect-row" key={s.id}>
                <div>
                  <b>{s.name}</b> <span class="wp-mono wp-dim">#{s.id}</span>
                  <div class="wp-mono wp-dim" style={{ fontSize: "0.75em" }}>
                    {words(s.status).toUpperCase()} · {s.stalenessS === null ? "never seen" : s.stalenessS < 0.5 ? <span class="wp-danger">VISUAL</span> : `seen ${s.stalenessS.toFixed(0)} s ago`} · {s.unitsAssigned} units
                    {s.bounty > 0 && ` · bounty ${Math.round(s.bounty)}`}
                  </div>
                </div>
                <Badge tone={s.level === "None" ? "dim" : "danger"}>{s.level === "None" ? "—" : s.level}</Badge>
                <Button size="sm" pressed={captain.priorityTarget === s.id} onClick={() => send({ type: "captain", command: { SetPriorityTarget: { target: captain.priorityTarget === s.id ? null : s.id } } })} title="Priority target">
                  ★
                </Button>
              </div>
            ))}
          </Panel>
          <Panel title="Tools">
            <div class="wp-tool-grid wp-mono">
              <span>Roadblock</span>
              <span>
                {rd.roadblocks[0]}/{rd.roadblocks[1]}
              </span>
              <span class={rd.roadblock_s > 0 ? "wp-warn" : "wp-ok"}>{rd.roadblock_s > 0 ? `${rd.roadblock_s.toFixed(0)} s` : "ready"}</span>
              <span>Spikes</span>
              <span>
                {rd.spikes[0]}/{rd.spikes[1]}
              </span>
              <span class={rd.spikes_s > 0 ? "wp-warn" : "wp-ok"}>{rd.spikes_s > 0 ? `${rd.spikes_s.toFixed(0)} s` : "ready"}</span>
              <span>Air unit</span>
              <span>—</span>
              <span class={rd.helo_s > 0 ? "wp-warn" : "wp-ok"}>{rd.helo_s > 0 ? `${rd.helo_s.toFixed(0)} s` : "ready"}</span>
            </div>
            {captain.heatByDistrict.length > 0 && (
              <div class="wp-row wp-mono" style={{ marginTop: "0.6em", fontSize: "0.8em" }}>
                <span class="wp-dim">HEAT</span>
                {captain.heatByDistrict.map(([d, h]) => (
                  <Badge key={d} tone={h > 50 ? "danger" : h > 10 ? "warn" : "dim"}>
                    {d} {h.toFixed(0)}
                  </Badge>
                ))}
              </div>
            )}
          </Panel>
          <Panel title="Incidents" tag={`${captain.incidents.length}`}>
            <ul class="wp-feed" style={{ maxHeight: "10em" }}>
              {[...captain.incidents].reverse().slice(0, 30).map((i) => (
                <li key={i.id} style={{ gridTemplateColumns: "3.4em 1fr" }}>
                  <span class="wp-dim">{mmss(i.tS)}</span>
                  <span>
                    {words(i.kind)} {i.suspect !== null && <span class="wp-dim">· #{i.suspect}</span>} {i.unit !== null && <span class="wp-dim">· unit {i.unit}</span>}
                  </span>
                </li>
              ))}
            </ul>
          </Panel>
        </aside>
      </div>
    </div>
  );
}

/** The suspect commands default to: the priority target, else the most wanted. */
function topSuspect(c: CaptainState): Suspect | undefined {
  const p = c.suspects.find((s) => s.id === c.priorityTarget);
  if (p) return p;
  return [...c.suspects].sort((a, b) => b.heat - a.heat || a.level.localeCompare(b.level))[0];
}

function describeOrder(o: UnitOrder): string {
  if (typeof o === "string") return o.toLowerCase();
  if ("Pursue" in o) return `pursue #${o.Pursue.target}`;
  if ("Intercept" in o) return `intercept #${o.Intercept.target}`;
  return `respond #${o.Respond.incident}`;
}

function describeUnit(u: PoliceUnit, c: CaptainState): string {
  const racer = (id: number | null) => c.suspects.find((s) => s.id === id)?.name ?? `#${id}`;
  switch (u.assignment) {
    case "pursue":
      return `pursuing ${racer(u.target)}${u.tactic ? ` (${u.tactic})` : ""}`;
    case "intercept":
      return `intercepting ${racer(u.target)}`;
    case "helo-track":
      return `tracking ${racer(u.target)}`;
    case "respond":
      return u.incident !== null ? `responding to #${u.incident}` : "responding";
    case "roadblock":
      return "manning a roadblock";
    case "spikes":
      return "laying spikes";
    case "out-of-service":
      return "out of service";
    case "idle":
      return "available";
    default:
      return "patrolling";
  }
}

// ------------------------------------------------------------------ radial menu

function RadialMenu(props: { radial: Radial; captain: CaptainState; selected: PoliceUnit | null; onClose: () => void }) {
  const send = useSend();
  const { x, y, hit } = props.radial;
  const c = props.captain;
  const cmd = (command: CaptainCommand) => {
    send({ type: "captain", command });
    props.onClose();
  };
  const top = topSuspect(c);
  const pos = hit.kind === "ground" ? hit.pos : hit.kind === "unit" ? hit.unit.pos : hit.kind === "suspect" ? hit.suspect.pos : hit.incident.pos;
  const nearestCar = [...c.units]
    .filter((u) => u.kind === "Car" && !["out-of-service", "roadblock", "spikes"].includes(u.assignment))
    .sort((a, b) => Math.hypot(a.pos[0] - pos[0], a.pos[1] - pos[1]) - Math.hypot(b.pos[0] - pos[0], b.pos[1] - pos[1]))[0];
  const sendUnit = props.selected && props.selected.kind === "Car" ? props.selected : nearestCar;
  const rd = c.readiness;
  const items: { label: string; disabled?: boolean; run: () => void }[] = [];
  if (hit.kind === "unit") {
    const u = hit.unit;
    items.push({ label: "Patrol", run: () => cmd({ AssignUnit: { unit: u.id, order: "Patrol" } }) });
    items.push({ label: "Disengage", run: () => cmd({ AssignUnit: { unit: u.id, order: "Disengage" } }) });
    if (top) {
      items.push({ label: `Pursue ${top.name}`, run: () => cmd({ AssignUnit: { unit: u.id, order: { Pursue: { target: top.id } } } }) });
      items.push({ label: `Intercept ${top.name}`, run: () => cmd({ AssignUnit: { unit: u.id, order: { Intercept: { target: top.id } } } }) });
    }
  } else if (hit.kind === "suspect") {
    const s = hit.suspect;
    items.push({ label: c.priorityTarget === s.id ? "Clear priority" : "Priority target", run: () => cmd({ SetPriorityTarget: { target: c.priorityTarget === s.id ? null : s.id } }) });
    if (sendUnit) items.push({ label: `Send ${sendUnit.callsign}`, run: () => cmd({ AssignUnit: { unit: sendUnit.id, order: { Pursue: { target: s.id } } } }) });
    items.push({ label: `Roadblock ahead${rd.roadblock_s > 0 ? ` (${rd.roadblock_s.toFixed(0)}s)` : ""}`, run: () => cmd({ RequestRoadblock: { target: s.id, segment: null } }) });
    items.push({ label: `Spikes ahead${rd.spikes_s > 0 ? ` (${rd.spikes_s.toFixed(0)}s)` : ""}`, run: () => cmd({ RequestSpikes: { target: s.id, segment: null } }) });
    items.push({ label: "Air unit", run: () => cmd({ RequestHelicopter: { target: s.id } }) });
    items.push({ label: "Call off", run: () => cmd({ CallOff: { target: s.id } }) });
  } else if (hit.kind === "incident") {
    const inc = hit.incident;
    if (sendUnit) items.push({ label: `Send ${sendUnit.callsign}`, run: () => cmd({ AssignUnit: { unit: sendUnit.id, order: { Respond: { incident: inc.id } } } }) });
  } else if (top) {
    // Ground: tools on this road segment, aimed at the priority / most wanted racer.
    const seg = hit.road;
    items.push({ label: `Roadblock here (${top.name})`, disabled: seg === null, run: () => cmd({ RequestRoadblock: { target: top.id, segment: seg } }) });
    items.push({ label: `Spikes here (${top.name})`, disabled: seg === null, run: () => cmd({ RequestSpikes: { target: top.id, segment: seg } }) });
  }
  items.push({ label: "Cancel", run: props.onClose });
  // Items sit on an ellipse (labels are wide, so spread them more horizontally).
  const R = 70 + items.length * 6;
  return (
    <div class="wp-radial" style={{ left: `${x}px`, top: `${y}px` }} role="menu" onPointerDown={(e) => e.stopPropagation()} onContextMenu={(e) => e.preventDefault()}>
      <div class="wp-radial-hub wp-mono">
        {hit.kind === "unit" ? hit.unit.callsign : hit.kind === "suspect" ? `#${hit.suspect.id}` : hit.kind === "incident" ? words(hit.incident.kind) : hit.road !== null ? `SEG ${hit.road}` : "NO ROAD"}
      </div>
      {items.map((it, i) => {
        const a = (i / items.length) * Math.PI * 2 - Math.PI / 2;
        return (
          <button
            type="button"
            role="menuitem"
            key={it.label}
            class="wp-radial-item"
            disabled={it.disabled}
            autofocus={i === 0}
            style={{ left: `${Math.cos(a) * R * 1.55}px`, top: `${Math.sin(a) * R}px` }}
            onClick={it.run}
          >
            {it.label}
          </button>
        );
      })}
    </div>
  );
}

// ------------------------------------------------------------------ drawing

function buildRoadPath(map: CityMap): Path2D | null {
  if (typeof Path2D === "undefined") return null;
  const p = new Path2D();
  for (const r of map.roads) {
    r.points.forEach(([x, y], i) => (i ? p.lineTo(x, y) : p.moveTo(x, y)));
  }
  return p;
}

/** Low-res heat texture over the map bounds (gaussian splats), rebuilt when sources change. */
function useHeatCanvas(heat: { pos: Vec2; weight: number }[], map: CityMap, enabled: boolean): HTMLCanvasElement | null {
  const ref = useRef<HTMLCanvasElement | null>(null);
  const last = useRef(0);
  const key = enabled ? heat.map((h) => `${h.pos[0].toFixed(0)},${h.pos[1].toFixed(0)},${h.weight.toFixed(2)}`).join(";") : "";
  return useMemo(() => {
    if (!enabled || typeof document === "undefined") return null;
    // Throttle rebuilds to 4 Hz: the heat map is a slow-moving layer.
    const now = performance.now();
    if (ref.current && now - last.current < 250) return ref.current;
    last.current = now;
    const N = 96;
    const c = ref.current ?? document.createElement("canvas");
    c.width = N;
    c.height = N;
    const ctx = c.getContext("2d");
    if (!ctx) return null;
    const img = ctx.createImageData(N, N);
    const [x0, y0] = map.boundsMin;
    const [x1, y1] = map.boundsMax;
    const sigma = Math.max(x1 - x0, y1 - y0) / 13;
    for (let j = 0; j < N; j++) {
      for (let i = 0; i < N; i++) {
        const wx = x0 + ((i + 0.5) / N) * (x1 - x0);
        const wy = y1 - ((j + 0.5) / N) * (y1 - y0);
        let v = 0;
        for (const h of heat) {
          const d2 = (wx - h.pos[0]) ** 2 + (wy - h.pos[1]) ** 2;
          v += h.weight * Math.exp(-d2 / (2 * sigma * sigma));
        }
        v = Math.min(1, v);
        const o = (j * N + i) * 4;
        // amber → red ramp, alpha follows intensity
        img.data[o] = 255;
        img.data[o + 1] = Math.round(180 * (1 - v));
        img.data[o + 2] = 30;
        img.data[o + 3] = Math.round(170 * v);
      }
    }
    ctx.putImageData(img, 0, 0);
    ref.current = c;
    return c;
  }, [key, enabled, map]);
}

function draw(
  ctx: CanvasRenderingContext2D,
  dpr: number,
  size: { w: number; h: number },
  cam: Camera,
  map: CityMap,
  cap: CaptainState,
  roads: Path2D | null,
  heat: HTMLCanvasElement | null,
  layers: Layer[],
  selected: number | null,
  drag: { unit: number; to: [number, number] } | null,
  col: Record<string, string>,
  nowS: number,
): void {
  const on = (l: Layer) => layers.includes(l);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = "#040605";
  ctx.fillRect(0, 0, size.w, size.h);
  // World transform: metres → pixels, +Y up.
  const world = () => ctx.setTransform(dpr * cam.z, 0, 0, -dpr * cam.z, dpr * (size.w / 2 - cam.cx * cam.z), dpr * (size.h / 2 + cam.cy * cam.z));
  const screenOf = (p: Vec2): [number, number] => [(p[0] - cam.cx) * cam.z + size.w / 2, -(p[1] - cam.cy) * cam.z + size.h / 2];
  const px = 1 / cam.z; // one pixel in metres

  world();
  if (on("zones")) {
    for (const z of map.zones) {
      ctx.beginPath();
      z.polygon.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      ctx.closePath();
      ctx.fillStyle = ZONE_TINT[z.kind] ?? "rgba(255,255,255,0.02)";
      ctx.fill();
    }
  }
  if (on("heat") && heat) {
    const [x0, y0] = map.boundsMin;
    const [x1, y1] = map.boundsMax;
    ctx.save();
    // Draw the texture right side up: flip back for the image.
    ctx.translate(x0, y1);
    ctx.scale(1, -1);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(heat, 0, 0, x1 - x0, y1 - y0);
    ctx.restore();
  }
  if (on("roads") && roads) {
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = "rgba(230,239,233,0.08)";
    ctx.lineWidth = Math.max(9, 6 * px);
    ctx.stroke(roads);
    ctx.strokeStyle = "rgba(230,239,233,0.32)";
    ctx.lineWidth = Math.max(1.2 * px, 1.5);
    ctx.stroke(roads);
  }
  // POIs: stations / safehouses / cameras as small squares.
  for (const p of map.pois) {
    const c = p.kind === "PoliceStation" ? col.polB : p.kind === "Safehouse" || p.kind === "SafehouseCandidate" ? col.scan : p.kind === "Camera" ? col.warn : null;
    if (!c) continue;
    ctx.fillStyle = c;
    ctx.fillRect(p.pos[0] - 5 * px, p.pos[1] - 5 * px, 10 * px, 10 * px);
  }

  // Screen-space overlays (constant pixel size).
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.font = "11px ui-monospace, Menlo, monospace";
  ctx.textBaseline = "middle";

  if (on("tools")) {
    for (const t of cap.tools) {
      const [x, y] = screenOf(t.pos);
      ctx.strokeStyle = col.info;
      ctx.fillStyle = col.info;
      ctx.lineWidth = 2;
      if (t.tool === "SpikeStrip") {
        ctx.beginPath();
        for (let i = -3; i <= 3; i++) ctx.lineTo(x + i * 4, y + (i % 2 ? -4 : 4));
        ctx.stroke();
      } else {
        ctx.fillRect(x - 12, y - 3, 24, 6);
      }
      if (on("labels")) ctx.fillText(`${t.tool === "SpikeStrip" ? "Spikes" : "Roadblock"}${t.remainingS !== null ? ` ${t.remainingS.toFixed(0)}s` : ""}`, x + 14, y);
    }
  }

  if (on("incidents")) {
    for (const i of cap.incidents) {
      const age = nowS - i.tS;
      if (age < 0 || age > 90) continue;
      const [x, y] = screenOf(i.pos);
      ctx.globalAlpha = Math.max(0.25, 1 - age / 90);
      ctx.fillStyle = i.kind === "Crash" ? col.danger : i.kind === "Report" || i.kind === "Sighting" ? col.polA : col.warn;
      ctx.beginPath();
      ctx.moveTo(x, y - 7);
      ctx.lineTo(x + 6, y + 5);
      ctx.lineTo(x - 6, y + 5);
      ctx.closePath();
      ctx.fill();
      ctx.globalAlpha = 1;
    }
  }

  // Assignment lines (solid = current, dashed = order sent, not yet reflected).
  if (on("units")) {
    ctx.lineWidth = 1.2;
    for (const u of cap.units) {
      const t = targetPos(u, cap.suspects, cap.incidents);
      if (!t) continue;
      const [ax, ay] = screenOf(u.pos);
      const [bx, by] = screenOf(t);
      ctx.setLineDash(u.pendingOrder ? [4, 4] : []);
      ctx.strokeStyle = u.id === selected ? col.ink : "rgba(59,125,255,0.55)";
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
      ctx.stroke();
    }
    ctx.setLineDash([]);
  }

  if (on("suspects")) {
    for (const s of cap.suspects) {
      const [x, y] = screenOf(s.pos);
      const lvl = s.level === "None" ? 0 : Number(s.level.slice(1));
      const c = lvl >= 3 ? col.danger : lvl > 0 ? col.warn : col.ink3;
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(s.headingRad);
      ctx.fillStyle = c;
      ctx.beginPath();
      ctx.moveTo(0, -9);
      ctx.lineTo(6, 0);
      ctx.lineTo(0, 9);
      ctx.lineTo(-6, 0);
      ctx.closePath();
      ctx.fill();
      ctx.restore();
      // A stale sighting shows as a dashed "last known" ring.
      if ((s.stalenessS ?? 0) > 1) {
        ctx.strokeStyle = c;
        ctx.setLineDash([3, 3]);
        ctx.beginPath();
        ctx.arc(x, y, 18, 0, Math.PI * 2);
        ctx.stroke();
        ctx.setLineDash([]);
      }
      if (s.id === cap.priorityTarget) {
        ctx.strokeStyle = col.ink;
        ctx.beginPath();
        ctx.arc(x, y, 13, 0, Math.PI * 2);
        ctx.stroke();
      }
      if (on("labels")) {
        ctx.fillStyle = col.ink;
        ctx.fillText(`${s.name}${lvl ? ` L${lvl}` : ""}${s.status === "Searching" ? " (searching)" : ""}`, x + 12, y - 10);
      }
    }
  }

  if (on("units")) {
    for (const u of cap.units) {
      const [x, y] = screenOf(u.pos);
      const isSel = u.id === selected;
      const off = u.assignment === "out-of-service";
      ctx.save();
      ctx.globalAlpha = off ? 0.35 : 1;
      ctx.translate(x, y);
      if (u.kind === "Helicopter") {
        ctx.strokeStyle = col.polA;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.arc(0, 0, 7, 0, Math.PI * 2);
        ctx.moveTo(-11, -11);
        ctx.lineTo(11, 11);
        ctx.moveTo(11, -11);
        ctx.lineTo(-11, 11);
        ctx.stroke();
      } else {
        ctx.rotate(u.headingRad);
        ctx.fillStyle = u.assignment === "pursue" || u.assignment === "intercept" ? col.polA : col.polB;
        ctx.fillRect(-4.5, -8, 9, 16);
        ctx.fillStyle = "#fff";
        ctx.fillRect(-4.5, -3, 9, 2);
      }
      ctx.restore();
      if (isSel) {
        ctx.strokeStyle = col.ink;
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.arc(x, y, 15, 0, Math.PI * 2);
        ctx.stroke();
      }
      if (u.human) {
        ctx.fillStyle = col.info;
        ctx.fillRect(x - 2, y - 16, 4, 4);
      }
      if (on("labels") && !off) {
        ctx.fillStyle = isSel ? col.ink : "rgba(230,239,233,0.75)";
        ctx.fillText(u.callsign, x + 12, y + 8);
      }
    }
  }

  // Drag preview.
  if (drag) {
    const u = cap.units.find((x) => x.id === drag.unit);
    if (u) {
      const [ax, ay] = screenOf(u.pos);
      ctx.strokeStyle = col.scan;
      ctx.lineWidth = 2;
      ctx.setLineDash([6, 4]);
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(drag.to[0], drag.to[1]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.beginPath();
      ctx.arc(drag.to[0], drag.to[1], 10, 0, Math.PI * 2);
      ctx.stroke();
    }
  }

  // Scale bar.
  const meters = [50, 100, 200, 500, 1000].find((m) => m * cam.z > 80) ?? 1000;
  ctx.fillStyle = col.ink3;
  ctx.fillRect(16, size.h - 22, meters * cam.z, 2);
  ctx.fillText(`${meters} m`, 16, size.h - 32);
}
