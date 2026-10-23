/**
 * Settings: controls (full remapping incl. gamepad/wheel binding flow), assist matrix, damage
 * mode, accessibility, feedback & audio. Every change is a `UiCommand`; nothing is local state
 * except the open tab and the binding-capture dialog.
 */
import { useEffect, useRef, useState } from "preact/hooks";
import type { AssistProfile, DamageMode } from "../../generated/contracts";
import { ACTIONS, codeLabel, conflictFor, detectPadInput, deviceOf, type InputAction, type InputDevice } from "../../model/bindings";
import { useSend, useSlice } from "../../model/GameModel";
import type { AssistPreset, Palette } from "../../model/types";
import { ASSIST_PRESETS } from "../../mock/mockData";
import { Badge, Button, ChoiceField, Modal, Panel, ScreenHead, SliderField, Tabs, ToggleField, useEscape } from "../../ui/kit";
import "./settings.css";
import { ASSIST_LEVELS } from "../../menu/settings";

type Tab = "controls" | "assists" | "damage" | "access" | "feedback";

/** Settings screen. */
export function Settings() {
  const [tab, setTab] = useState<Tab>("controls");
  const send = useSend();
  useEscape(() => send({ type: "back" }));
  return (
    <div class="wp-screen">
      <ScreenHead kicker="// SYSTEM" title="Settings" back="back" />
      <Tabs<Tab>
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "controls", label: "Controls" },
          { id: "assists", label: "Assists" },
          { id: "damage", label: "Damage mode" },
          { id: "access", label: "Accessibility" },
          { id: "feedback", label: "Feedback & audio" },
        ]}
      />
      {tab === "controls" && <Controls />}
      {tab === "assists" && <Assists />}
      {tab === "damage" && <DamageMode />}
      {tab === "access" && <Accessibility />}
      {tab === "feedback" && <Feedback />}
    </div>
  );
}

// ------------------------------------------------------------------ controls

interface PadInfo {
  index: number;
  id: string;
  device: Exclude<InputDevice, "keyboard">;
}

function usePads(): PadInfo[] {
  const [pads, setPads] = useState<PadInfo[]>([]);
  useEffect(() => {
    const scan = () => {
      const list = typeof navigator !== "undefined" && navigator.getGamepads ? Array.from(navigator.getGamepads()) : [];
      setPads(list.filter((p): p is Gamepad => !!p).map((p) => ({ index: p.index, id: p.id, device: deviceOf(p.id) })));
    };
    scan();
    window.addEventListener("gamepadconnected", scan);
    window.addEventListener("gamepaddisconnected", scan);
    return () => {
      window.removeEventListener("gamepadconnected", scan);
      window.removeEventListener("gamepaddisconnected", scan);
    };
  }, []);
  return pads;
}

const DEVICES: { id: InputDevice; label: string }[] = [
  { id: "keyboard", label: "Keyboard" },
  { id: "gamepad", label: "Gamepad" },
  { id: "wheel", label: "Wheel & pedals" },
];

function Controls() {
  const bindings = useSlice((s) => s.settings.bindings);
  const input = useSlice((s) => s.settings.input);
  const send = useSend();
  const pads = usePads();
  const [capture, setCapture] = useState<{ action: InputAction; device: InputDevice } | null>(null);
  const groups = [...new Set(ACTIONS.map((a) => a.group))];
  return (
    <div class="wp-grid wp-settings-controls">
      <Panel title="Bindings" tag="Click a cell to rebind">
        <table class="wp-table wp-bind-table">
          <thead>
            <tr>
              <th>Action</th>
              {DEVICES.map((d) => (
                <th key={d.id}>{d.label}</th>
              ))}
            </tr>
          </thead>
          {groups.map((g) => (
            <tbody key={g}>
              <tr>
                <td colSpan={4} class="wp-bind-group">
                  {g}
                </td>
              </tr>
              {ACTIONS.filter((a) => a.group === g).map((a) => (
                <tr key={a.action}>
                  <td>{a.label}</td>
                  {DEVICES.map((d) => (
                    <td key={d.id}>
                      <button type="button" class="wp-bind-cell" onClick={() => setCapture({ action: a.action, device: d.id })} aria-label={`Rebind ${a.label} on ${d.label}`}>
                        {codeLabel(bindings[a.action][d.id])}
                      </button>
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          ))}
        </table>
        <div class="wp-row" style={{ marginTop: "0.8em" }}>
          <Button size="sm" onClick={() => send({ type: "settings.resetBindings" })}>
            Reset to defaults
          </Button>
        </div>
      </Panel>
      <div class="wp-stack">
        <Panel title="Devices">
          <ul class="wp-device-list">
            <li>
              <Badge tone="ok">KEYBOARD</Badge> always available
            </li>
            {pads.length === 0 && <li class="wp-dim">No gamepad or wheel detected. Press a button on it to wake it up.</li>}
            {pads.map((p) => (
              <li key={p.index}>
                <Badge tone="info">{p.device.toUpperCase()}</Badge> <span class="wp-mono wp-dim2">{p.id}</span>
              </li>
            ))}
          </ul>
        </Panel>
        <Panel title="Steering & pedals">
          <SliderField label="Steering deadzone" value={input.steerDeadzone} min={0} max={0.3} step={0.01} onChange={(v) => send({ type: "settings.patch", patch: { input: { steerDeadzone: v } } })} />
          <SliderField
            label="Steering linearity"
            hint="1 = linear; higher = finer control near centre"
            value={input.steerLinearity}
            min={1}
            max={3}
            step={0.1}
            format={(v) => v.toFixed(1)}
            onChange={(v) => send({ type: "settings.patch", patch: { input: { steerLinearity: v } } })}
          />
          <SliderField
            label="Wheel rotation"
            hint="Match your wheel's lock-to-lock setting"
            value={input.wheelRotationDeg}
            min={180}
            max={1080}
            step={10}
            format={(v) => `${v}°`}
            onChange={(v) => send({ type: "settings.patch", patch: { input: { wheelRotationDeg: v } } })}
          />
          <ToggleField label="Combined pedals" hint="Throttle and brake on one axis" checked={input.combinedPedals} onChange={(v) => send({ type: "settings.patch", patch: { input: { combinedPedals: v } } })} />
        </Panel>
      </div>
      {capture && <CaptureDialog action={capture.action} device={capture.device} pads={pads} onClose={() => setCapture(null)} />}
    </div>
  );
}

/**
 * The binding flow: listen for the next key (keyboard) or the next button/axis movement
 * (gamepad/wheel, compared against a baseline so resting pedals don't bind), show conflicts,
 * confirm.
 */
function CaptureDialog(props: { action: InputAction; device: InputDevice; pads: PadInfo[]; onClose: () => void }) {
  const send = useSend();
  const bindings = useSlice((s) => s.settings.bindings);
  const [code, setCode] = useState<string | null>(null);
  const baseline = useRef<Map<number, { buttons: number[]; axes: number[] }>>(new Map());
  const label = ACTIONS.find((a) => a.action === props.action)?.label ?? props.action;
  const deviceLabel = DEVICES.find((d) => d.id === props.device)?.label ?? props.device;

  useEffect(() => {
    if (code) return;
    if (props.device === "keyboard") {
      const onKey = (e: KeyboardEvent) => {
        e.preventDefault();
        e.stopPropagation();
        if (e.code === "Escape" && !e.shiftKey) return props.onClose(); // Shift+Esc binds Escape
        setCode(e.code);
      };
      window.addEventListener("keydown", onKey, { capture: true });
      return () => window.removeEventListener("keydown", onKey, { capture: true });
    }
    let raf = 0;
    const snap = (p: Gamepad) => ({ buttons: p.buttons.map((b) => b.value), axes: [...p.axes] });
    const poll = () => {
      const pads = navigator.getGamepads ? Array.from(navigator.getGamepads()) : [];
      for (const p of pads) {
        if (!p || deviceOf(p.id) !== props.device) continue;
        const base = baseline.current.get(p.index);
        if (!base) {
          baseline.current.set(p.index, snap(p));
          continue;
        }
        const hit = detectPadInput(snap(p), base);
        if (hit) {
          setCode(hit);
          return;
        }
      }
      raf = requestAnimationFrame(poll);
    };
    raf = requestAnimationFrame(poll);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
    window.addEventListener("keydown", onKey);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("keydown", onKey);
    };
  }, [code, props.device]);

  const conflict = code ? conflictFor(bindings, props.action, props.device, code) : null;
  const conflictLabel = conflict ? ACTIONS.find((a) => a.action === conflict)?.label : null;
  const devicePresent = props.device === "keyboard" || props.pads.some((p) => p.device === props.device);
  const confirm = () => {
    if (code) send({ type: "settings.bind", action: props.action, device: props.device, code });
    props.onClose();
  };

  return (
    <Modal title={`Rebind · ${deviceLabel}`} onClose={props.onClose}>
      <div class="wp-capture" data-capturing>
        <div class="wp-capture-action">{label}</div>
        {!code ? (
          <>
            <div class="wp-capture-listen">
              <span class="wp-capture-dot" />
              {props.device === "keyboard" ? "Press a key…" : "Press a button or move an axis…"}
            </div>
            {!devicePresent && <p class="wp-warn">No {deviceLabel.toLowerCase()} detected — connect one and press any button.</p>}
            <p class="wp-dim wp-mono" style={{ fontSize: "0.8em" }}>
              Current: {codeLabel(bindings[props.action][props.device])} · Esc cancels{props.device === "keyboard" ? " · Shift+Esc binds Esc" : ""}
            </p>
          </>
        ) : (
          <>
            <div class="wp-capture-got wp-mono">{codeLabel(code)}</div>
            {conflict && (
              <p class="wp-warn">
                Already used by <b>{conflictLabel}</b>. Confirming swaps them: {conflictLabel} gets {codeLabel(bindings[props.action][props.device])}.
              </p>
            )}
          </>
        )}
        <div class="wp-row" style={{ justifyContent: "flex-end", marginTop: "1em" }}>
          <Button size="sm" variant="ghost" onClick={() => send({ type: "settings.unbind", action: props.action, device: props.device })}>
            Clear binding
          </Button>
          {code && (
            <Button size="sm" onClick={() => setCode(null)}>
              Try again
            </Button>
          )}
          <Button size="sm" variant="primary" disabled={!code} onClick={confirm}>
            {conflict ? "Swap" : "Bind"}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

// ------------------------------------------------------------------ assists

const ASSIST_ROWS: { key: keyof AssistProfile; label: string; hint: string; kind: "bool" | "frac" | "level" }[] = [
  { key: "abs", label: "ABS", hint: "Off / Half (short lock-ups) / Full", kind: "level" },
  { key: "tcs", label: "Traction control", hint: "Off (raw throttle) / Half (some wheelspin) / Full", kind: "level" },
  { key: "esc", label: "Stability control", hint: "Brakes single wheels to stop spins", kind: "bool" },
  { key: "steer_speed_sensitivity", label: "Speed-sensitive steering", hint: "Less lock at speed", kind: "frac" },
  { key: "countersteer_help", label: "Countersteer help", hint: "Hands off in a slide: the wheels follow it (up to 30 % lock)", kind: "frac" },
  { key: "throttle_filter", label: "Throttle smoothing", hint: "Digital → gradual throttle", kind: "frac" },
  { key: "brake_filter", label: "Brake smoothing", hint: "Digital → gradual brake", kind: "frac" },
  { key: "auto_gears", label: "Automatic gears", hint: "", kind: "bool" },
  { key: "auto_clutch", label: "Automatic clutch", hint: "", kind: "bool" },
];

const PRESET_COLS: { id: Exclude<AssistPreset, "custom">; label: string; note: string }[] = [
  { id: "rookie", label: "Rookie", note: "Arcade, all on" },
  { id: "street", label: "Street", note: "Arcade, ABS Full + TCS Half" },
  { id: "sim", label: "Sim", note: "Simulation, ABS Half, TCS off" },
];

function Assists() {
  const assists = useSlice((s) => s.settings.assists);
  const preset = useSlice((s) => s.settings.assistPreset);
  const send = useSend();
  const set = (k: keyof AssistProfile, v: boolean | number | string) => send({ type: "settings.patch", patch: { assists: { ...assists, [k]: v } } });
  const fmt = (v: boolean | number | string) =>
    typeof v === "string" ? (v === "Off" ? "—" : v.toUpperCase()) : typeof v === "boolean" ? (v ? "ON" : "—") : v === 0 ? "—" : `${Math.round(v * 100)}%`;
  return (
    <Panel title="Assist matrix" tag={preset === "custom" ? "CUSTOM" : preset.toUpperCase()}>
      <p class="wp-dim" style={{ marginTop: 0 }}>
        A preset sets the damage mode and the assists in one click; they stay independent afterwards. Pick a preset column, then fine-tune your own column.
      </p>
      <table class="wp-table wp-assist-table">
        <thead>
          <tr>
            <th>Assist</th>
            {PRESET_COLS.map((c) => (
              <th key={c.id} class="num">
                <Button size="sm" pressed={preset === c.id} onClick={() => send({ type: "settings.assistPreset", preset: c.id })}>
                  {c.label}
                </Button>
                <div class="wp-dim" style={{ fontSize: "0.85em", marginTop: "0.3em", textTransform: "none", letterSpacing: 0 }}>
                  {c.note}
                </div>
              </th>
            ))}
            <th>Yours</th>
          </tr>
        </thead>
        <tbody>
          {ASSIST_ROWS.map((r) => (
            <tr key={r.key}>
              <td>
                {r.label}
                {r.hint && <div class="wp-dim" style={{ fontSize: "0.8em" }}>{r.hint}</div>}
              </td>
              {PRESET_COLS.map((c) => (
                <td key={c.id} class="num" data-match={ASSIST_PRESETS[c.id][r.key] === assists[r.key]}>
                  {fmt(ASSIST_PRESETS[c.id][r.key])}
                </td>
              ))}
              <td>
                {r.kind === "level" ? (
                  <select class="wp-select" value={String(assists[r.key])} aria-label={r.label} onChange={(e) => set(r.key, (e.target as HTMLSelectElement).value)}>
                    {ASSIST_LEVELS.map((l) => (
                      <option key={l} value={l}>
                        {l}
                      </option>
                    ))}
                  </select>
                ) : r.kind === "bool" ? (
                  <input type="checkbox" class="wp-toggle" checked={assists[r.key] as boolean} aria-label={r.label} onChange={(e) => set(r.key, (e.target as HTMLInputElement).checked)} />
                ) : (
                  <span class="wp-row">
                    <input type="range" class="wp-slider" min={0} max={1} step={0.05} value={assists[r.key] as number} aria-label={r.label} onInput={(e) => set(r.key, Number((e.target as HTMLInputElement).value))} />
                    <span class="wp-mono wp-dim2">{Math.round((assists[r.key] as number) * 100)}%</span>
                  </span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  );
}

// ------------------------------------------------------------------ damage mode

/** data/modes/{arcade,sim,extreme}.toml and data/economy/consequences.toml. */
const MODE_ROWS: { label: string; arcade: string; sim: string; extreme: string }[] = [
  { label: "Damage to racers (you, AI)", arcade: "20 %", sim: "100 %", extreme: "100 %" },
  { label: "Damage to traffic, police", arcade: "100 %", sim: "100 %", extreme: "100 %" },
  { label: "Crash energy into the body", arcade: "45 %", sim: "45 %", extreme: "100 %" },
  { label: "Total loss below health", arcade: "10 %", sim: "10 %", extreme: "25 %" },
  { label: "Cabin intrusion = total loss", arcade: "0.78 m", sim: "0.78 m", extreme: "0.25 m" },
  { label: "Wheels come off", arcade: "rarely", sim: "rarely", extreme: "at mount failure" },
  { label: "Component wear", arcade: "× 0.24", sim: "× 0.24", extreme: "× 1.0" },
  { label: "Repair bills", arcade: "× 0.5", sim: "× 0.5", extreme: "× 1.0" },
  { label: "Fines at capture", arcade: "× 0.6", sim: "× 1.25", extreme: "× 1.25" },
  { label: "Car seized at level 4–5", arcade: "never", sim: "30 % chance", extreme: "30 % chance" },
  { label: "Totalled car at capture", arcade: "impounded", sim: "destroyed", extreme: "destroyed" },
];

function DamageMode() {
  const mode = useSlice((s) => s.settings.damageMode);
  const send = useSend();
  const card = (m: DamageMode, title: string, blurb: string) => (
    <button type="button" class="wp-mode-card" aria-pressed={mode === m} onClick={() => send({ type: "settings.damageMode", mode: m })}>
      <span class="wp-mode-title">{title}</span>
      <span class="wp-dim2">{blurb}</span>
    </button>
  );
  return (
    <div class="wp-grid" style={{ gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1.2fr)" }}>
      <div class="wp-stack">
        {card("Arcade", "Arcade", "Forgiving. You and the AI racers take 20 % damage; traffic and police take full damage.")}
        {card("Simulation", "Simulation", "Punishing but survivable. One hard hit degrades the car; several end it.")}
        {card("Extreme", "Extreme", "Real-world-ish. One hard hit at speed is a total loss. Bills and fines hurt.")}
        <p class="wp-dim wp-mono" style={{ fontSize: "0.8em" }}>
          Every mode runs the same damage code with different tables (data/modes/*.toml). Changing mode applies from the next session; online lobbies set their own mode.
        </p>
      </div>
      <Panel title="What changes">
        <table class="wp-table">
          <thead>
            <tr>
              <th />
              <th class="num">Arcade</th>
              <th class="num">Simulation</th>
              <th class="num">Extreme</th>
            </tr>
          </thead>
          <tbody>
            {MODE_ROWS.map((r) => (
              <tr key={r.label}>
                <td>{r.label}</td>
                <td class="num" data-on={mode === "Arcade"}>
                  {r.arcade}
                </td>
                <td class="num" data-on={mode === "Simulation"}>
                  {r.sim}
                </td>
                <td class="num" data-on={mode === "Extreme"}>
                  {r.extreme}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Panel>
    </div>
  );
}

// ------------------------------------------------------------------ accessibility

const PALETTES: { id: Palette; label: string; colors: string[] }[] = [
  { id: "default", label: "Default", colors: ["#3dff8f", "#ffb21e", "#ff3d3d"] },
  { id: "deuteranopia", label: "Deuteranopia", colors: ["#38b6ff", "#ffd23f", "#ff8a00"] },
  { id: "protanopia", label: "Protanopia", colors: ["#38b6ff", "#f4f4a0", "#ffb000"] },
  { id: "tritanopia", label: "Tritanopia", colors: ["#2fe0b0", "#ff6fa8", "#ff2626"] },
  { id: "highContrast", label: "High contrast", colors: ["#00ff66", "#ffe600", "#ff1f1f"] },
];

function Accessibility() {
  const a = useSlice((s) => s.settings.accessibility);
  const units = useSlice((s) => s.settings.units);
  const send = useSend();
  const patch = (p: Partial<typeof a>) => send({ type: "settings.patch", patch: { accessibility: p } });
  return (
    <div class="wp-grid" style={{ gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)" }}>
      <Panel title="Colour">
        <div class="wp-palettes" role="radiogroup" aria-label="Colour palette">
          {PALETTES.map((p) => (
            <button type="button" class="wp-palette" aria-pressed={a.palette === p.id} onClick={() => patch({ palette: p.id })} key={p.id}>
              <span class="wp-row" style={{ gap: "0.2em" }}>
                {p.colors.map((c) => (
                  <i key={c} style={{ background: c }} />
                ))}
              </span>
              {p.label}
            </button>
          ))}
        </div>
        <p class="wp-dim" style={{ fontSize: "0.85em" }}>
          Palettes remap the ok / warning / danger colours used by damage, heat and money. Shapes and labels never rely on colour alone.
        </p>
      </Panel>
      <Panel title="Readability">
        <SliderField label="UI scale" value={a.uiScale} min={0.75} max={1.5} step={0.05} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => patch({ uiScale: v })} />
        <ToggleField label="Reduce motion" hint="No flashing light bars or blinking warnings" checked={a.reduceMotion} onChange={(v) => patch({ reduceMotion: v })} />
        <ToggleField label="Large scanner text" checked={a.largeScannerText} onChange={(v) => patch({ largeScannerText: v })} />
        <ChoiceField
          label="Units"
          value={units}
          options={[
            { id: "metric", label: "km/h" },
            { id: "imperial", label: "mph" },
          ]}
          onChange={(v) => send({ type: "settings.patch", patch: { units: v } })}
        />
      </Panel>
    </div>
  );
}

// ------------------------------------------------------------------ feedback & audio

function Feedback() {
  const f = useSlice((s) => s.settings.feedback);
  const audio = useSlice((s) => s.settings.audio);
  const send = useSend();
  return (
    <div class="wp-grid" style={{ gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)" }}>
      <Panel title="Rumble & force feedback">
        <ToggleField label="Controller rumble" checked={f.rumble} onChange={(v) => send({ type: "settings.patch", patch: { feedback: { rumble: v } } })} />
        <SliderField label="Rumble strength" value={f.rumbleStrength} onChange={(v) => send({ type: "settings.patch", patch: { feedback: { rumbleStrength: v } } })} />
        <SliderField label="Wheel force feedback" hint="Rack torque, road rumble, damper" value={f.ffbStrength} onChange={(v) => send({ type: "settings.patch", patch: { feedback: { ffbStrength: v } } })} />
      </Panel>
      <Panel title="Audio">
        {(["master", "engine", "scanner", "music"] as const).map((k) => (
          <SliderField key={k} label={k[0].toUpperCase() + k.slice(1)} value={audio[k]} onChange={(v) => send({ type: "settings.patch", patch: { audio: { [k]: v } } })} />
        ))}
      </Panel>
    </div>
  );
}
