// Controls settings panel: lists every action with two keyboard and two gamepad slots. Click a
// slot, then press a key (or a pad button / push a stick or trigger) to rebind it; Esc cancels,
// Delete clears the slot. Changes save immediately to localStorage (`wp.bindings.v1`). Game input
// is neutral while the panel is open.
//
// Below the bindings: per-device input shaping (dead zone, saturation, sensitivity, speed
// sensitivity, pedal dead zones / saturation) and live test bars of the processed input. The
// values belong to the game menu's settings (`shaping` link, wired by main), so the panel and
// Settings → Controls always show and edit the same numbers.
import {
  ACTIONS,
  SLOTS,
  assign,
  clearSavedBindings,
  defaultBindings,
  keyName,
  padName,
  saveBindings,
  type ActionId,
  type PadControl,
} from "../bindings";
import { allPads, captureKey, firstPad, getBindings, getDeviceControls, PAD_PRESS, probeInput, setBindings, setDeviceControls, setInputBlocked } from "../input";
import { CONTROL_DEVICES, CONTROL_RANGES, defaultDeviceControls, type ControlDevice, type DeviceControls } from "../../../ui-kit/src/menu/controls";
import { paintInputBars } from "../../../ui-kit/src/menu/bars";

/** Where the shaping values live (main points this at the game menu's settings). */
export interface ShapingLink {
  /** Current per-device profiles. */
  get(): Record<ControlDevice, DeviceControls>;
  /** Store new per-device profiles (applies + persists). */
  set(c: Record<ControlDevice, DeviceControls>): void;
}

/** Shaping sliders, in panel order. */
const SHAPE_ROWS: { key: keyof DeviceControls; label: string }[] = [
  { key: "steerDeadzone", label: "steering dead zone" },
  { key: "steerSaturation", label: "steering saturation" },
  { key: "steerSensitivity", label: "steering sensitivity" },
  { key: "steerSpeedSensitivity", label: "steering speed sensitivity" },
  { key: "throttleSaturation", label: "accelerator saturation" },
  { key: "throttleDeadzone", label: "accelerator dead zone" },
  { key: "brakeSaturation", label: "brake saturation" },
  { key: "brakeDeadzone", label: "brake dead zone" },
  { key: "clutchSaturation", label: "clutch saturation" },
  { key: "clutchDeadzone", label: "clutch dead zone" },
];

type Device = "keys" | "pad";

/** The open/close-able controls panel. */
export class ControlsPanel {
  private readonly root: HTMLElement;
  private readonly table: HTMLTableElement;
  private readonly status: HTMLElement;
  private readonly dz: HTMLInputElement;
  private readonly curve: HTMLInputElement;
  private capturing: { device: Device; action: ActionId; slot: number } | null = null;
  private padRaf = 0;
  private barsRaf = 0;
  private shapeDevice: ControlDevice = "keyboard";
  /** Called after every change (main refreshes the F1 help). */
  onChange: (() => void) | null = null;
  /** Where the shaping values are read and written. Default: the input module only (not saved). */
  shaping: ShapingLink = { get: getDeviceControls, set: setDeviceControls };

  constructor(parent: HTMLElement) {
    this.root = document.createElement("div");
    this.root.className = "panel help controls-panel hidden";
    this.root.id = "controls-panel";
    this.root.innerHTML =
      `<b>Controls</b> <span class="dim">click a slot, then press a key or a pad button / move a stick · Esc cancel · Delete clear</span>` +
      `<table class="bind-table"></table>` +
      `<div class="row">stick deadzone <input id="bind-dz" type="range" min="0" max="0.4" step="0.01" /> <span id="bind-dz-v"></span>` +
      ` · steering curve <input id="bind-curve" type="range" min="1" max="2.5" step="0.1" /> <span id="bind-curve-v"></span></div>` +
      `<div class="row dim">fixed: 1–8 cameras · debug-layer keys (F1 help) · keys inside the open city map · right stick looks around</div>` +
      `<div class="row"><b>Input shaping</b> device <select id="shape-dev">${CONTROL_DEVICES.map((d) => `<option value="${d}">${d}</option>`).join("")}</select>` +
      ` <button id="shape-reset">reset device</button> <span class="dim">same values as Settings → Controls</span></div>` +
      `<div class="shape-grid">${SHAPE_ROWS.map((r) => {
        const [min, max] = CONTROL_RANGES[r.key];
        return `<span>${r.label}</span><input type="range" id="shape-${r.key}" data-k="${r.key}" min="${min}" max="${max}" step="0.05" /><span class="dim" data-v="${r.key}"></span>`;
      }).join("")}</div>` +
      `<div class="shape-bars">${(["steer", "throttle", "brake", "clutch"] as const)
        .map((ch) => `<span>${ch}</span><div class="shape-bar${ch === "steer" ? " bipolar" : ""}" data-bar="${ch}"><i></i></div><span data-bar-value="${ch}">0%</span>`)
        .join("")}<span class="dim" id="shape-inuse" style="grid-column: 1 / -1">hold driving keys / pedals: the bars show the processed input</span></div>` +
      `<div class="row"><button id="bind-reset">reset to defaults</button> <button id="bind-close">close</button> <span id="bind-status" class="dim"></span></div>`;
    parent.appendChild(this.root);
    this.table = this.root.querySelector("table")!;
    this.status = this.root.querySelector("#bind-status")!;
    this.dz = this.root.querySelector("#bind-dz")!;
    this.curve = this.root.querySelector("#bind-curve")!;
    this.dz.addEventListener("input", () => {
      getBindings().padDeadzone = Number(this.dz.value);
      this.changed();
    });
    this.curve.addEventListener("input", () => {
      getBindings().padCurve = Number(this.curve.value);
      this.changed();
    });
    this.root.querySelector("#bind-reset")!.addEventListener("click", () => {
      this.stopCapture();
      clearSavedBindings();
      setBindings(defaultBindings());
      this.status.textContent = "defaults restored";
      this.render();
      this.onChange?.();
    });
    this.root.querySelector("#bind-close")!.addEventListener("click", () => this.close());
    const dev = this.root.querySelector<HTMLSelectElement>("#shape-dev")!;
    dev.addEventListener("change", () => {
      this.shapeDevice = dev.value as ControlDevice;
      this.refreshShaping();
    });
    this.root.querySelector("#shape-reset")!.addEventListener("click", () => this.setShape(defaultDeviceControls(this.shapeDevice)));
    this.root.querySelectorAll<HTMLInputElement>(".shape-grid input").forEach((el) =>
      el.addEventListener("input", () => this.setShape({ [el.dataset.k as keyof DeviceControls]: Number(el.value) })),
    );
    this.render();
  }

  /** Re-read the shaping values into the sliders (main calls it when the settings change). */
  refreshShaping(): void {
    const c = this.shaping.get()[this.shapeDevice];
    (this.root.querySelector("#shape-dev") as HTMLSelectElement).value = this.shapeDevice;
    for (const r of SHAPE_ROWS) {
      const el = this.root.querySelector<HTMLInputElement>(`#shape-${r.key}`)!;
      el.value = String(c[r.key]);
      this.root.querySelector(`[data-v="${r.key}"]`)!.textContent = `${Math.round(c[r.key] * 100)}%`;
    }
  }

  private setShape(patch: Partial<DeviceControls>): void {
    const all = this.shaping.get();
    this.shaping.set({ ...all, [this.shapeDevice]: { ...all[this.shapeDevice], ...patch } });
    this.refreshShaping();
  }

  /** Paint the test bars every frame while open. */
  private watchBars(): void {
    cancelAnimationFrame(this.barsRaf);
    const box = this.root.querySelector<HTMLElement>(".shape-bars")!;
    const inUse = this.root.querySelector<HTMLElement>("#shape-inuse")!;
    const tick = () => {
      if (!this.isOpen) return;
      const p = probeInput();
      paintInputBars(box, p);
      inUse.textContent = `in use: ${p.device}`;
      this.barsRaf = requestAnimationFrame(tick);
    };
    this.barsRaf = requestAnimationFrame(tick);
  }

  /** Is the panel open? */
  get isOpen(): boolean {
    return !this.root.classList.contains("hidden");
  }

  /** Open the panel (blocks game input). */
  open(): void {
    this.root.classList.remove("hidden");
    setInputBlocked(true);
    const pads = allPads();
    this.status.textContent = pads.length ? `devices: ${pads.map((p) => p.id).join(" · ")}` : "no gamepad / wheel connected (press a button to wake it)";
    this.render();
    // Edit the profile of the device in use.
    this.shapeDevice = probeInput().device;
    this.refreshShaping();
    this.watchBars();
    // Esc closes the panel when nothing is being rebound.
    captureKey((e) => this.onKey(e));
  }

  /** Close the panel (unblocks game input). */
  close(): void {
    this.stopCapture();
    captureKey(null);
    cancelAnimationFrame(this.barsRaf);
    this.root.classList.add("hidden");
    setInputBlocked(false);
  }

  /** Toggle open/closed. */
  toggle(): void {
    if (this.isOpen) this.close();
    else this.open();
  }

  private changed(): void {
    saveBindings(getBindings());
    this.render();
    this.onChange?.();
  }

  private render(): void {
    const b = getBindings();
    this.dz.value = String(b.padDeadzone);
    this.curve.value = String(b.padCurve);
    this.root.querySelector("#bind-dz-v")!.textContent = `${Math.round(b.padDeadzone * 100)}%`;
    this.root.querySelector("#bind-curve-v")!.textContent = b.padCurve.toFixed(1);
    const head = `<tr><th></th>${"<th>key</th>".repeat(SLOTS)}${"<th>gamepad</th>".repeat(SLOTS)}</tr>`;
    const rows = ACTIONS.map((a) => {
      const cell = (device: Device, slot: number) => {
        const cap = this.capturing;
        const waiting = cap && cap.device === device && cap.action === a.id && cap.slot === slot;
        const v = device === "keys" ? b.keys[a.id][slot] : b.pad[a.id][slot];
        const text = waiting ? "press…" : v === undefined ? "–" : device === "keys" ? keyName(v as string) : padName(v as PadControl);
        return `<td><button class="bind${waiting ? " waiting" : ""}" data-d="${device}" data-a="${a.id}" data-s="${slot}">${text}</button></td>`;
      };
      const slots = (d: Device) => Array.from({ length: SLOTS }, (_, s) => cell(d, s)).join("");
      return `<tr><td>${a.label}</td>${slots("keys")}${slots("pad")}</tr>`;
    }).join("");
    this.table.innerHTML = head + rows;
    this.table.querySelectorAll<HTMLButtonElement>("button.bind").forEach((btn) =>
      btn.addEventListener("click", () => this.startCapture(btn.dataset.d as Device, btn.dataset.a as ActionId, Number(btn.dataset.s))),
    );
  }

  private startCapture(device: Device, action: ActionId, slot: number): void {
    this.stopCapture();
    this.capturing = { device, action, slot };
    this.status.textContent = device === "keys" ? "press a key" : "press a pad / wheel button or move a stick, trigger or pedal";
    if (device === "pad") this.watchPad();
    this.render();
  }

  private stopCapture(): void {
    this.capturing = null;
    cancelAnimationFrame(this.padRaf);
  }

  private onKey(e: KeyboardEvent): void {
    const cap = this.capturing;
    if (e.code === "Escape") {
      if (cap) {
        this.stopCapture();
        this.status.textContent = "cancelled";
        this.render();
      } else this.close();
      return;
    }
    if (!cap) {
      if (getBindings().keys.controls.includes(e.code)) this.close();
      return;
    }
    if (e.code === "Delete") {
      this.finish(null);
      return;
    }
    if (cap.device === "keys") this.finish(e.code);
  }

  /** Poll every pad / wheel until a control moves past PAD_PRESS from where it rested when
   * capture began. An axis that rested near one end (a wheel pedal) is stored as a full-range
   * pedal axis; a centred axis as one stick direction. */
  private watchPad(): void {
    const rest = new Map<number, { axes: number[]; buttons: number[] }>();
    const snap = (p: Gamepad) => ({ axes: [...p.axes], buttons: p.buttons.map((b) => b.value) });
    for (const p of allPads()) rest.set(p.index, snap(p));
    const tick = () => {
      if (!this.capturing) return;
      for (const pad of allPads()) {
        const r = rest.get(pad.index);
        if (!r) {
          // A device woke up after capture began: its current state is its rest pose.
          rest.set(pad.index, snap(pad));
          continue;
        }
        const bi = pad.buttons.findIndex((b, i) => b.value > PAD_PRESS && (r.buttons[i] ?? 0) <= PAD_PRESS);
        if (bi >= 0) return this.finish({ button: bi });
        for (let i = 0; i < pad.axes.length; i++) {
          const r0 = r.axes[i] ?? 0;
          const d = pad.axes[i] - r0;
          if (Math.abs(r0) > 0.8) {
            // Pedal: rests at one end; travelling half way is enough to bind it.
            if (Math.abs(d) > 1.0) return this.finish({ axis: i, dir: d > 0 ? 1 : -1, full: true });
          } else if (Math.abs(d) > PAD_PRESS && Math.abs(pad.axes[i]) > PAD_PRESS) {
            return this.finish({ axis: i, dir: pad.axes[i] > 0 ? 1 : -1 });
          }
        }
      }
      this.padRaf = requestAnimationFrame(tick);
    };
    this.padRaf = requestAnimationFrame(tick);
  }

  private finish(value: string | PadControl | null): void {
    const cap = this.capturing;
    if (!cap) return;
    this.stopCapture();
    const b = getBindings();
    const from =
      cap.device === "keys"
        ? assign(b, "keys", cap.action, cap.slot, value as string | null)
        : assign(b, "pad", cap.action, cap.slot, value as PadControl | null);
    const label = (id: ActionId) => ACTIONS.find((a) => a.id === id)?.label ?? id;
    this.status.textContent = value === null ? "cleared" : from ? `saved (moved from “${label(from)}”)` : "saved";
    this.changed();
  }
}
