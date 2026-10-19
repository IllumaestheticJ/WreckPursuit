/**
 * Captain map: a 2D canvas command view.
 *
 * - Layers: districts, roads, heat map, incidents, tools, suspects, units (toggle each).
 * - Pan: drag empty space. Zoom: mouse wheel (around the cursor) or +/-.
 * - Select a unit by clicking it (or 1-9). Drag a unit onto a suspect / incident / spot to assign
 *   it (Pursue / Respond / Hold). Right-click opens the command radial (tools, recall, priority).
 * - Every action is a `captain` UiCommand carrying the proposed typed `CaptainCommand` (UI-2).
 */
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useSend, useSlice } from "../../model/GameModel";
import type { CaptainCommand, PoliceTool } from "../../model/proposed";
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

type Hit = { kind: "unit"; unit: PoliceUnit } | { kind: "suspect"; suspect: Suspect } | { kind: "incident"; incident: Incident } | { kind: "ground"; pos: Vec2 };

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
};

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
  const fitted = useRef(false);

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

  // Fit the city once we know the size.
  useEffect(() => {
    if (fitted.current || size.w <= 200) return;
    fitted.current = true;
    // Fit the road network (the zone polygons extend well past it).
    const pts = map.roads.flatMap((r) => r.points);
    const xs = pts.map((p) => p[0]);
    const ys = pts.map((p) => p[1]);
    const [x0, x1, y0, y1] = pts.length ? [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)] : [-500, 500, -500, 500];
    setCam({ cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, z: Math.min(size.w / (x1 - x0), size.h / (y1 - y0)) * 0.92 });
  }, [size.w, size.h, map]);

  // Follow the highest-priority visible suspect.
  useEffect(() => {
    if (!follow) return;
    const s = [...captain.suspects].sort((a, b) => b.priority - a.priority)[0];
    if (s) setCam((c) => ({ ...c, cx: s.pos[0], cy: s.pos[1] }));
  }, [follow, captain.suspects]);

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
    return { kind: "ground", pos: toWorld(x, y) };
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
    const col = themeColors(wrap.current);
    draw(ctx, dpr, size, cam, map, captain, roadPath, heatCanvas, layers, selected, drag, col, nowS);
  });

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
      const moved = Math.hypot(x - ux, y - uy) > 18;
      if (moved && unit) {
        const hit = hitTest(x, y);
        const cmd: CaptainCommand =
          hit.kind === "suspect"
            ? { Assign: { unit: unit.id, target: { Pursue: { racer: hit.suspect.id } } } }
            : hit.kind === "incident"
              ? { Assign: { unit: unit.id, target: { Incident: { id: hit.incident.id } } } }
              : { Assign: { unit: unit.id, target: { Hold: { pos: toWorld(x, y) } } } };
        send({ type: "captain", command: cmd });
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

  // Keyboard: 1-9 select unit, R recall, +/- zoom.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === "INPUT") return;
      const n = Number(e.key);
      if (n >= 1 && n <= 9 && captain.units[n - 1]) setSelected(captain.units[n - 1].id);
      else if ((e.key === "r" || e.key === "R") && selected !== null) send({ type: "captain", command: { Recall: { unit: selected } } });
      else if (e.key === "+" || e.key === "=") setCam((c) => ({ ...c, z: Math.min(8, c.z * 1.25) }));
      else if (e.key === "-") setCam((c) => ({ ...c, z: Math.max(0.15, c.z / 1.25) }));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [captain.units, selected]);

  const sel = captain.units.find((u) => u.id === selected) ?? null;

  return (
    <div class="wp-screen wp-captain">
      <div class="wp-captain-top">
        <div>
          <div class="wp-mono wp-captain-kicker">// METRO PD · CAPTAIN'S DESK · {mmss(nowS)}</div>
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
          {radial && <RadialMenu radial={radial} captain={captain} selected={sel} onClose={() => setRadial(null)} toWorld={toWorld} />}
          <div class="wp-captain-hint wp-mono">drag unit → target · right-click: commands · wheel: zoom · 1-9 select · R recall</div>
        </div>
        <aside class="wp-captain-side">
          <Panel title="Units" tag={`${captain.units.length}`}>
            <ul class="wp-unit-list">
              {captain.units.map((u, i) => (
                <li key={u.id}>
                  <button type="button" class="wp-unit-row" aria-pressed={u.id === selected} onClick={() => setSelected(u.id)}>
                    <span class="wp-mono wp-dim">{i + 1}</span>
                    <span>
                      <b>{u.callsign}</b> {u.human && <Badge tone="info">Player</Badge>}
                      <div class="wp-mono wp-dim" style={{ fontSize: "0.75em" }}>
                        {u.kind} · {describeTarget(u, captain)}
                      </div>
                    </span>
                    <Badge tone={u.status === "Pursuit" ? "danger" : u.status === "Responding" ? "warn" : u.status === "Roadblock" ? "info" : "dim"}>{u.status}</Badge>
                  </button>
                </li>
              ))}
            </ul>
            {sel && (
              <div class="wp-row" style={{ marginTop: "0.5em" }}>
                <Button size="sm" onClick={() => send({ type: "captain", command: { Recall: { unit: sel.id } } })}>
                  Recall
                </Button>
                {captain.suspects[0] && (
                  <Button size="sm" variant="danger" onClick={() => send({ type: "captain", command: { Assign: { unit: sel.id, target: { Pursue: { racer: topSuspect(captain)!.id } } } } })}>
                    Pursue top suspect
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
                    {Math.round(s.speedMps * 3.6)} km/h · {s.visible ? <span class="wp-danger">VISUAL</span> : "last known"}
                  </div>
                </div>
                <Badge tone={s.level === "None" ? "dim" : "danger"}>{s.level === "None" ? "—" : s.level}</Badge>
                <span class="wp-row" style={{ gap: "0.2em" }}>
                  {[1, 2, 3].map((p) => (
                    <Button key={p} size="sm" pressed={s.priority === p} onClick={() => send({ type: "captain", command: { SetPriority: { racer: s.id, priority: p } } })} title={`Priority ${p}`}>
                      P{p}
                    </Button>
                  ))}
                </span>
              </div>
            ))}
          </Panel>
          <Panel title="Tools">
            <div class="wp-row">
              {(Object.keys(captain.charges) as PoliceTool[]).map((t) => (
                <Badge key={t} tone={captain.charges[t] > 0 ? "info" : "dim"}>
                  {words(t)} ×{captain.charges[t]}
                </Badge>
              ))}
            </div>
          </Panel>
          <Panel title="Incidents" tag={`${captain.incidents.length}`}>
            <ul class="wp-feed" style={{ maxHeight: "10em" }}>
              {[...captain.incidents].reverse().slice(0, 30).map((i) => (
                <li key={i.id} style={{ gridTemplateColumns: "3.4em 1fr" }}>
                  <span class="wp-dim">{mmss(i.tS)}</span>
                  <span>
                    {words(i.kind)} {i.suspect !== null && <span class="wp-dim">· #{i.suspect}</span>}
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

function topSuspect(c: CaptainState): Suspect | undefined {
  return [...c.suspects].sort((a, b) => b.priority - a.priority || (b.visible ? 1 : 0) - (a.visible ? 1 : 0))[0];
}

function describeTarget(u: PoliceUnit, c: CaptainState): string {
  const t = u.target;
  if (!t) return u.status === "Returning" ? "returning to base" : "patrolling";
  if (t.kind === "racer") return `pursuing ${c.suspects.find((s) => s.id === t.id)?.name ?? `#${t.id}`}`;
  if (t.kind === "incident") return `responding to incident #${t.id}`;
  return `holding ${t.pos.map((v) => v.toFixed(0)).join(", ")}`;
}

// ------------------------------------------------------------------ radial menu

function RadialMenu(props: { radial: Radial; captain: CaptainState; selected: PoliceUnit | null; onClose: () => void; toWorld: (x: number, y: number) => Vec2 }) {
  const send = useSend();
  const { x, y, hit } = props.radial;
  const pos = hit.kind === "ground" ? hit.pos : hit.kind === "unit" ? hit.unit.pos : hit.kind === "suspect" ? hit.suspect.pos : hit.incident.pos;
  const cmd = (c: CaptainCommand) => {
    send({ type: "captain", command: c });
    props.onClose();
  };
  const nearestFree = [...props.captain.units]
    .filter((u) => u.kind !== "Helicopter" && u.status !== "Disabled")
    .sort((a, b) => Math.hypot(a.pos[0] - pos[0], a.pos[1] - pos[1]) - Math.hypot(b.pos[0] - pos[0], b.pos[1] - pos[1]))[0];
  const heli = props.captain.units.find((u) => u.kind === "Helicopter");
  const items: { label: string; disabled?: boolean; run: () => void }[] = [];
  const deploy = (tool: PoliceTool) => ({
    label: `${words(tool)} (${props.captain.charges[tool]})`,
    disabled: props.captain.charges[tool] <= 0,
    run: () => cmd({ Deploy: { tool, pos, heading_rad: 0 } }),
  });
  if (hit.kind === "unit") {
    items.push({ label: "Recall", run: () => cmd({ Recall: { unit: hit.unit.id } }) });
    const s = topSuspect(props.captain);
    if (s) items.push({ label: `Pursue ${s.name}`, run: () => cmd({ Assign: { unit: hit.unit.id, target: { Pursue: { racer: s.id } } } }) });
    items.push({ label: "Hold here", run: () => cmd({ Assign: { unit: hit.unit.id, target: { Hold: { pos } } } }) });
    items.push({ label: "Patrol area", run: () => cmd({ Assign: { unit: hit.unit.id, target: { Patrol: { center: pos, radius_m: 200 } } } }) });
  } else if (hit.kind === "suspect") {
    items.push({ label: "Priority 1", run: () => cmd({ SetPriority: { racer: hit.suspect.id, priority: 3 } }) });
    if (nearestFree) items.push({ label: `Send ${nearestFree.callsign}`, run: () => cmd({ Assign: { unit: nearestFree.id, target: { Pursue: { racer: hit.suspect.id } } } }) });
    if (heli) items.push({ label: "Air unit", run: () => cmd({ Assign: { unit: heli.id, target: { Pursue: { racer: hit.suspect.id } } } }) });
    items.push(deploy("Pit"));
  } else {
    items.push(deploy("Roadblock"), deploy("SpikeStrip"), deploy("Spotlight"));
    if (props.selected) items.push({ label: `Send ${props.selected.callsign}`, run: () => cmd({ Assign: { unit: props.selected!.id, target: hit.kind === "incident" ? { Incident: { id: hit.incident.id } } : { Hold: { pos } } } }) });
    else if (nearestFree) items.push({ label: `Send ${nearestFree.callsign}`, run: () => cmd({ Assign: { unit: nearestFree.id, target: hit.kind === "incident" ? { Incident: { id: hit.incident.id } } : { Hold: { pos } } } }) });
  }
  items.push({ label: "Cancel", run: props.onClose });
  // Items sit on an ellipse (labels are wide, so spread them more horizontally).
  const R = 70 + items.length * 6;
  return (
    <div class="wp-radial" style={{ left: `${x}px`, top: `${y}px` }} role="menu" onPointerDown={(e) => e.stopPropagation()} onContextMenu={(e) => e.preventDefault()}>
      <div class="wp-radial-hub wp-mono">{hit.kind === "unit" ? hit.unit.callsign : hit.kind === "suspect" ? `#${hit.suspect.id}` : hit.kind === "incident" ? words(hit.incident.kind) : "HERE"}</div>
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
    const sigma = 140;
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
  // POIs (stations) as small squares.
  ctx.fillStyle = col.polB;
  for (const p of map.pois) {
    if (p.kind !== "PoliceStation") continue;
    ctx.fillRect(p.pos[0] - 6 * px, p.pos[1] - 6 * px, 12 * px, 12 * px);
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
      } else if (t.tool === "Roadblock") {
        ctx.fillRect(x - 12, y - 3, 24, 6);
      } else {
        ctx.beginPath();
        ctx.arc(x, y, 12, 0, Math.PI * 2);
        ctx.globalAlpha = 0.25;
        ctx.fill();
        ctx.globalAlpha = 1;
      }
      if (on("labels")) ctx.fillText(words(t.tool), x + 14, y);
    }
  }

  if (on("incidents")) {
    for (const i of cap.incidents) {
      const age = nowS - i.tS;
      if (age < 0 || age > 90) continue;
      const [x, y] = screenOf(i.pos);
      ctx.globalAlpha = Math.max(0.25, 1 - age / 90);
      ctx.fillStyle = i.kind === "Crash" ? col.danger : i.kind === "Sighting" ? col.polA : col.warn;
      ctx.beginPath();
      ctx.moveTo(x, y - 7);
      ctx.lineTo(x + 6, y + 5);
      ctx.lineTo(x - 6, y + 5);
      ctx.closePath();
      ctx.fill();
      ctx.globalAlpha = 1;
    }
  }

  // Assignment lines.
  if (on("units")) {
    ctx.setLineDash([4, 4]);
    ctx.lineWidth = 1.2;
    for (const u of cap.units) {
      const t = targetPos(u, cap.suspects, cap.incidents);
      if (!t) continue;
      const [ax, ay] = screenOf(u.pos);
      const [bx, by] = screenOf(t);
      ctx.strokeStyle = u.id === selected ? col.ink : "rgba(59,125,255,0.6)";
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
      if (!s.visible) {
        ctx.strokeStyle = c;
        ctx.setLineDash([3, 3]);
        ctx.beginPath();
        ctx.arc(x, y, 18, 0, Math.PI * 2);
        ctx.stroke();
        ctx.setLineDash([]);
      }
      if (on("labels")) {
        ctx.fillStyle = col.ink;
        ctx.fillText(`${s.name}${lvl ? ` L${lvl}` : ""}${s.priority > 1 ? ` P${s.priority}` : ""}`, x + 12, y - 10);
      }
    }
  }

  if (on("units")) {
    for (const u of cap.units) {
      const [x, y] = screenOf(u.pos);
      const isSel = u.id === selected;
      ctx.save();
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
        ctx.fillStyle = u.status === "Pursuit" ? col.polA : col.polB;
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
      if (on("labels")) {
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
