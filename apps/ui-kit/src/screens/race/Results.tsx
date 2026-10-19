/** Post-race results and payout breakdown (lines become typed `EconomyEvent`s in v0.2.0). */
import { useSend, useSlice } from "../../model/GameModel";
import { Badge, Button, Money, Panel, ScreenHead } from "../../ui/kit";
import { raceTime } from "../../ui/format";
import "./race.css";

/** Results screen. */
export function Results() {
  const send = useSend();
  const r = useSlice((s) => s.results);
  const winner = r.rows[0]?.timeS ?? null;
  const totals = r.payout.reduce((a, l) => ({ cash: a.cash + l.cash, rep: a.rep + l.rep, bounty: a.bounty + l.bounty }), { cash: 0, rep: 0, bounty: 0 });
  return (
    <div class="wp-screen">
      <ScreenHead kicker="// RESULTS" title={r.eventName}>
        <Button variant="primary" onClick={() => send({ type: "results.continue" })}>
          Continue to garage
        </Button>
      </ScreenHead>
      <div class="wp-results">
        <Panel title="Classification">
          <table class="wp-table wp-results-table">
            <thead>
              <tr>
                <th>Pos</th>
                <th>Driver</th>
                <th>Car</th>
                <th class="num">Time</th>
                <th class="num">Gap</th>
              </tr>
            </thead>
            <tbody>
              {r.rows.map((row) => (
                <tr key={row.position} class={row.isLocal ? "is-local" : ""}>
                  <td class="wp-results-pos">{row.status === "Finished" ? row.position : "—"}</td>
                  <td>
                    <b>{row.name}</b>
                  </td>
                  <td>{row.car}</td>
                  <td class="num">{row.status === "Finished" ? raceTime(row.timeS) : <Badge tone="danger">{row.status}</Badge>}</td>
                  <td class="num wp-dim">{row.timeS !== null && winner !== null && row.position > 1 ? `+${(row.timeS - winner).toFixed(3)}` : ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
        <Panel title="Payout" tag="SETTLED">
          <table class="wp-table">
            <thead>
              <tr>
                <th>Line</th>
                <th class="num">Cash</th>
                <th class="num">Rep</th>
                <th class="num">Bounty</th>
              </tr>
            </thead>
            <tbody>
              {r.payout.map((l) => (
                <tr key={l.label}>
                  <td>{l.label}</td>
                  <td class="num">{l.cash ? <Money value={l.cash} signed tone /> : <span class="wp-dim">—</span>}</td>
                  <td class="num">{l.rep ? <span class="wp-ok">+{l.rep}</span> : <span class="wp-dim">—</span>}</td>
                  <td class="num">{l.bounty ? <span class="wp-warn">+{l.bounty.toLocaleString("en-US")}</span> : <span class="wp-dim">—</span>}</td>
                </tr>
              ))}
              <tr class="wp-results-total">
                <td>Net</td>
                <td class="num">
                  <Money value={totals.cash} signed tone />
                </td>
                <td class="num wp-ok">+{totals.rep}</td>
                <td class="num wp-warn">+{totals.bounty.toLocaleString("en-US")}</td>
              </tr>
            </tbody>
          </table>
          <div class="wp-stack" style={{ marginTop: "1em" }}>
            <div class="wp-spread">
              <span class="wp-dim2">Cash</span>
              <span>
                <Money value={r.cashBefore} /> → <Money value={r.cashAfter} tone />
              </span>
            </div>
            <div class="wp-spread">
              <span class="wp-dim2">Damage to repair (estimate)</span>
              <Money value={-r.damageEstimate} signed tone />
            </div>
            <div class="wp-spread wp-bill-total">
              <span>After repairs</span>
              <Money value={totals.cash - r.damageEstimate} signed tone />
            </div>
            <p class="wp-dim" style={{ fontSize: "0.8em", margin: 0 }}>
              Bounty grows when you win while wanted: more money if you stay free, more heat on the street.
            </p>
          </div>
        </Panel>
      </div>
    </div>
  );
}
