/**
 * Repair bill for the car being driven, as an overlay panel: shop / district / option /
 * insurance / wanted controls, the itemised quote, insurance, what the player pays, salvage.
 *
 * Data: `UiState.bill`. In the testbed it is live (`wp_damage` estimate priced by
 * `wp_economy::Economy::quote_repair` through `wp_wasm::repair_quote_json`); on the demo page it
 * comes from the mock garage. Changing a control sends `bill.request`; Close sends `back`.
 */
import { useSend, useSlice } from "../../model/GameModel";
import type { BillRequest } from "../../model/types";
import { Badge, Button, ChoiceField, Money, Panel, severityColor, useEscape } from "../../ui/kit";
import { pct } from "../../ui/format";
import "./garage.css";

const DISTRICTS = ["default", "Core", "Suburb", "Industrial", "Docks", "Hills"];
const WANTED: BillRequest["wanted"][] = ["None", "L1", "L2", "L3", "L4", "L5"];

/** Repair-bill overlay screen. */
export function RepairBill() {
  const send = useSend();
  const bill = useSlice((s) => s.bill);
  useEscape(() => send({ type: "back" }));
  const ask = (r: Partial<BillRequest>) => send({ type: "bill.request", request: r });
  if (!bill) {
    return (
      <div class="wp-bill-overlay">
        <Panel title="Repair bill">
          <p class="wp-dim">No car to price yet.</p>
          <Button onClick={() => ask({})}>Price my car</Button>
        </Panel>
      </div>
    );
  }
  const r = bill.request;
  return (
    <div class="wp-bill-overlay" role="dialog" aria-label="Repair bill">
      <Panel title={`Repair bill · ${bill.carName}`} tag={`${bill.mode.toUpperCase()} · ${bill.source === "wp_economy" ? "LIVE" : "MOCK"}`} class="wp-bill-panel">
        <div class="wp-spread" style={{ marginBottom: "0.4em" }}>
          <span class="wp-mono">
            <span class="wp-dim">CONDITION </span>
            <span style={{ color: severityColor(1 - bill.health) }}>{pct(bill.health)}</span>
            {bill.totalLoss && (
              <>
                {" "}
                <Badge tone="danger" solid>
                  Total loss
                </Badge>
              </>
            )}
          </span>
          <span class="wp-row">
            <span class="wp-mono wp-dim">{bill.carClass}</span>
            <Button size="sm" onClick={() => ask({})}>
              Refresh
            </Button>
            <Button size="sm" onClick={() => send({ type: "back" })} hotkey="Esc">
              Close
            </Button>
          </span>
        </div>
        <ChoiceField<BillRequest["option"]>
          label="Decision"
          value={r.option}
          options={[
            { id: "RepairAll", label: "Repair all" },
            { id: "DriveDamaged", label: "Drive damaged" },
            { id: "Scrap", label: "Scrap" },
          ]}
          onChange={(option) => ask({ option })}
        />
        <ChoiceField<BillRequest["shop"]>
          label="Shop"
          hint={r.shop === "Crooked" ? "Marked-up labour, never reports" : "Normal rates, reports wanted cars (L2+)"}
          value={r.shop}
          options={[
            { id: "Legit", label: "Legit" },
            { id: "Crooked", label: "Crooked" },
          ]}
          onChange={(shop) => ask({ shop })}
        />
        <div class="wp-field">
          <label>District · wanted · insurance</label>
          <span class="wp-row">
            <select class="wp-select" value={r.district} aria-label="District" onChange={(e) => ask({ district: (e.target as HTMLSelectElement).value })}>
              {DISTRICTS.map((d) => (
                <option key={d} value={d}>
                  {d}
                </option>
              ))}
            </select>
            <select class="wp-select" value={r.wanted} aria-label="Wanted level" onChange={(e) => ask({ wanted: (e.target as HTMLSelectElement).value as BillRequest["wanted"] })}>
              {WANTED.map((w) => (
                <option key={w} value={w}>
                  {w === "None" ? "not wanted" : `wanted ${w}`}
                </option>
              ))}
            </select>
            <select
              class="wp-select"
              value={r.insurance ?? "none"}
              aria-label="Insurance"
              onChange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                ask({ insurance: v === "none" ? null : (v as "basic" | "full") });
              }}
            >
              <option value="none">uninsured</option>
              <option value="basic">basic</option>
              <option value="full">full</option>
            </select>
          </span>
        </div>
        {bill.error ? (
          <p class="wp-dim" style={{ margin: "0.8em 0" }}>
            {bill.error === "nothing to repair" || /nothing to repair/i.test(bill.error) ? "Nothing to repair. Clean car." : bill.error}
          </p>
        ) : (
          <>
            {r.option === "RepairAll" && (
              <table class="wp-table wp-bill">
                <thead>
                  <tr>
                    <th>Item</th>
                    <th class="num">Damage</th>
                    <th class="num">Parts</th>
                    <th class="num">Labour</th>
                    <th class="num">Total</th>
                  </tr>
                </thead>
                <tbody>
                  {bill.lines.map((l, i) => (
                    <tr key={i}>
                      <td>{l.label}</td>
                      <td class="num" style={{ color: severityColor(l.severity) }}>
                        {pct(l.severity)}
                      </td>
                      <td class="num">
                        <Money value={l.parts} />
                      </td>
                      <td class="num">
                        <Money value={l.labour} />
                      </td>
                      <td class="num">
                        <Money value={l.total} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            <div class="wp-bill-foot">
              {r.option === "RepairAll" && (
                <>
                  <div class="wp-spread">
                    <span class="wp-dim2">
                      Subtotal ({bill.lines.length} items, labour ×{bill.labourMultiplier.toFixed(2)})
                    </span>
                    <Money value={bill.gross} />
                  </div>
                  {r.insurance && (
                    <div class="wp-spread">
                      <span class="wp-dim2">Insurance ({r.insurance})</span>
                      {bill.claimDenied ? <Badge tone="danger">Denied: damage while wanted</Badge> : <Money value={-bill.insuranceCovers} signed tone />}
                    </div>
                  )}
                </>
              )}
              {r.option === "DriveDamaged" && <p class="wp-dim2" style={{ margin: 0 }}>Free. The car keeps its damage and drives like it looks.</p>}
              <div class="wp-spread wp-bill-total">
                <span>{r.option === "Scrap" ? "You receive" : "You pay"}</span>
                <Money value={r.option === "Scrap" ? bill.salvage : bill.playerPays} tone={r.option === "Scrap"} />
              </div>
              {bill.reportsToPolice && <div class="wp-warn">This shop will report the car to the police.</div>}
            </div>
          </>
        )}
      </Panel>
    </div>
  );
}
