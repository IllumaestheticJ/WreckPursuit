// Radio scanner playback. The Rust director owns the queue, priorities, tier delay, chop and
// music ducking; this file only *performs* RadioSay / RadioEnd: squelch, static bed and a
// placeholder voice from the browser's speechSynthesis.
//
// Limitation: speechSynthesis plays straight to the OS, so it cannot pass through the Web Audio
// radio EQ. The squelch and static do (mixer.radioIn), which sells the effect. Recorded line
// packs (future) will be AudioBuffers routed through the same EQ.

import type { VoicePool } from "./voices";
import type { RadioPriority } from "./types";

const RADIO_PACK = "radio_rec";

export interface RadioLog {
  line: number;
  unit: number;
  text: string;
  priority: RadioPriority;
  interrupted: boolean;
}

export class ScannerPlayer {
  private current: { line: number; stopStatic: () => void } | null = null;
  /** Last lines for the debug panel, newest first. */
  readonly log: RadioLog[] = [];
  /** Speak with TTS (off = squelch + static only). */
  tts = true;
  /** Scanner volume 0..1 (applied to TTS, which bypasses the mixer). */
  volume = 0.9;

  constructor(private pool: VoicePool) {}

  say(line: number, unit: number, text: string, priority: RadioPriority, pitch: number): void {
    this.end(this.current?.line ?? -1, true);
    this.pool.playLocal(RADIO_PACK, "radio/squelch_on", 0.6);
    const stopStatic = this.pool.playLocal(RADIO_PACK, "radio/static", 0.12);
    this.current = { line, stopStatic };
    this.log.unshift({ line, unit, text, priority, interrupted: false });
    this.log.length = Math.min(this.log.length, 8);
    if (this.tts && typeof speechSynthesis !== "undefined") {
      const u = new SpeechSynthesisUtterance(text);
      u.pitch = Math.min(2, Math.max(0, pitch * 0.9));
      u.rate = priority === "Urgent" || priority === "Pursuit" ? 1.25 : 1.1;
      u.volume = this.volume;
      speechSynthesis.speak(u);
    }
  }

  end(line: number, interrupted: boolean): void {
    if (!this.current || this.current.line !== line) return;
    this.current.stopStatic();
    this.pool.playLocal(RADIO_PACK, "radio/squelch_off", 0.5);
    if (interrupted && typeof speechSynthesis !== "undefined") speechSynthesis.cancel();
    const entry = this.log.find((l) => l.line === line);
    if (entry) entry.interrupted = interrupted;
    this.current = null;
  }

  stopAll(): void {
    if (this.current) this.end(this.current.line, true);
  }
}
