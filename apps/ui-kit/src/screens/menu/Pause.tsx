/** In-race pause menu (over a dimmed HUD). */
import { useSend, useSlice } from "../../model/GameModel";
import { Hud } from "../hud/Hud";
import { DamageSilhouette } from "../hud/DamageSilhouette";
import { Badge, Button, Panel } from "../../ui/kit";
import { words } from "../../ui/format";
import "./menu.css";

/** Pause screen. */
export function Pause() {
  const send = useSend();
  const hud = useSlice((s) => s.hud);
  const failed = hud.failed;
  return (
    <div class="wp-pause">
      <div class="wp-pause-under" aria-hidden="true">
        <Hud />
      </div>
      <div class="wp-pause-body">
        <Panel title="Paused" tag={hud.heat.state === "pursuit" ? "PURSUIT ACTIVE" : undefined} class="wp-pause-panel">
          <div class="wp-pause-grid">
            <nav class="wp-stack" aria-label="Pause menu">
              <Button variant="primary" onClick={() => send({ type: "resume" })} hotkey="Esc">
                Resume
              </Button>
              <Button onClick={() => send({ type: "navigate", screen: "settings" })}>Settings</Button>
              <Button onClick={() => send({ type: "navigate", screen: "replay" })}>Replay</Button>
              <Button onClick={() => send({ type: "replay.seek", tick: 0 })}>Restart</Button>
              <Button variant="danger" onClick={() => send({ type: "quitToMenu" })}>
                Quit to menu
              </Button>
            </nav>
            <div>
              <DamageSilhouette regions={hud.regions} detached={hud.detached} components={hud.components} wheelsDetached={hud.wheelsDetached} totalLoss={hud.totalLoss} width="7em" labels />
            </div>
            <div class="wp-stack">
              <div class="wp-mono wp-dim wp-upper" style={{ fontSize: "0.7em" }}>
                Condition
              </div>
              <div class="wp-mono" style={{ fontSize: "1.6em" }}>
                {Math.round(hud.health * 100)}%
              </div>
              <div class="wp-row">
                {failed.length === 0 ? <Badge tone="ok">No failures</Badge> : failed.map((c) => <Badge key={c} tone="danger">{words(c)}</Badge>)}
              </div>
              {hud.heat.state === "pursuit" && <p class="wp-warn">Quitting during a pursuit counts as a capture.</p>}
            </div>
          </div>
        </Panel>
      </div>
    </div>
  );
}
