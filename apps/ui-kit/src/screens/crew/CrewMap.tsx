/**
 * Crews & territory: city zones tinted by the owning crew (opacity = control), contested zones
 * hatched, crew roster with holdings. Map geometry is the real city (`data/city`).
 */
import { useMemo, useState } from "preact/hooks";
import { useSend, useSlice } from "../../model/GameModel";
import type { Territory } from "../../model/types";
import { Badge, Meter, Panel, ScreenHead, useEscape } from "../../ui/kit";
import { centroid, polygonArea, polygonPath, roadsPath } from "../../ui/mapGeometry";
import { pct } from "../../ui/format";
import "./crew.css";

const POI_GLYPH: Record<string, string> = {
  PoliceStation: "★",
  Helipad: "H",
  Garage: "G",
  SafehouseCandidate: "S",
  RaceStartGrid: "⚑",
};

/** Crew / territory screen. */
export function CrewMap() {
  const send = useSend();
  const crew = useSlice((s) => s.crew);
  const map = useSlice((s) => s.map);
  const [sel, setSel] = useState<Territory | null>(null);
  useEscape(() => send({ type: "back" }));
  const roads = useMemo(() => roadsPath(map), [map]);
  const hue = (id: string | null) => crew.crews.find((c) => c.id === id)?.hue;
  const [x0, y0] = map.boundsMin;
  const [x1, y1] = map.boundsMax;
  const holdings = crew.crews.map((c) => {
    const t = crew.territories.filter((z) => z.owner === c.id);
    return { crew: c, zones: t.length, control: t.reduce((a, z) => a + z.control, 0) / Math.max(crew.territories.length, 1) };
  });
  return (
    <div class="wp-screen">
      <ScreenHead kicker="// TURF" title="Crews & territory" back="back" />
      <div class="wp-crew">
        <Panel title={map.id} tag="TERRITORY" class="wp-crew-map">
          <svg viewBox={`${x0} ${-y1} ${x1 - x0} ${y1 - y0}`} role="img" aria-label="Territory map" style={{ width: "100%", display: "block" }}>
            <defs>
              <pattern id="wp-contest" width="16" height="16" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
                <line x1="0" y1="0" x2="0" y2="16" stroke="var(--danger)" stroke-width="5" opacity="0.7" />
              </pattern>
            </defs>
            {crew.territories.map((t) => {
              const h = hue(t.owner);
              // Outer districts are huge polygons: keep them faint so the blocks read.
              const big = polygonArea(t.polygon) > 40000;
              return (
                <g key={t.zoneId} onClick={() => setSel(t)} style={{ cursor: "pointer" }}>
                  <path
                    d={polygonPath(t.polygon)}
                    fill={h === undefined ? "rgba(255,255,255,0.03)" : `hsl(${h} 85% 50% / ${big ? 0.1 : 0.15 + t.control * 0.5})`}
                    stroke={sel?.zoneId === t.zoneId ? "var(--ink)" : "rgba(0,0,0,0.6)"}
                    stroke-width={sel?.zoneId === t.zoneId ? 6 : 3}
                  />
                  {t.contested && <path d={polygonPath(t.polygon)} fill="url(#wp-contest)" pointer-events="none" />}
                </g>
              );
            })}
            <path d={roads} fill="none" stroke="rgba(0,0,0,0.85)" stroke-width="9" pointer-events="none" />
            <path d={roads} fill="none" stroke="rgba(230,239,233,0.18)" stroke-width="2.5" pointer-events="none" />
            {crew.pois.map((p) => (
              <g key={p.name} transform={`translate(${p.pos[0]} ${-p.pos[1]})`} pointer-events="none">
                <circle r="16" fill="#000" stroke="var(--ink-2)" stroke-width="2" />
                <text text-anchor="middle" dy="6" font-size="17" fill="var(--ink)" font-family="var(--font-mono)">
                  {POI_GLYPH[p.kind] ?? "•"}
                </text>
              </g>
            ))}
            {crew.territories
              .filter((t) => t.owner && t.control > 0.85 && polygonArea(t.polygon) <= 40000)
              .map((t) => {
                const [cx, cy] = centroid(t.polygon);
                return (
                  <text key={`l${t.zoneId}`} x={cx} y={-cy} text-anchor="middle" dy="7" font-size="20" font-weight="700" fill="rgba(255,255,255,0.75)" pointer-events="none" font-family="var(--font-display)">
                    {t.owner}
                  </text>
                );
              })}
          </svg>
          <div class="wp-row wp-mono wp-dim" style={{ fontSize: "0.75em", marginTop: "0.5em" }}>
            {Object.entries(POI_GLYPH).map(([k, g]) => (
              <span key={k}>
                {g} {k.replace("Candidate", "").replace(/([a-z])([A-Z])/g, "$1 $2")}
              </span>
            ))}
            <span>
              <span class="wp-danger">////</span> contested
            </span>
          </div>
        </Panel>
        <div class="wp-stack">
          <Panel title="Crews">
            {holdings
              .sort((a, b) => b.control - a.control)
              .map(({ crew: c, zones, control }) => (
                <div class="wp-crew-row" key={c.id}>
                  <span class="wp-crew-swatch" style={{ background: `hsl(${c.hue} 85% 50%)` }} />
                  <div>
                    <div class="wp-spread">
                      <b>
                        [{c.tag}] {c.name}
                      </b>
                      {c.isPlayers && <Badge tone="ok">Yours</Badge>}
                    </div>
                    <div class="wp-mono wp-dim" style={{ fontSize: "0.78em" }}>
                      {c.members} members · rep {c.rep.toLocaleString("en-US")} · {zones} blocks
                    </div>
                    <Meter value={control * 3} color={`hsl(${c.hue} 85% 50%)`} label={`${c.name} control`} />
                  </div>
                </div>
              ))}
          </Panel>
          <Panel title="Block" tag={sel ? `#${sel.zoneId}` : "SELECT ONE"}>
            {sel ? (
              <dl class="wp-kv">
                <dt>District</dt>
                <dd>{sel.kind}</dd>
                <dt>Held by</dt>
                <dd>{sel.owner ? crew.crews.find((c) => c.id === sel.owner)?.name : <span class="wp-dim">nobody</span>}</dd>
                <dt>Control</dt>
                <dd>{pct(sel.control)}</dd>
                <dt>Status</dt>
                <dd>{sel.contested ? <Badge tone="danger">Contested</Badge> : <Badge>Quiet</Badge>}</dd>
              </dl>
            ) : (
              <p class="wp-dim" style={{ margin: 0 }}>
                Click a block on the map.
              </p>
            )}
          </Panel>
        </div>
      </div>
    </div>
  );
}
