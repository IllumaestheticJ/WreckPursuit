/**
 * Top-down car silhouette coloured by `BodyRegion` damage severity (front at the top).
 * Frame rails and the cabin cell are drawn as inner strokes; wheels use the `Wheel*` component
 * health and disappear when detached. Detached panels render as dashed outlines.
 */
import type { BodyRegion, Component } from "../../generated/contracts";
import { severityColor } from "../../ui/kit";

/** Panel polygons in a 100 × 220 box (x right, y down = rearward). */
const PANELS: { region: BodyRegion; d: string }[] = [
  { region: "FrontBumper", d: "M22 14 Q50 2 78 14 L80 24 L20 24 Z" },
  { region: "Hood", d: "M26 26 L74 26 L72 66 L28 66 Z" },
  { region: "FrontLeftFender", d: "M18 26 L25 26 L27 66 L14 66 L14 34 Z" },
  { region: "FrontRightFender", d: "M75 26 L82 26 L86 34 L86 66 L73 66 Z" },
  { region: "Windshield", d: "M29 68 L71 68 L67 86 L33 86 Z" },
  { region: "Roof", d: "M33 88 L67 88 L67 134 L33 134 Z" },
  { region: "LeftDoorF", d: "M14 68 L27 68 L31 88 L31 110 L14 110 Z" },
  { region: "LeftDoorR", d: "M14 112 L31 112 L31 136 L27 150 L14 150 Z" },
  { region: "RightDoorF", d: "M73 68 L86 68 L86 110 L69 110 L69 88 Z" },
  { region: "RightDoorR", d: "M69 112 L86 112 L86 150 L73 150 L69 136 Z" },
  { region: "RearGlass", d: "M33 136 L67 136 L70 152 L30 152 Z" },
  { region: "Trunk", d: "M28 154 L72 154 L74 194 L26 194 Z" },
  { region: "RearLeftQuarter", d: "M14 152 L27 152 L25 194 L18 194 L14 186 Z" },
  { region: "RearRightQuarter", d: "M73 152 L86 152 L86 186 L82 194 L75 194 Z" },
  { region: "RearBumper", d: "M20 196 L80 196 L78 206 Q50 216 22 206 Z" },
];

/** Structure drawn as strokes over the panels. */
const FRAME: { region: BodyRegion; d: string }[] = [
  { region: "FrameFront", d: "M30 20 L70 20 M34 20 L34 64 M66 20 L66 64" },
  { region: "FrameLeft", d: "M20 66 L20 152" },
  { region: "FrameRight", d: "M80 66 L80 152" },
  { region: "FrameRear", d: "M34 156 L34 200 M66 156 L66 200 M30 200 L70 200" },
];

const WHEELS: { c: Component; x: number; y: number; idx: number }[] = [
  { c: "WheelFL", x: 4, y: 36, idx: 0 },
  { c: "WheelFR", x: 86, y: 36, idx: 1 },
  { c: "WheelRL", x: 4, y: 158, idx: 2 },
  { c: "WheelRR", x: 86, y: 158, idx: 3 },
];

/** Props: region severities 0..=1, detached regions, component health, detached wheel indices. */
export function DamageSilhouette(props: {
  regions: Partial<Record<BodyRegion, number>>;
  detached?: BodyRegion[];
  components?: Partial<Record<Component, number>>;
  wheelsDetached?: number[];
  totalLoss?: boolean;
  width?: string;
  labels?: boolean;
}) {
  const sev = (r: BodyRegion) => props.regions[r] ?? 0;
  const cabin = sev("Cabin");
  return (
    <svg
      viewBox="0 0 100 220"
      style={{ width: props.width ?? "6em", display: "block", overflow: "visible" }}
      role="img"
      aria-label="Damage by body region"
    >
      <defs>
        <pattern id="wp-hatch" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="4" stroke="var(--danger)" stroke-width="1.2" />
        </pattern>
      </defs>
      {WHEELS.map((w) => {
        if (props.wheelsDetached?.includes(w.idx)) {
          return <rect key={w.c} x={w.x} y={w.y} width="10" height="24" rx="2" fill="none" stroke="var(--danger)" stroke-dasharray="2 2" />;
        }
        const h = props.components?.[w.c] ?? 1;
        return <rect key={w.c} x={w.x} y={w.y} width="10" height="24" rx="2" fill="#151a17" stroke={severityColor(1 - h)} stroke-width="1.6" />;
      })}
      {PANELS.map((p) => {
        const s = sev(p.region);
        const gone = props.detached?.includes(p.region);
        return (
          <path
            key={p.region}
            d={p.d}
            fill={gone ? "url(#wp-hatch)" : s > 0.01 ? severityColor(s) : "rgba(230,239,233,0.07)"}
            fill-opacity={gone ? 1 : s > 0.01 ? 0.35 + 0.6 * s : 1}
            stroke={gone ? "var(--danger)" : "rgba(230,239,233,0.35)"}
            stroke-dasharray={gone ? "3 2" : undefined}
            stroke-width="0.8"
          >
            <title>{`${p.region}: ${Math.round(s * 100)}%${gone ? " (detached)" : ""}`}</title>
          </path>
        );
      })}
      {/* Cabin safety cell: outline thickens and reddens as it intrudes. */}
      <rect x="31" y="70" width="38" height="80" fill="none" stroke={cabin > 0.01 ? severityColor(cabin) : "rgba(230,239,233,0.15)"} stroke-width={1 + cabin * 3} stroke-dasharray="4 2">
        <title>{`Cabin: ${Math.round(cabin * 100)}%`}</title>
      </rect>
      {FRAME.map((f) => {
        const s = sev(f.region);
        return s > 0.01 ? <path key={f.region} d={f.d} fill="none" stroke={severityColor(s)} stroke-width={1 + s * 2.5} stroke-linecap="round" /> : null;
      })}
      {props.totalLoss && (
        <g>
          <rect x="6" y="96" width="88" height="28" fill="var(--danger)" />
          <text x="50" y="115" text-anchor="middle" font-size="13" font-weight="700" fill="#120404" font-family="var(--font-display)" letter-spacing="1">
            TOTAL LOSS
          </text>
        </g>
      )}
      {props.labels && (
        <g font-size="7" fill="var(--ink-3)" font-family="var(--font-mono)" text-anchor="middle">
          <text x="50" y="-2">FRONT</text>
          <text x="50" y="226">REAR</text>
        </g>
      )}
    </svg>
  );
}
