// Radio scanner playback. The Rust director owns the queue, priorities, tier delay, chop and
// music ducking; this file only *performs* RadioSay / RadioEnd: squelch on/off clicks and the
// static bed (recorded `radio_rec` pack when shipped, else the procedural one) through the
// radio EQ (mixer.radioIn: band-pass + drive on the Police bus), and the spoken line from the
// browser's speechSynthesis (owner playtest row 79).
//
// Voices (owner, 2026-10-22): officers are male, dispatch is female. Each unit gets its own
// male system voice (English preferred, picked by a fixed hash of the unit number), pitch
// (wp_audio's per-unit pitch, kept low) and rate; dispatch (unit 0) a female voice, pitched up.
// Browsers do not expose a voice's gender, so it is guessed from the voice name
// (`voiceGender`); when no voice of that gender exists the pitch carries the difference.
//
// Priority tiers (row 144): tier 1 (roadblock / spikes / helicopter / unit ahead) gets a
// two-tone alert before the voice; tier 3 (impacts, wrecks) is shouted: louder static burst,
// faster and higher speech. Tiers 4–5 are played only with "Radio: All". Limitation: speechSynthesis plays straight to the OS, so the
// spoken words cannot pass through the Web Audio radio EQ; the squelch and static do, which
// sells the effect. Its volume follows master × Police bus. Without speechSynthesis (or with
// the "Police radio voice" setting off) a line is squelch + static + the radio strip's text.
// Recorded line packs (future) will be AudioBuffers routed through the same EQ.
//
// Helicopter pilot lines ("Air 9: …", PR #32) get the pilot treatment instead of the static
// bed (heli.ts): rotor-chopped cockpit noise through a harder radio EQ, a mic-key chirp (not on
// tier 1 lines, which already get the alert), a lower, quicker voice, and HELI_LOUDNESS on all
// of it. Their tier comes from wp_police like any line (eyes on / lost visual / reacquired are
// tier 1, position reports tier 4, so "Important only" drops those).

import { HELI_LOUDNESS, isAirLine, type HeliAudio } from "./heli";
import type { VoicePool } from "./voices";
import type { RadioPriority } from "./types";

export interface RadioLog {
  line: number;
  unit: number;
  text: string;
  priority: RadioPriority;
  /** Police priority tier, 0 = unknown. */
  tier: number;
  interrupted: boolean;
}

const FEMALE = /female|woman|samantha|victoria|karen|moira|tessa|fiona|zira|aria|jenny|susan|serena|allison|ava|kate|libby|sonia|hazel|heera|catherine|veena|joanna|salli|kimberly|emma|amy|nicole|olivia|natasha|clara|michelle|sara|elsa/i;
const MALE = /\bmale\b|\bman\b|daniel|alex\b|fred|david|mark|guy|george|james|thomas|oliver|arthur|rishi|aaron|tom\b|matthew|joey|justin|brian|russell|ryan|eric|christopher|roger|steffan|william|liam|gordon|lee\b|ralph|bruce/i;

/** Gender guessed from a speech voice's name ("Google UK English Female", "Microsoft David",
 * "Samantha"); null when the name does not say. "Female" is checked first ("Female" contains
 * "male"). */
export function voiceGender(name: string): "female" | "male" | null {
  if (FEMALE.test(name)) return "female";
  if (MALE.test(name)) return "male";
  return null;
}

/** Voice for a speaker: dispatch (unit 0) female, units male, from `all` (English preferred);
 * falls back to voices of unknown gender, then to any voice. Pure (unit-tested). */
export function pickVoice<V extends { name: string; lang: string }>(all: readonly V[], unit: number): V | null {
  if (!all.length) return null;
  const en = all.filter((v) => v.lang.toLowerCase().startsWith("en"));
  const pool = en.length ? en : all;
  const want = unit === 0 ? "female" : "male";
  const exact = pool.filter((v) => voiceGender(v.name) === want);
  const unknown = pool.filter((v) => voiceGender(v.name) === null);
  const from = exact.length ? exact : unknown.length ? unknown : pool;
  return unit === 0 ? from[0] : from[hash(unit) % from.length];
}

/** Speech pitch for a speaker (0..2): dispatch higher, units low (wp_audio's per-unit pitch,
 * 0.85..1.15, scaled down), shouted tier 3 a little higher still. */
export function speechPitch(unit: number, unitPitch: number, tier: number): number {
  const base = unit === 0 ? 1.15 : 0.62 + (unitPitch - 0.85) * 0.5;
  return Math.min(2, Math.max(0, base + (tier === 3 ? 0.12 : 0)));
}

/** Fixed integer hash (stable across sessions) for voice / rate picks. */
function hash(n: number): number {
  return Math.imul(n + 1, 2654435761) >>> 16;
}

export class ScannerPlayer {
  private current: { line: number; stopStatic: () => void; air: boolean } | null = null;
  /** Last lines for the debug panel, newest first. */
  readonly log: RadioLog[] = [];
  /** Speak with TTS (off = squelch + static only): the "Police radio voice" setting. */
  tts = true;
  /** Spoken-line volume 0..1 (master × Police bus: TTS bypasses the mixer). */
  volume = 0.9;
  /** speechSynthesis.speak calls made (e2e: speech is attempted when supported). */
  speechAttempts = 0;
  /** Called for every line the scanner says (the radio strip shows it). */
  onSay: ((unit: number, text: string, tier: number) => void) | null = null;
  /** Alert tones played before tier 1 lines (e2e). */
  tones = 0;

  /** Recorded radio pack when shipped (the pool swaps in its procedural fallback on request). */
  private readonly pack: string;

  /** Helicopter pilot lines spoken (e2e). */
  airLines = 0;

  constructor(
    private pool: VoicePool,
    private heli: HeliAudio | null = null,
  ) {
    this.pack = pool.has("radio_rec", "radio/static") ? "radio_rec" : "radio_proc";
  }

  /** Whether this browser can speak lines. */
  static speechSupported(): boolean {
    return typeof speechSynthesis !== "undefined" && typeof SpeechSynthesisUtterance !== "undefined";
  }

  /** The system voice for a unit (null = the browser default): dispatch female, units male. */
  private voiceFor(unit: number): SpeechSynthesisVoice | null {
    return pickVoice(speechSynthesis.getVoices(), unit);
  }

  say(line: number, unit: number, text: string, priority: RadioPriority, pitch: number, tier = 0): void {
    this.end(this.current?.line ?? -1, true);
    const air = !!this.heli && isAirLine(text);
    const loud = air ? HELI_LOUDNESS : 1;
    if (air) this.airLines++;
    this.pool.playLocal(this.pack, "radio/squelch_on", (tier === 3 ? 0.9 : 0.6) * loud);
    // Tier 3: a louder, harsher static bed (the chaos comes across); tier 1: a two-tone alert.
    // The pilot: the rotor-chopped cockpit bed instead of the static.
    const stopStatic = air && this.heli ? this.heli.keyUp(tier !== 1) : this.pool.playLocal(this.pack, "radio/static", tier === 3 ? 0.32 : 0.12);
    let lead_s = 0;
    if (tier === 1) {
      lead_s = this.pool.playRadioTone([1250, 1650], 0.11, 0.18);
      this.tones++;
    }
    this.current = { line, stopStatic, air };
    this.log.unshift({ line, unit, text, priority, tier, interrupted: false });
    this.log.length = Math.min(this.log.length, 8);
    this.onSay?.(unit, text, tier);
    if (this.tts && this.volume > 0 && ScannerPlayer.speechSupported()) {
      try {
        const u = new SpeechSynthesisUtterance(text);
        const voice = this.voiceFor(unit);
        if (voice) u.voice = voice;
        u.pitch = speechPitch(unit, pitch, tier) * (air ? 0.85 : 1);
        // Each unit talks at its own pace; urgent / pursuit traffic a bit faster, shouted
        // impact traffic faster still; the pilot clipped and quick over the rotor.
        const base = 1.05 + (hash(unit * 7 + 3) % 5) * 0.04;
        u.rate = air ? base + 0.2 : tier === 3 ? base + 0.25 : priority === "Urgent" || priority === "Pursuit" ? base + 0.12 : base;
        u.volume = Math.min(1, this.volume * (tier === 3 ? 1 : 0.9) * loud);
        this.speechAttempts++;
        if (lead_s > 0) setTimeout(() => speechSynthesis.speak(u), lead_s * 1000);
        else speechSynthesis.speak(u);
      } catch (e) {
        console.warn("radio voice:", e);
      }
    }
  }

  end(line: number, interrupted: boolean): void {
    if (!this.current || this.current.line !== line) return;
    this.current.stopStatic();
    this.pool.playLocal(this.pack, "radio/squelch_off", 0.5 * (this.current.air ? HELI_LOUDNESS : 1));
    if (interrupted && ScannerPlayer.speechSupported()) speechSynthesis.cancel();
    const entry = this.log.find((l) => l.line === line);
    if (entry) entry.interrupted = interrupted;
    this.current = null;
  }

  stopAll(): void {
    if (this.current) this.end(this.current.line, true);
  }
}
