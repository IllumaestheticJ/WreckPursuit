// Free car tune outside the career (G10): the developer panel's "car tune" row (four 1..5
// sliders for the car in the car selector, for A/B work) and the hook that puts the stored
// tune on the player entity of a quick run (free drive, city chase, race). Career runs use the
// tune saved with the career car instead (wp_wasm `career_start_run`).
//
// Tunes are stored per viewer and per car id (ui-kit `garage/tune.ts`, localStorage).

import type { CarTune } from "../../ui-kit/src/garage/types";
import { isStockTune, quickTune, saveQuickTune, STOCK_TUNE, TUNE_SLIDERS, tuneSummary } from "../../ui-kit/src/garage/tune";

/** `json` (simcli scenario JSON) with the viewer's stored tune for the first entity's car. A
 * stock tune leaves the scenario untouched. */
export function withQuickTune(json: string): string {
  const sc = JSON.parse(json) as { entities?: { car: string; tune?: CarTune }[] };
  const first = sc.entities?.[0];
  if (!first) return json;
  const tune = quickTune(first.car);
  if (isStockTune(tune)) return json;
  first.tune = tune;
  return JSON.stringify(sc);
}

/** Wire the "car tune" row: `row` gets one slider per tune slider plus a reset button and a
 * summary; it follows the car selector `carSel`. Changes are saved at once and apply from the
 * next start. */
export function mountTuneRow(row: HTMLElement, carSel: HTMLSelectElement): void {
  const inputs = new Map<keyof CarTune, HTMLInputElement>();
  const values = new Map<keyof CarTune, HTMLElement>();
  const summary = document.createElement("span");
  summary.className = "dim";
  const reset = document.createElement("button");
  reset.textContent = "stock";
  reset.title = "every slider back to 3 (stock)";

  const current = (): CarTune => {
    const t = { ...STOCK_TUNE };
    for (const [k, el] of inputs) t[k] = Number(el.value);
    return t;
  };
  const show = (t: CarTune) => {
    for (const s of TUNE_SLIDERS) {
      inputs.get(s.key)!.value = String(t[s.key]);
      values.get(s.key)!.textContent = s.steps[t[s.key] - 1];
    }
    summary.textContent = `${tuneSummary(t)} · from the next start`;
    reset.disabled = isStockTune(t);
  };

  for (const s of TUNE_SLIDERS) {
    const label = document.createElement("label");
    label.title = `${s.label}: 1 ${s.low.toLowerCase()} … 5 ${s.high.toLowerCase()}, 3 = stock. ${s.hint}`;
    label.append(`${s.label.toLowerCase()} `);
    const input = document.createElement("input");
    input.type = "range";
    input.min = "1";
    input.max = "5";
    input.step = "1";
    input.style.width = "60px";
    input.dataset.tune = s.key;
    input.oninput = () => {
      saveQuickTune(carSel.value, current());
      show(current());
    };
    const value = document.createElement("span");
    value.className = "dim";
    label.append(input, " ", value);
    inputs.set(s.key, input);
    values.set(s.key, value);
    row.append(label);
  }
  reset.onclick = () => {
    saveQuickTune(carSel.value, { ...STOCK_TUNE });
    show({ ...STOCK_TUNE });
  };
  row.append(reset, summary);
  carSel.addEventListener("change", () => show(quickTune(carSel.value)));
  show(quickTune(carSel.value));
}
