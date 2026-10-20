/**
 * Shared UI primitives. Small on purpose: screens compose these and the CSS classes in
 * theme.css. No component holds game state.
 */
import type { ComponentChildren, JSX } from "preact";
import { useEffect, useRef } from "preact/hooks";
import { useSend, useSlice } from "../model/GameModel";
import type { ScreenId } from "../model/types";

/** Bordered panel with an optional title row. */
export function Panel(props: { title?: string; tag?: ComponentChildren; class?: string; style?: JSX.CSSProperties; children?: ComponentChildren }) {
  return (
    <section class={`wp-panel ${props.class ?? ""}`} style={props.style}>
      {props.title && (
        <div class="wp-panel-title">
          <span>{props.title}</span>
          {props.tag !== undefined && <b>{props.tag}</b>}
        </div>
      )}
      {props.children}
    </section>
  );
}

/** Button. `variant` picks the style; `pressed` renders a toggle. */
export function Button(props: {
  onClick?: () => void;
  variant?: "primary" | "danger" | "ghost";
  size?: "sm";
  disabled?: boolean;
  pressed?: boolean;
  title?: string;
  hotkey?: string;
  children?: ComponentChildren;
}) {
  const cls = ["wp-btn", props.variant && `wp-btn--${props.variant}`, props.size && `wp-btn--${props.size}`].filter(Boolean).join(" ");
  return (
    <button type="button" class={cls} onClick={props.onClick} disabled={props.disabled} aria-pressed={props.pressed} title={props.title}>
      {props.children}
      {props.hotkey && <span class="wp-kbd">{props.hotkey}</span>}
    </button>
  );
}

/** Small status label. */
export function Badge(props: { tone?: "ok" | "warn" | "danger" | "info" | "dim"; solid?: boolean; children?: ComponentChildren; title?: string }) {
  return (
    <span class={`wp-badge wp-badge--${props.tone ?? "dim"} ${props.solid ? "wp-badge--solid" : ""}`} title={props.title}>
      {props.children}
    </span>
  );
}

/** Horizontal bar 0..=1. `color` is any CSS colour (defaults to the accent). */
export function Meter(props: { value: number; color?: string; segmented?: boolean; label?: string }) {
  const v = Math.max(0, Math.min(1, props.value));
  return (
    <div
      class={`wp-meter ${props.segmented ? "wp-meter--seg" : ""}`}
      role="meter"
      aria-valuemin={0}
      aria-valuemax={1}
      aria-valuenow={v}
      aria-label={props.label}
      style={{ "--meter": props.color } as JSX.CSSProperties}
    >
      <i style={{ width: `${v * 100}%` }} />
    </div>
  );
}

/**
 * Colour for a damage/health level, palette-aware: severity 0 → ok, 0.5 → warn, 1 → danger.
 * Uses CSS `color-mix` so colour-blind palettes apply automatically.
 */
export function severityColor(severity: number): string {
  const s = Math.max(0, Math.min(1, severity));
  if (s <= 0.5) return `color-mix(in oklab, var(--warn) ${Math.round(s * 200)}%, var(--ok))`;
  return `color-mix(in oklab, var(--danger) ${Math.round((s - 0.5) * 200)}%, var(--warn))`;
}

/** Labelled value. */
export function Stat(props: { label: string; value: ComponentChildren; unit?: string; tone?: "ok" | "warn" | "danger" | "cash" }) {
  return (
    <div>
      <div class="wp-dim wp-mono wp-upper" style={{ fontSize: "0.68em", letterSpacing: "0.14em" }}>
        {props.label}
      </div>
      <div class={`wp-mono ${props.tone ? `wp-${props.tone}` : ""}`} style={{ fontSize: "1.25em" }}>
        {props.value}
        {props.unit && <span class="wp-dim" style={{ fontSize: "0.7em", marginLeft: "0.25em" }}>{props.unit}</span>}
      </div>
    </div>
  );
}

/** Money with the in-game currency sign; `signed` shows +/−. */
export function Money(props: { value: number; signed?: boolean; tone?: boolean }) {
  const v = Math.round(props.value);
  const sign = props.signed ? (v > 0 ? "+" : v < 0 ? "−" : "±") : v < 0 ? "−" : "";
  const cls = props.tone ? (v > 0 ? "wp-cash" : v < 0 ? "wp-danger" : "wp-dim") : "";
  return <span class={`wp-mono ${cls}`}>{`${sign}$${Math.abs(v).toLocaleString("en-US")}`}</span>;
}

/**
 * Before → after with a coloured change. `higherIsBetter` decides the colour; `format` renders
 * both numbers.
 */
export function Delta(props: { before: number; after: number; format: (v: number) => string; higherIsBetter?: boolean; epsilon?: number }) {
  const d = props.after - props.before;
  const eps = props.epsilon ?? 1e-6;
  const better = props.higherIsBetter ?? true ? d > eps : d < -eps;
  const worse = props.higherIsBetter ?? true ? d < -eps : d > eps;
  return (
    <span class="wp-delta">
      <span class="wp-dim">{props.format(props.before)}</span>
      {Math.abs(d) > eps && (
        <>
          {" → "}
          <span class={better ? "better" : worse ? "worse" : ""}>{props.format(props.after)}</span>
        </>
      )}
    </span>
  );
}

/** Tab strip (controlled). */
export function Tabs<T extends string>(props: { tabs: { id: T; label: string }[]; value: T; onChange: (v: T) => void }) {
  return (
    <div class="wp-tabs" role="tablist">
      {props.tabs.map((t) => (
        <button type="button" role="tab" class="wp-tab" aria-selected={t.id === props.value} onClick={() => props.onChange(t.id)} key={t.id}>
          {t.label}
        </button>
      ))}
    </div>
  );
}

/** On/off switch row. */
export function ToggleField(props: { label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div class="wp-field">
      <label>
        {props.label}
        {props.hint && <small>{props.hint}</small>}
      </label>
      <input type="checkbox" class="wp-toggle" checked={props.checked} aria-label={props.label} onChange={(e) => props.onChange((e.target as HTMLInputElement).checked)} />
    </div>
  );
}

/** Slider row (0..=1 by default). */
export function SliderField(props: { label: string; hint?: string; value: number; min?: number; max?: number; step?: number; format?: (v: number) => string; onChange: (v: number) => void }) {
  const fmt = props.format ?? ((v: number) => `${Math.round(v * 100)}%`);
  return (
    <div class="wp-field">
      <label>
        {props.label}
        {props.hint && <small>{props.hint}</small>}
      </label>
      <span class="wp-row">
        <input
          type="range"
          class="wp-slider"
          min={props.min ?? 0}
          max={props.max ?? 1}
          step={props.step ?? 0.05}
          value={props.value}
          aria-label={props.label}
          onInput={(e) => props.onChange(Number((e.target as HTMLInputElement).value))}
        />
        <span class="wp-mono wp-dim2" style={{ minWidth: "3.5em", textAlign: "right" }}>{fmt(props.value)}</span>
      </span>
    </div>
  );
}

/** Choice row with segmented buttons. */
export function ChoiceField<T extends string>(props: { label: string; hint?: string; value: T; options: { id: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <div class="wp-field">
      <label>
        {props.label}
        {props.hint && <small>{props.hint}</small>}
      </label>
      <span class="wp-row" role="radiogroup" aria-label={props.label}>
        {props.options.map((o) => (
          <Button size="sm" pressed={o.id === props.value} onClick={() => props.onChange(o.id)} key={o.id}>
            {o.label}
          </Button>
        ))}
      </span>
    </div>
  );
}

/** Modal dialog; Escape closes. */
export function Modal(props: { title: string; onClose: () => void; children?: ComponentChildren }) {
  const onClose = useRef(props.onClose);
  onClose.current = props.onClose;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div class="wp-modal-back" onClick={(e) => e.target === e.currentTarget && props.onClose()}>
      <Panel title={props.title} class="wp-modal">
        <div role="dialog" aria-label={props.title}>
          {props.children}
        </div>
      </Panel>
    </div>
  );
}

/** Standard screen header: kicker, title, right-side account readout, back button. */
export function ScreenHead(props: { kicker: string; title: string; back?: ScreenId | "back"; children?: ComponentChildren }) {
  const send = useSend();
  const account = useSlice((s) => s.account);
  return (
    <header class="wp-head">
      <h1>
        <small>{props.kicker}</small>
        {props.title}
      </h1>
      <div class="wp-head-right">
        {props.children}
        <span>
          CASH <Money value={account.cash} tone />
        </span>
        <span>
          REP <span class="wp-ok">{account.rep.toLocaleString("en-US")}</span>
        </span>
        <span>DAY {account.day}</span>
        {props.back && (
          <Button size="sm" onClick={() => send(props.back === "back" ? { type: "back" } : { type: "navigate", screen: props.back as ScreenId })} hotkey="Esc">
            Back
          </Button>
        )}
      </div>
    </header>
  );
}

/** Escape key → handler (for screens with a back action). */
export function useEscape(handler: () => void): void {
  // Latest handler via a ref, so the listener is attached once (screens re-render every frame).
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !document.querySelector(".wp-modal-back")) ref.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

/** Car stats as `CarStatBars` reads them (`CarStats` in `data/catalog.ts`). */
type StatSource = {
  zero_to_100_s: number | null;
  top_speed_kmh: number;
  cornering_g: number;
  bars: { acceleration: number; top_speed: number; cornering: number };
};

const STAT_ROWS = [
  { key: "acceleration", label: "Accel", value: (s: StatSource) => (s.zero_to_100_s === null ? "—" : `${s.zero_to_100_s.toFixed(1)} s`), hint: "0–100 km/h" },
  { key: "top_speed", label: "Top speed", value: (s: StatSource) => `${Math.round(s.top_speed_kmh)} km/h`, hint: "" },
  { key: "cornering", label: "Cornering", value: (s: StatSource) => `${s.cornering_g.toFixed(2)} g`, hint: "50 m skidpad" },
] as const;

/** One-line stat summary (tooltips, screen readers). */
export function statSummary(s: StatSource): string {
  return STAT_ROWS.map((r) => `${r.label} ${r.value(s)}${r.hint ? ` (${r.hint})` : ""}`).join(" · ");
}

/**
 * Acceleration / top speed / cornering bars, physics-measured. `compact`: three unlabelled bars
 * in that order (the table header names them), numbers in the tooltip.
 */
export function CarStatBars(props: { stats: StatSource; compact?: boolean }) {
  const s = props.stats;
  return (
    <div class={`wp-statbars ${props.compact ? "wp-statbars--compact" : ""}`} title={statSummary(s)}>
      {STAT_ROWS.map((r) => (
        <div class="wp-statbar" key={r.key} data-stat={r.key}>
          {!props.compact && <span class="wp-dim wp-mono wp-upper">{r.label}</span>}
          <Meter value={s.bars[r.key]} segmented label={`${r.label}: ${r.value(s)}`} />
          {!props.compact && <span class="wp-mono">{r.value(s)}</span>}
        </div>
      ))}
    </div>
  );
}
