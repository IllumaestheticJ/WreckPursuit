/**
 * A car's picture for lists and pickers: its three-quarter thumbnail, turning on a turntable
 * while hovered or focused (or always with `spin`). Renders nothing for an id without a
 * thumbnail (cars added before `gen-thumbs` ran).
 */
import { carThumbUrl, carTurntableUrl, TURN_FRAMES } from "../data/thumbs";
import "./carThumb.css";

/** Thumbnail of car `id`; `size` sm 96 × 54 px, md 160 × 90, lg 320 × 180. */
export function CarThumb(props: { id: string; size?: "sm" | "md" | "lg"; spin?: boolean; alt?: string }) {
  const thumb = carThumbUrl(props.id);
  if (!thumb) return null;
  const turn = carTurntableUrl(props.id);
  return (
    <span
      class={`wp-carthumb wp-carthumb--${props.size ?? "sm"} ${props.spin ? "wp-carthumb--spin" : ""}`}
      data-car-thumb={props.id}
      role="img"
      aria-label={props.alt ?? props.id}
      style={{ "--wp-turn-frames": TURN_FRAMES } as Record<string, number>}
    >
      <img class="wp-carthumb-still" src={thumb} alt="" draggable={false} />
      {turn && <img class="wp-carthumb-turn" src={turn} alt="" draggable={false} />}
    </span>
  );
}
