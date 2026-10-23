// Thin wrapper over the audio functions of the wp_wasm surface (crates/wp_wasm/src/audio.rs).
import * as wasm from "../wasm/pkg/wp_wasm.js";
import type { AudioCommand, AudioLibrary, FrameOut, RadioPriority } from "./types";

export const audioBridge = {
  library: (): AudioLibrary => JSON.parse(wasm.audio_library()),
  /** listener = [pos xyz, vel xyz, forward xyz, up xyz] in sim world space. */
  frame: (listener: Float32Array, eventsJson: string, focus: number): FrameOut =>
    JSON.parse(wasm.audio_frame(listener, eventsJson, focus)),
  setSiren: (entity: number, mode: string): void => wasm.audio_set_siren(entity, mode),
  setScannerTier: (tier: string): void => wasm.audio_set_scanner_tier(tier),
  /** "Radio: Important only" (police tiers 1–3) or "All" (row 144). */
  setRadioImportantOnly: (on: boolean): void => wasm.audio_set_radio_important_only(on),
  radio: (unit: number, priority: RadioPriority, text: string): void => wasm.audio_radio(unit, priority, text),
  reset: (): AudioCommand[] => JSON.parse(wasm.audio_reset()),
  /** "Physical" once the engine synthesizer runs, "Layers" otherwise. */
  setEngineModel: (mode: "Physical" | "Layers"): void => wasm.audio_set_engine_model(mode),
};
