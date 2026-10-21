/**
 * Heading-up circular minimap. Static road geometry is one memoised path; only the group
 * transform changes per frame.
 */
import { useMemo } from "preact/hooks";
import type { CityMap, MapBlip, Vec2 } from "../../model/types";
import { headingDeg, roadsPath } from "../../ui/mapGeometry";
import { heliShape } from "./HeliIcon";

const BLIP_COLOR: Record<MapBlip["kind"], string> = {
  player: "var(--scan)",
  racer: "var(--warn)",
  police: "var(--police-b)",
  helicopter: "var(--heli)",
  traffic: "var(--ink-3)",
  checkpoint: "var(--cash)",
  incident: "var(--danger)",
};

/** Minimap. `radiusM` is the world radius shown. */
export function Minimap(props: { map: CityMap; pos: Vec2; headingRad: number; blips: MapBlip[]; radiusM?: number; size?: string; flashing?: boolean }) {
  const R = props.radiusM ?? 260;
  const roads = useMemo(() => roadsPath(props.map), [props.map]);
  const [px, py] = props.pos;
  const rot = -headingDeg(props.headingRad);
  return (
    <svg viewBox={`${-R} ${-R} ${2 * R} ${2 * R}`} style={{ width: props.size ?? "11em", display: "block" }} role="img" aria-label="Minimap">
      <defs>
        <clipPath id="wp-minimap-clip">
          <circle r={R} />
        </clipPath>
      </defs>
      <circle r={R} fill="rgba(5,8,7,0.82)" stroke={props.flashing ? "var(--police-a)" : "var(--edge-hot)"} stroke-width={R * 0.02} />
      <g clip-path="url(#wp-minimap-clip)">
        <g transform={`rotate(${rot}) translate(${-px} ${py})`}>
          <path d={roads} fill="none" stroke="rgba(230,239,233,0.22)" stroke-width={R * 0.035} stroke-linecap="round" stroke-linejoin="round" />
          {/* Who the helicopter is watching: dashed tether + pulsing spotlight ring, under every blip. */}
          {props.blips.map((b) =>
            b.tracking ? (
              <g key={`t${b.kind}${b.id}`} class="wp-minimap-track">
                <line x1={b.pos[0]} y1={-b.pos[1]} x2={b.tracking[0]} y2={-b.tracking[1]} stroke="var(--heli)" stroke-width={R * 0.012} stroke-dasharray={`${R * 0.04} ${R * 0.03}`} />
                <circle cx={b.tracking[0]} cy={-b.tracking[1]} r={R * 0.09} fill="var(--heli)" fill-opacity={0.15} stroke="var(--heli)" stroke-width={R * 0.012}>
                  <animate attributeName="r" values={`${R * 0.07};${R * 0.12};${R * 0.07}`} dur="1.2s" repeatCount="indefinite" />
                </circle>
              </g>
            ) : null,
          )}
          {[...props.blips].sort((a, b) => Number(a.kind === "helicopter") - Number(b.kind === "helicopter")).map((b) => {
            const color = BLIP_COLOR[b.kind];
            const [x, y] = [b.pos[0], -b.pos[1]];
            if (b.kind === "helicopter") {
              // Distinct from the car triangles: a top-down helicopter, drawn on top, with its
              // call sign kept upright against the map's rotation.
              return (
                <g key={`h${b.id}`} transform={`translate(${x} ${y})`}>
                  <g transform={`rotate(${headingDeg(b.headingRad ?? 0)})`}>{heliShape(R * 0.075, color)}</g>
                  {b.label && (
                    <text transform={`rotate(${-rot})`} x={R * 0.1} y={-R * 0.08} font-size={R * 0.075} fill={color} stroke="#000" stroke-width={R * 0.01} paint-order="stroke" font-family="var(--font-mono)" font-weight="bold">
                      {b.label}
                    </text>
                  )}
                </g>
              );
            }
            if (b.kind === "checkpoint") {
              return <circle key={`c${b.id}`} cx={x} cy={y} r={R * 0.05} fill="none" stroke={color} stroke-width={R * 0.015} />;
            }
            return (
              <g key={`${b.kind}${b.id}`} transform={`translate(${x} ${y}) rotate(${headingDeg(b.headingRad ?? 0)})`}>
                <path d={`M0 ${-R * 0.05} L${R * 0.035} ${R * 0.035} L${-R * 0.035} ${R * 0.035} Z`} fill={color} />
              </g>
            );
          })}
        </g>
        {/* North marker rotates with the map. */}
        <g transform={`rotate(${rot})`}>
          <text y={-R * 0.84} text-anchor="middle" font-size={R * 0.1} fill="var(--ink-3)" font-family="var(--font-mono)">
            N
          </text>
        </g>
      </g>
      {/* Player arrow fixed at the centre, pointing up. */}
      <path d={`M0 ${-R * 0.07} L${R * 0.05} ${R * 0.05} L0 ${R * 0.025} L${-R * 0.05} ${R * 0.05} Z`} fill="var(--scan)" stroke="#000" stroke-width={R * 0.008} />
    </svg>
  );
}
