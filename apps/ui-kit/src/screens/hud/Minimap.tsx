/**
 * Heading-up circular minimap. Static road geometry is one memoised path; only the group
 * transform changes per frame.
 */
import { useMemo } from "preact/hooks";
import type { CityMap, MapBlip, Vec2 } from "../../model/types";
import { headingDeg, roadsPath } from "../../ui/mapGeometry";

const BLIP_COLOR: Record<MapBlip["kind"], string> = {
  player: "var(--scan)",
  racer: "var(--warn)",
  police: "var(--police-b)",
  helicopter: "var(--police-a)",
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
          {props.blips.map((b) => {
            const color = BLIP_COLOR[b.kind];
            const [x, y] = [b.pos[0], -b.pos[1]];
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
