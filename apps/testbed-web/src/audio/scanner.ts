// Radio scanner playback. The Rust director owns the queue, priorities, tier delay, chop and
// music ducking; this file only *performs* RadioSay / RadioEnd: squelch, static bed and a
// placeholder voice from the browser's speechSynthesis.
//
// Limitation: speechSynthesis plays straight to the OS, so it cannot pass through the Web Audio
// radio EQ. The squelch and static do (mixer.radioIn), which sells the effect. Recorded line
// packs (future) will be AudioBuffers routed through the same EQ.
//
// Helicopter lines ("Air-1, …") get the pilot radio treatment instead of the static bed
// (heli.ts): mic-key chirp, rotor-chopped cockpit noise, a flatter, quicker voice, and
// HELI_LOUDNESS on all of it.

import { HELI_LOUDNESS, isAirLine, type HeliAudio } from "./heli";
import type { VoicePool } from "./voices";
import type { RadioPriority } from "./types";

const RADIO_PACK = "radio_proc";

export interface RadioLog {
  line: number;
  unit: number;
  text: string;
  priority: RadioPriority;
  interrupted: boolean;
}

export class ScannerPlayer {
  private current: { line: number; stopStatic: () => void; air: boolean } | null = null;
  /** Last lines for the debug panel, newest first. */
  readonly log: RadioLog[] = [];
  /** Speak with TTS (off = squelch + static only). */
  tts = true;
  /** Scanner volume 0..1 (applied to TTS, which bypasses the mixer). */
  volume = 0.9;

  constructor(
    private pool: VoicePool,
    private heli: HeliAudio,
  ) {}

  say(line: number, unit: number, text: string, priority: RadioPriority, pitch: number): void {
    this.end(this.current?.line ?? -1, true);
    const air = isAirLine(text);
    const loud = air ? HELI_LOUDNESS : 1;
    this.pool.playLocal(RADIO_PACK, "radio/squelch_on", 0.6 * loud);
    const stopStatic = air ? this.heli.keyUp() : this.pool.playLocal(RADIO_PACK, "radio/static", 0.12);
    this.current = { line, stopStatic, air };
    this.log.unshift({ line, unit, text, priority, interrupted: false });
    this.log.length = Math.min(this.log.length, 8);
    if (this.tts && typeof speechSynthesis !== "undefined") {
      const u = new SpeechSynthesisUtterance(text);
      // The pilot: lower, clipped and quick over the rotor noise.
      u.pitch = Math.min(2, Math.max(0, pitch * (air ? 0.7 : 0.9)));
      u.rate = air ? 1.3 : priority === "Urgent" || priority === "Pursuit" ? 1.25 : 1.1;
      u.volume = Math.min(1, this.volume * loud);
      speechSynthesis.speak(u);
    }
  }

  end(line: number, interrupted: boolean): void {
    if (!this.current || this.current.line !== line) return;
    this.current.stopStatic();
    this.pool.playLocal(RADIO_PACK, "radio/squelch_off", 0.5 * (this.current.air ? HELI_LOUDNESS : 1));
    if (interrupted && typeof speechSynthesis !== "undefined") speechSynthesis.cancel();
    const entry = this.log.find((l) => l.line === line);
    if (entry) entry.interrupted = interrupted;
    this.current = null;
  }

  stopAll(): void {
    if (this.current) this.end(this.current.line, true);
  }
}
