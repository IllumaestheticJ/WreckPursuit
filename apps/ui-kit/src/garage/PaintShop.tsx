/**
 * Workshop paint shop (owner request 2026-10-25): respray the car in any colour, and fit or take
 * off vinyls, paint finishes and cosmetic parts, previewed live on the car in the bay before you
 * pay. Lives in the workshop's right column so the car stays in view.
 */
import { useEffect, useState } from "preact/hooks";
import { Button, Money, Panel, Tabs } from "../ui/kit";
import type { CarView, ModOption, ModTreeView, WorkshopScreen } from "./types";

/** What the paint shop needs from the host (workshop mode). */
export interface PaintShopApi {
  /** The career car being worked on. */
  car(): CarView;
  /** Its mod tree (the cosmetic group is listed here). */
  mods(): ModTreeView;
  /** Pay for a respray in 0xRRGGBB. */
  respray(paint: number): WorkshopScreen;
  /** Buy and fit cosmetic mod `id` (rebuilds the car). */
  install(id: string): WorkshopScreen;
  /** Take cosmetic mod `id` off (refund at resale value; rebuilds the car). */
  uninstall(id: string): WorkshopScreen;
  /** Show the car in this colour without paying (null: its own colour again). */
  previewPaint(paint: number | null): void;
  /** Show the car with these mods without paying (null: its real build again). Adds only: a
   * fitted part the car was built with stays drawn. */
  previewMods(mods: string[] | null): void;
}

/** Factory colours (wp_economy FACTORY_PAINTS) then the paint shop's extra range. */
export const PAINT_COLOURS: number[] = [
  0xd04a2a, 0x2a7fd0, 0x3fb950, 0xd0a92a, 0x9b59b6, 0x1abc9c, 0xd8d8d2, 0x8a9099, 0x1e1e22, 0x7a1f1f, 0x2b3a55, 0xe07a1f,
  0xf2f2ee, 0x0c0c0e, 0xc0161e, 0xff5a1f, 0xf5c518, 0xb6e01f, 0x0f6b3a, 0x5fd3f3, 0x123c8c, 0x6a2c91, 0xff4fa3, 0x8c6a4a,
  0xc9b48a, 0x4b5320, 0x3a3f46, 0xa7b0b8,
];

type Section = "colour" | "finish" | "vinyls" | "body" | "wheels" | "inside";
const SECTIONS: { id: Section; label: string }[] = [
  { id: "colour", label: "Colour" },
  { id: "finish", label: "Finish" },
  { id: "vinyls", label: "Vinyls" },
  { id: "body", label: "Body" },
  { id: "wheels", label: "Wheels" },
  { id: "inside", label: "Cabin & bay" },
];

/** Which paint-shop section a cosmetic mod is listed in. */
function sectionOf(m: ModOption): Section {
  if (m.slot === "Paint") return "finish";
  if (m.slot === "Vinyls") return "vinyls";
  if (m.slot === "Wheels" || m.slot === "Tires") return "wheels";
  if (m.slot === "Interior" || m.slot === "Engine") return "inside";
  return "body";
}

const hex6 = (v: number) => `#${v.toString(16).padStart(6, "0")}`;

/** Paint shop panel. `onScreen` gets the workshop screen after every purchase (cash). */
export function PaintShop(props: { api: PaintShopApi; onScreen: (s: WorkshopScreen) => void; onClose: () => void }) {
  const { api } = props;
  const [section, setSection] = useState<Section>("colour");
  const [car, setCar] = useState(() => api.car());
  const [tree, setTree] = useState(() => api.mods());
  const [pick, setPick] = useState<number | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Leaving the shop always shows the car as it really is.
  useEffect(() => () => (api.previewPaint(null), api.previewMods(null)), []);
  const refresh = () => {
    setCar(api.car());
    setTree(api.mods());
  };
  const act = (f: () => WorkshopScreen) => {
    try {
      props.onScreen(f());
      setError(null);
      setPreview(null);
      refresh();
    } catch (e) {
      setError(String((e as Error).message ?? e).replace(/^(Error: )?wp_wasm: /, ""));
    }
  };
  const choose = (c: number) => {
    setPick(c);
    api.previewPaint(c === car.paint ? null : c);
  };
  const installed = tree.mods.filter((m) => m.installed).map((m) => m.id);
  const nameOf = (id: string) => tree.mods.find((m) => m.id === id)?.name ?? id;
  const showMod = (m: ModOption) => {
    if (preview === m.id) {
      setPreview(null);
      api.previewMods(null);
      return;
    }
    setPreview(m.id);
    // The build with this mod (it takes the place of what it replaces).
    const mods = [...installed.filter((id) => id !== m.replaces), m.id];
    api.previewMods(mods);
  };
  const rows = tree.mods.filter((m) => m.group === "cosmetic" && sectionOf(m) === section).sort((a, b) => a.slot.localeCompare(b.slot) || a.tier - b.tier);
  return (
    <Panel
      title="Paint shop"
      tag={
        <Button size="sm" onClick={props.onClose}>
          Close
        </Button>
      }
    >
      <div data-testid="paint-shop" class="wp-ps">
        <Tabs tabs={SECTIONS} value={section} onChange={(s) => (setSection(s), setPreview(null), api.previewMods(null))} />
        {error && (
          <p class="wp-danger" data-testid="ps-error">
            {error}
          </p>
        )}
        {section === "colour" ? (
          <div class="wp-ps-colour">
            <div class="wp-ps-swatches" role="listbox" aria-label="Colours">
              {PAINT_COLOURS.map((c) => (
                <button
                  key={c}
                  type="button"
                  role="option"
                  aria-selected={(pick ?? car.paint) === c}
                  class={`wp-ps-swatch${(pick ?? car.paint) === c ? " is-picked" : ""}${car.paint === c ? " is-current" : ""}`}
                  style={{ background: hex6(c) }}
                  title={car.paint === c ? `${hex6(c)} (current)` : hex6(c)}
                  data-paint={hex6(c)}
                  onClick={() => choose(c)}
                />
              ))}
            </div>
            <label class="wp-row wp-ps-custom" style={{ gap: "0.5em" }}>
              <span class="wp-dim">Custom</span>
              <input
                type="color"
                data-testid="ps-custom"
                value={hex6(pick ?? car.paint)}
                onInput={(e) => choose(parseInt((e.target as HTMLInputElement).value.slice(1), 16))}
              />
              <span class="wp-mono wp-dim">{hex6(pick ?? car.paint)}</span>
            </label>
            <div class="wp-row" style={{ gap: "0.5em", marginTop: "0.6em" }}>
              <Button
                variant="primary"
                testid="ps-respray"
                disabled={pick === null || pick === car.paint}
                onClick={() => pick !== null && act(() => api.respray(pick))}
              >
                Respray · <Money value={car.respray_fee} />
              </Button>
              {pick !== null && pick !== car.paint && (
                <Button size="sm" onClick={() => (setPick(null), api.previewPaint(null))}>
                  Keep current
                </Button>
              )}
            </div>
            <p class="wp-dim" style={{ fontSize: "0.85em" }}>
              Pick a colour to see it on the car. Nothing is charged until you respray.
            </p>
          </div>
        ) : (
          <div class="wp-ps-list">
            {rows.length === 0 && <p class="wp-dim">Nothing for this car here.</p>}
            {rows.map((m) => (
              <div key={m.id} class={`wp-ps-row${preview === m.id ? " is-preview" : ""}`} data-mod={m.id}>
                <div>
                  <b>{m.name}</b> <span class="wp-dim wp-mono">T{m.tier}</span>
                  {m.installed && <span class="wp-ok"> · fitted</span>}
                  <div class="wp-dim" style={{ fontSize: "0.8em" }}>
                    {m.replaces ? `Replaces ${nameOf(m.replaces)}. ` : ""}
                    {m.tradeoff}
                  </div>
                  {!m.allowed && m.reason && (
                    <div class="wp-warn" style={{ fontSize: "0.8em" }}>
                      {m.reason}
                    </div>
                  )}
                </div>
                <div class="wp-row" style={{ gap: "0.3em", justifyContent: "flex-end" }}>
                  {!m.installed && (
                    <Button size="sm" pressed={preview === m.id} onClick={() => showMod(m)} testid={`ps-preview-${m.id}`}>
                      {preview === m.id ? "Previewing" : "Preview"}
                    </Button>
                  )}
                  {m.installed ? (
                    <Button size="sm" disabled={!m.allowed} onClick={() => act(() => api.uninstall(m.id))}>
                      Remove
                    </Button>
                  ) : (
                    <Button size="sm" variant="primary" disabled={!m.allowed} onClick={() => act(() => api.install(m.id))} testid={`ps-buy-${m.id}`}>
                      {m.replaces ? "Swap" : "Fit"} · <Money value={m.price} />
                    </Button>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </Panel>
  );
}
