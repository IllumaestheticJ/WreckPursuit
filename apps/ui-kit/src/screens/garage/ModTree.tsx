/**
 * Mod tree: slots → upgrade lines → tiers, with requirements, class/mode limits and the
 * before/after effect of the selected mod on performance, durability and the class index.
 * Mod data is the real `data/mods/*.json`; numbers come from the estimator port
 * (`data/estimate.ts`, mirror of `wp_economy::Economy::preview`).
 */
import type { ComponentChildren } from "preact";
import { useMemo, useState } from "preact/hooks";
import { useSend, useSlice } from "../../model/GameModel";
import { CARS, MODS, modBlocker, modTree, slotLabel, type ModDef } from "../../data/catalog";
import { previewBuild, type ModPreview } from "../../data/estimate";
import { buildStats } from "../../data/stats";
import { Badge, Button, CarStatBars, Delta, Money, Panel, ScreenHead, useEscape } from "../../ui/kit";
import { words } from "../../ui/format";
import "./garage.css";

/** Mod tree screen (for the active car). */
export function ModTree() {
  const send = useSend();
  const cars = useSlice((s) => s.garage.cars);
  const activeUid = useSlice((s) => s.garage.activeCar);
  const mode = useSlice((s) => s.settings.damageMode);
  const cash = useSlice((s) => s.account.cash);
  const [carUid, setCarUid] = useState<number>(activeUid ?? cars[0]?.uid ?? 0);
  const car = cars.find((c) => c.uid === carUid) ?? cars[0];
  const tree = useMemo(() => modTree(), []);
  const [slot, setSlot] = useState(tree[0].slot);
  const [sel, setSel] = useState<string>(tree[0].lines[0].mods[0].id);
  useEscape(() => send({ type: "navigate", screen: "garage" }));
  if (!car) return <div class="wp-screen">No car.</div>;
  const def = CARS[car.defId];
  const mod = MODS[sel];
  const installed = car.mods.includes(sel);
  // Candidate build: the selected mod replaces whatever is on the same line.
  const candidate = installed
    ? car.mods.filter((m) => m !== sel)
    : [...car.mods.filter((m) => !(MODS[m]?.line === mod.line && MODS[m]?.slot === mod.slot)), sel];
  const before = previewBuild(def, car.mods);
  const after = previewBuild(def, candidate);
  // Stat bars: the build now (provider-measured if available) vs the candidate build.
  const statsNow = buildStats(car.defId, car.mods, car.stats);
  const statsAfter = buildStats(car.defId, candidate);
  const slotEntry = tree.find((t) => t.slot === slot) ?? tree[0];
  const blocker = installed ? null : modBlocker(mod, car.mods, car.class, mode);
  const replaced = car.mods.find((m) => MODS[m]?.line === mod.line && MODS[m]?.slot === mod.slot && m !== sel);
  const cost = installed ? -Math.round(mod.price * 0.4) : mod.price - (replaced ? Math.round(MODS[replaced].price * 0.4) : 0);

  return (
    <div class="wp-screen">
      <ScreenHead kicker="// GARAGE · BUILD" title="Mod tree" back="garage">
        <label class="wp-row">
          <span class="wp-dim">CAR</span>
          <select class="wp-select" value={car.uid} onChange={(e) => setCarUid(Number((e.target as HTMLSelectElement).value))}>
            {cars
              .filter((c) => c.location.kind === "Garage")
              .map((c) => (
                <option key={c.uid} value={c.uid}>
                  {c.name}
                </option>
              ))}
          </select>
        </label>
        <ClassBadge p={before} />
      </ScreenHead>
      <div class="wp-modtree">
        <nav class="wp-mod-slots" aria-label="Mod slots">
          {tree.map((t) => {
            const count = car.mods.filter((m) => MODS[m]?.slot === t.slot).length;
            return (
              <button type="button" key={t.slot} class="wp-mod-slot" aria-pressed={t.slot === slot} onClick={() => setSlot(t.slot)}>
                <span>{slotLabel(t.slot)}</span>
                {count > 0 && <span class="wp-ok wp-mono">{count}</span>}
              </button>
            );
          })}
        </nav>
        <Panel title={slotLabel(slot)} tag={`${slotEntry.lines.length} LINES`} class="wp-mod-lines">
          {slotEntry.lines.map(({ line, mods }) => (
            <div class="wp-mod-line" key={line}>
              <div class="wp-mod-line-name wp-mono wp-upper">{words(line.replace(/_/g, " "))}</div>
              <div class="wp-mod-nodes">
                {mods.map((m, i) => (
                  <ModNode key={m.id} mod={m} first={i === 0} state={nodeState(m, car.mods, car.class, mode)} selected={m.id === sel} onClick={() => setSel(m.id)} />
                ))}
              </div>
            </div>
          ))}
        </Panel>
        <Panel title={`Tier ${mod.tier} · ${slotLabel(mod.slot)}`} tag={installed ? "INSTALLED" : blocker ? "LOCKED" : "AVAILABLE"} class="wp-mod-detail">
          <h2 class="wp-mod-title">{mod.name}</h2>
          <div class="wp-spread">
            <Money value={mod.price} />
            {replaced && !installed && <span class="wp-dim wp-mono" style={{ fontSize: "0.8em" }}>replaces {MODS[replaced].name} (+40 % back)</span>}
          </div>
          <p class="wp-mod-tradeoff">{mod.tradeoff}</p>
          {(mod.requires?.length ?? 0) > 0 && (
            <div class="wp-row" style={{ gap: "0.3em" }}>
              <span class="wp-dim wp-mono" style={{ fontSize: "0.75em" }}>REQUIRES</span>
              {mod.requires!.map((r) => (
                <Badge key={r} tone={modBlocker({ ...mod, requires: [r] }, car.mods, car.class, mode) ? "danger" : "ok"}>
                  {MODS[r]?.name ?? r}
                </Badge>
              ))}
            </div>
          )}
          {mod.modes?.length ? <Badge tone="warn">{mod.modes.join(" / ")} only</Badge> : null}
          {mod.class_whitelist?.length ? <Badge tone="info">{mod.class_whitelist.join(", ")}</Badge> : null}
          {statsNow && statsAfter && (
            <div class="wp-garage-stats" style={{ margin: "0.6em 0" }}>
              <div class="wp-dim wp-mono wp-upper" style={{ fontSize: "0.68em" }}>
                {installed ? "Without this mod" : "With this mod"} · tick = current build
              </div>
              <CarStatBars stats={statsAfter.stats} baseline={statsNow.stats} />
            </div>
          )}
          <DeltaTable before={before} after={after} />
          <div class="wp-row" style={{ justifyContent: "space-between", marginTop: "0.8em" }}>
            {blocker ? <span class="wp-danger wp-mono" style={{ fontSize: "0.85em" }}>{blocker}</span> : <span />}
            {installed ? (
              <Button onClick={() => send({ type: "mods.uninstall", car: car.uid, mod: sel })}>
                Remove (<Money value={-cost} signed />)
              </Button>
            ) : (
              <Button variant="primary" disabled={!!blocker || cost > cash} onClick={() => send({ type: "mods.install", car: car.uid, mod: sel })}>
                Install · ${cost.toLocaleString("en-US")}
              </Button>
            )}
          </div>
        </Panel>
      </div>
    </div>
  );
}

type NodeState = "installed" | "superseded" | "available" | "locked";

function nodeState(m: ModDef, installed: string[], cls: Parameters<typeof modBlocker>[2], mode: Parameters<typeof modBlocker>[3]): NodeState {
  if (installed.includes(m.id)) return "installed";
  if (installed.some((id) => MODS[id]?.line === m.line && MODS[id]?.slot === m.slot && MODS[id].tier > m.tier)) return "superseded";
  return modBlocker(m, installed, cls, mode) ? "locked" : "available";
}

function ModNode(props: { mod: ModDef; state: NodeState; selected: boolean; first: boolean; onClick: () => void }) {
  return (
    <>
      {!props.first && <span class="wp-mod-link" data-state={props.state} aria-hidden="true" />}
      <button type="button" class="wp-mod-node" data-state={props.state} aria-pressed={props.selected} onClick={props.onClick} title={props.mod.tradeoff}>
        <span class="wp-mod-tier wp-mono">T{props.mod.tier}</span>
        <span class="wp-mod-node-name">{props.mod.name}</span>
        <span class="wp-mono wp-dim" style={{ fontSize: "0.75em" }}>
          {props.state === "locked" ? "🔒 " : ""}${props.mod.price.toLocaleString("en-US")}
        </span>
      </button>
    </>
  );
}

function ClassBadge({ p }: { p: ModPreview }) {
  return (
    <span class="wp-class-badge">
      <b>{p.bracket}</b>
      <span class="wp-mono">{p.classIndex}</span>
    </span>
  );
}

const f2 = (v: number) => v.toFixed(2);
const f0 = (v: number) => v.toFixed(0);

function DeltaTable({ before, after }: { before: ModPreview; after: ModPreview }) {
  const b = before.perf;
  const a = after.perf;
  const durKeys = [...new Set([...Object.keys(before.durability), ...Object.keys(after.durability)])] as (keyof ModPreview["durability"])[];
  const handKeys = [...new Set([...Object.keys(before.handling), ...Object.keys(after.handling)])];
  return (
    <div class="wp-delta-table">
      <div class="wp-delta-head">Performance</div>
      <Row label="Class index">
        <Delta before={before.classIndex} after={after.classIndex} format={(v) => `${v} ${v === before.classIndex ? before.bracket : after.bracket}`} />
      </Row>
      <Row label="Peak power">
        <Delta before={b.peakPowerKw} after={a.peakPowerKw} format={(v) => `${f0(v)} kW`} epsilon={0.05} />
      </Row>
      <Row label="Mass">
        <Delta before={b.massKg} after={a.massKg} format={(v) => `${f0(v)} kg`} higherIsBetter={false} epsilon={0.05} />
      </Row>
      <Row label="Power / weight">
        <Delta before={b.powerToWeight} after={a.powerToWeight} format={(v) => `${f0(v)} W/kg`} epsilon={0.05} />
      </Row>
      <Row label="0–100 km/h">
        <Delta before={b.est0to100S} after={a.est0to100S} format={(v) => `${f2(v)} s`} higherIsBetter={false} epsilon={0.005} />
      </Row>
      <Row label="Top speed">
        <Delta before={b.estTopSpeedMps * 3.6} after={a.estTopSpeedMps * 3.6} format={(v) => `${f0(v)} km/h`} epsilon={0.3} />
      </Row>
      <Row label="Lateral grip">
        <Delta before={b.lateralGripG} after={a.lateralGripG} format={(v) => `${f2(v)} g`} epsilon={0.004} />
      </Row>
      <Row label="Braking">
        <Delta before={b.brakeDecelG} after={a.brakeDecelG} format={(v) => `${f2(v)} g`} epsilon={0.004} />
      </Row>
      <div class="wp-delta-head">Durability</div>
      {durKeys.length === 0 && <div class="wp-dim" style={{ fontSize: "0.85em" }}>Stock structure</div>}
      {durKeys.map((k) => (
        <Row key={k} label={words(String(k))}>
          <Delta before={before.durability[k] ?? 1} after={after.durability[k] ?? 1} format={(v) => `×${f2(v)}`} epsilon={0.001} />
        </Row>
      ))}
      {handKeys.length > 0 && <div class="wp-delta-head">Handling (not in the estimate)</div>}
      {handKeys.map((k) => (
        <Row key={k} label={k}>
          <Delta before={before.handling[k] ?? (k.endsWith("scale") ? 1 : 0)} after={after.handling[k] ?? (k.endsWith("scale") ? 1 : 0)} format={(v: number) => (Math.abs(v) < 10 ? f2(v) : f0(v))} epsilon={1e-4} />
        </Row>
      ))}
    </div>
  );
}

function Row(props: { label: string; children: ComponentChildren }) {
  return (
    <div class="wp-spread wp-delta-row">
      <span class="wp-dim2">{props.label}</span>
      {props.children}
    </div>
  );
}
