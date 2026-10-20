// Radio scanner playback. The Rust director owns the queue, priorities, tier delay, chop and
// music ducking; this file only *performs* RadioSay / RadioEnd: squelch on/off clicks and the
// static bed (recorded `radio_rec` pack when shipped, else the procedural one) through the
// radio EQ (mixer.radioIn: band-pass + drive on the Police bus), and the spoken line from the
// browser's speechSynthesis (owner playtest row 79).
//
// Voices: each unit gets its own system voice (English voices preferred, picked by a fixed
// hash of the unit number), pitch (wp_audio's per-unit pitch) and rate; dispatch (unit 0)
// always uses the first voice. Limitation: speechSynthesis plays straight to the OS, so the
// spoken words cannot pass through the Web Audio radio EQ; the squelch and static do, which
// sells the effect. Its volume follows master × Police bus. Without speechSynthesis (or with
// the "Police radio voice" setting off) a line is squelch + static + the radio strip's text.
// Recorded line packs (future) will be AudioBuffers routed through the same EQ.

import type { VoicePool } from "./voices";
import type { RadioPriority } from "./types";

export interface RadioLog {
  line: number;
  unit: number;
  text: string;
  priority: RadioPriority;
  interrupted: boolean;
}

/** Fixed integer hash (stable across sessions) for voice / rate picks. */
function hash(n: number): number {
  return Math.imul(n + 1, 2654435761) >>> 16;
}

export class ScannerPlayer {
  private current: { line: number; stopStatic: () => void } | null = null;
  /** Last lines for the debug panel, newest first. */
  readonly log: RadioLog[] = [];
  /** Speak with TTS (off = squelch + static only): the "Police radio voice" setting. */
  tts = true;
  /** Spoken-line volume 0..1 (master × Police bus: TTS bypasses the mixer). */
  volume = 0.9;
  /** speechSynthesis.speak calls made (e2e: speech is attempted when supported). */
  speechAttempts = 0;
  /** Called for every line the scanner says (the radio strip shows it). */
  onSay: ((unit: number, text: string) => void) | null = null;

  /** Recorded radio pack when shipped (the pool swaps in its procedural fallback on request). */
  private readonly pack: string;

  constructor(private pool: VoicePool) {
    this.pack = pool.has("radio_rec", "radio/static") ? "radio_rec" : "radio_proc";
  }

  /** Whether this browser can speak lines. */
  static speechSupported(): boolean {
    return typeof speechSynthesis !== "undefined" && typeof SpeechSynthesisUtterance !== "undefined";
  }

  /** The system voice for a unit (null = the browser default). */
  private voiceFor(unit: number): SpeechSynthesisVoice | null {
    const all = speechSynthesis.getVoices();
    if (!all.length) return null;
    const en = all.filter((v) => v.lang.toLowerCase().startsWith("en"));
    const pool = en.length ? en : all;
    return unit === 0 ? pool[0] : pool[hash(unit) % pool.length];
  }

  say(line: number, unit: number, text: string, priority: RadioPriority, pitch: number): void {
    this.end(this.current?.line ?? -1, true);
    this.pool.playLocal(this.pack, "radio/squelch_on", 0.6);
    const stopStatic = this.pool.playLocal(this.pack, "radio/static", 0.12);
    this.current = { line, stopStatic };
    this.log.unshift({ line, unit, text, priority, interrupted: false });
    this.log.length = Math.min(this.log.length, 8);
    this.onSay?.(unit, text);
    if (this.tts && this.volume > 0 && ScannerPlayer.speechSupported()) {
      try {
        const u = new SpeechSynthesisUtterance(text);
        const voice = this.voiceFor(unit);
        if (voice) u.voice = voice;
        u.pitch = Math.min(2, Math.max(0, pitch * 0.9));
        // Each unit talks at its own pace; urgent / pursuit traffic a bit faster.
        const base = 1.05 + (hash(unit * 7 + 3) % 5) * 0.04;
        u.rate = priority === "Urgent" || priority === "Pursuit" ? base + 0.12 : base;
        u.volume = Math.min(1, this.volume);
        this.speechAttempts++;
        speechSynthesis.speak(u);
      } catch (e) {
        console.warn("radio voice:", e);
      }
    }
  }

  end(line: number, interrupted: boolean): void {
    if (!this.current || this.current.line !== line) return;
    this.current.stopStatic();
    this.pool.playLocal(this.pack, "radio/squelch_off", 0.5);
    if (interrupted && ScannerPlayer.speechSupported()) speechSynthesis.cancel();
    const entry = this.log.find((l) => l.line === line);
    if (entry) entry.interrupted = interrupted;
    this.current = null;
  }

  stopAll(): void {
    if (this.current) this.end(this.current.line, true);
  }
}
