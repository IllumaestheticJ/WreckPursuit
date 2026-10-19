// Captain map view: 2D canvas with units, suspects, incidents, heat and a command radial.
// Left-click selects police units (shift adds), drag pans, wheel zooms, right-click opens the
// radial at the cursor. On a suspect: pursue / intercept (selected units), roadblock, spikes,
// helicopter, priority, call off. On empty map: patrol / disengage (selected units), clear
// priority. Commands are the contract's typed `CaptainCommand`s, validated by wp_police.
// Roads come from the road graph once wp_world publishes one; until then a 50 m grid is drawn.
import * as net from "./wasm";
import type { CaptainCommand, CaptainMapData, MapUnit, UnitOrder } from "./wasm";

const ORDER_NAMES = ["", "patrol", "pursue", "intercept", "respond", "disengage"];

type Choice =
  | "pursue" | "intercept" | "roadblock" | "spikes" | "helicopter" | "priority" | "call off"
  | "patrol" | "disengage" | "clear priority";

export class CaptainMap {
  readonly canvas = document.createElement("canvas");
  private ctx: CanvasRenderingContext2D;
  private center = { x: 0, y: 0 };
  private scale = 2; // px per metre
  private selected = new Set<number>();
  private radial: { sx: number; sy: number; wx: number; wy: number; target?: MapUnit } | null = null;
  private data: CaptainMapData = { tick: 0, units: [], incidents: [], heat: [] };
  private drag: { x: number; y: number } | null = null;

  constructor(parent: HTMLElement) {
    this.canvas.className = "captain-map";
    parent.append(this.canvas);
    this.ctx = this.canvas.getContext("2d")!;
    this.canvas.addEventListener("contextmenu", (e) => e.preventDefault());
    this.canvas.addEventListener("wheel", (e) => {
      e.preventDefault();
      this.scale = Math.min(20, Math.max(0.2, this.scale * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
    });
    this.canvas.addEventListener("mousedown", (e) => this.onDown(e));
    window.addEventListener("mousemove", (e) => {
      if (!this.drag) return;
      this.center.x -= (e.clientX - this.drag.x) / this.scale;
      this.center.y += (e.clientY - this.drag.y) / this.scale;
      this.drag = { x: e.clientX, y: e.clientY };
    });
    window.addEventListener("mouseup", () => (this.drag = null));
  }

  set visible(v: boolean) {
    this.canvas.style.display = v ? "block" : "none";
  }

  private toScreen(x: number, y: number): [number, number] {
    return [
      this.canvas.width / 2 + (x - this.center.x) * this.scale,
      this.canvas.height / 2 - (y - this.center.y) * this.scale,
    ];
  }

  private toWorld(sx: number, sy: number): [number, number] {
    return [
      this.center.x + (sx - this.canvas.width / 2) / this.scale,
      this.center.y - (sy - this.canvas.height / 2) / this.scale,
    ];
  }

  private unitAt(sx: number, sy: number): MapUnit | undefined {
    return this.data.units.find((u) => {
      const [ux, uy] = this.toScreen(u.x, u.y);
      return Math.hypot(ux - sx, uy - sy) < 10;
    });
  }

  private onDown(e: MouseEvent): void {
    const r = this.canvas.getBoundingClientRect();
    const sx = e.clientX - r.left;
    const sy = e.clientY - r.top;
    if (this.radial && e.button === 0) {
      const choice = this.radialChoice(sx, sy);
      if (choice) this.issue(choice);
      this.radial = null;
      return;
    }
    const hit = this.unitAt(sx, sy);
    if (e.button === 2) {
      const [wx, wy] = this.toWorld(sx, sy);
      const target = hit && (hit.kind === "racer") ? hit : undefined;
      this.radial = { sx, sy, wx, wy, target };
      return;
    }
    if (hit && (hit.kind === "police" || hit.kind === "ai_police")) {
      if (!e.shiftKey) this.selected.clear();
      if (this.selected.has(hit.entity)) this.selected.delete(hit.entity);
      else this.selected.add(hit.entity);
    } else {
      if (!e.shiftKey) this.selected.clear();
      this.drag = { x: e.clientX, y: e.clientY };
    }
  }

  private radialOptions(): Choice[] {
    return this.radial?.target
      ? ["pursue", "intercept", "roadblock", "spikes", "helicopter", "priority", "call off"]
      : ["patrol", "disengage", "clear priority"];
  }

  private radialChoice(sx: number, sy: number): Choice | null {
    if (!this.radial) return null;
    const opts = this.radialOptions();
    const dx = sx - this.radial.sx;
    const dy = sy - this.radial.sy;
    if (Math.hypot(dx, dy) < 12 || Math.hypot(dx, dy) > 80) return null;
    const a = (Math.atan2(dy, dx) + Math.PI * 2.5) % (Math.PI * 2);
    return opts[Math.floor(a / (Math.PI * 2 / opts.length))];
  }

  private issue(choice: Choice): void {
    if (!this.radial) return;
    const now = performance.now();
    const target = this.radial.target?.entity;
    const assign = (order: UnitOrder) => {
      for (const entity of this.selected) {
        const unit = this.data.units.find((u) => u.entity === entity)?.unit;
        if (unit != null) net.captainCommand({ AssignUnit: { unit, order } }, now);
      }
    };
    let cmd: CaptainCommand | null = null;
    switch (choice) {
      case "pursue": if (target != null) assign({ Pursue: { target } }); break;
      case "intercept": if (target != null) assign({ Intercept: { target } }); break;
      case "patrol": assign("Patrol"); break;
      case "disengage": assign("Disengage"); break;
      case "roadblock": if (target != null) cmd = { RequestRoadblock: { target, road: null } }; break;
      case "spikes": if (target != null) cmd = { RequestSpikes: { target, road: null } }; break;
      case "helicopter": if (target != null) cmd = { RequestHelicopter: { target } }; break;
      case "priority": if (target != null) cmd = { SetPriorityTarget: { target } }; break;
      case "clear priority": cmd = { SetPriorityTarget: { target: null } }; break;
      case "call off": if (target != null) cmd = { CallOff: { target } }; break;
    }
    if (cmd) net.captainCommand(cmd, now);
  }

  /** Last captain reply, shown in the footer. */
  lastReply = "";

  draw(data: CaptainMapData): void {
    this.data = data;
    const c = this.canvas;
    if (c.width !== c.clientWidth || c.height !== c.clientHeight) {
      c.width = c.clientWidth;
      c.height = c.clientHeight;
    }
    const g = this.ctx;
    g.fillStyle = "#14181c";
    g.fillRect(0, 0, c.width, c.height);

    // Grid (placeholder for the road graph).
    g.strokeStyle = "#2a3238";
    g.lineWidth = 1;
    const step = 50;
    const [x0, y1] = this.toWorld(0, 0);
    const [x1, y0] = this.toWorld(c.width, c.height);
    for (let x = Math.floor(x0 / step) * step; x <= x1; x += step) {
      const [sx] = this.toScreen(x, 0);
      g.beginPath(); g.moveTo(sx, 0); g.lineTo(sx, c.height); g.stroke();
    }
    for (let y = Math.floor(y0 / step) * step; y <= y1; y += step) {
      const [, sy] = this.toScreen(0, y);
      g.beginPath(); g.moveTo(0, sy); g.lineTo(c.width, sy); g.stroke();
    }

    for (const h of data.heat) {
      const [sx, sy] = this.toScreen(h.x, h.y + h.size);
      g.fillStyle = `rgba(255, 60, 30, ${(h.heat / 255) * 0.5})`;
      g.fillRect(sx, sy, h.size * this.scale, h.size * this.scale);
    }
    for (const i of data.incidents) {
      const [sx, sy] = this.toScreen(i.x, i.y);
      g.strokeStyle = `rgba(255, 170, 0, ${1 - i.age_s / 60})`;
      g.lineWidth = 2;
      g.beginPath(); g.moveTo(sx - 5, sy - 5); g.lineTo(sx + 5, sy + 5);
      g.moveTo(sx + 5, sy - 5); g.lineTo(sx - 5, sy + 5); g.stroke();
    }
    for (const u of data.units) {
      const [sx, sy] = this.toScreen(u.x, u.y);
      const police = u.kind === "police" || u.kind === "ai_police";
      g.save();
      g.translate(sx, sy);
      g.rotate(-u.heading);
      g.fillStyle = police ? "#3f8cff" : u.kind === "racer" ? "#ff4a3a" : "#888";
      g.beginPath(); g.moveTo(0, -9); g.lineTo(6, 7); g.lineTo(-6, 7); g.closePath(); g.fill();
      if (this.selected.has(u.entity)) {
        g.strokeStyle = "#fff"; g.lineWidth = 2; g.stroke();
      }
      g.restore();
      g.fillStyle = "#ccc";
      g.font = "10px monospace";
      const label = police ? `${u.entity}${u.order ? " " + ORDER_NAMES[u.order] : ""}`
        : `${u.entity}${u.wanted ? " ★" + u.wanted : ""}`;
      g.fillText(label, sx + 9, sy + 3);
    }

    if (this.radial) {
      const opts = this.radialOptions();
      const { sx, sy } = this.radial;
      g.fillStyle = "rgba(0,0,0,0.6)";
      g.beginPath(); g.arc(sx, sy, 80, 0, Math.PI * 2); g.fill();
      opts.forEach((o, k) => {
        const a = -Math.PI / 2 + (k + 0.5) * (Math.PI * 2 / opts.length);
        g.fillStyle = "#fff";
        g.font = "13px sans-serif";
        g.textAlign = "center";
        g.fillText(o, sx + Math.cos(a) * 50, sy + Math.sin(a) * 50 + 4);
      });
      g.textAlign = "start";
    }
    g.fillStyle = "#9f9";
    g.font = "12px monospace";
    g.fillText(`captain map · ${data.units.length} units · selected ${[...this.selected].join(",") || "none"} · ` +
      "click select, shift multi, right-click command, drag pan, wheel zoom" +
      (this.lastReply ? ` · last: ${this.lastReply}` : ""), 8, c.height - 8);
  }
}
