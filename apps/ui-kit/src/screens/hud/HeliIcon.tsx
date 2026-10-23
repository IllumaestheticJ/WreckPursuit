/**
 * Police helicopter glyphs (PR #32): `heliShape` draws the top-down silhouette inside an SVG
 * (minimap, centred on 0,0, nose up, `s` = rotor radius); `HeliBadge` is the inline "AIR 9"
 * badge on pilot radio lines. Both use `--heli`, apart from the red / blue cars.
 */

/** Top-down helicopter: rotor disc, two blades, fuselage, tail boom and tail rotor. */
export function heliShape(s: number, color: string, outline = "#000") {
  const w = s * 0.16;
  return (
    <g>
      <circle r={s} fill={color} fill-opacity={0.18} stroke={color} stroke-width={s * 0.06} />
      <path d={`M${-s} 0 L${s} 0 M0 ${-s} L0 ${s}`} stroke={color} stroke-width={s * 0.08} stroke-linecap="round" />
      <ellipse rx={s * 0.32} ry={s * 0.45} cy={-s * 0.1} fill={color} stroke={outline} stroke-width={s * 0.05} />
      <rect x={-w / 2} y={s * 0.3} width={w} height={s * 0.95} fill={color} stroke={outline} stroke-width={s * 0.04} />
      <path d={`M${-s * 0.28} ${s * 1.25} L${s * 0.28} ${s * 1.25}`} stroke={color} stroke-width={s * 0.1} stroke-linecap="round" />
    </g>
  );
}

/** "AIR 9"-style badge (with a tiny helicopter) for lines spoken by the pilot. */
export function HeliBadge(props: { text?: string }) {
  return (
    <span class="wp-heli-badge" aria-label="helicopter">
      <svg viewBox="-1.4 -1.4 2.8 3" width="1em" height="1em" aria-hidden="true">
        {heliShape(1, "currentColor", "transparent")}
      </svg>
      {props.text ?? "AIR"}
    </span>
  );
}

/** Call sign at the start of a pilot line ("Air 9: eyes on …" → "AIR 9"). */
export function airCallsign(text: string): string {
  const m = /^Air (\d+)/.exec(text);
  return m ? `AIR ${m[1]}` : "AIR";
}
