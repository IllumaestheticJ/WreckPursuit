// TypeScript mirror of wp_audio's JSON (crates/wp_audio/src/types.rs, data.rs).
// The Rust side decides *what* plays; this backend only decides *how* (synth or samples).

export type Bus = "Engine" | "World" | "Police" | "Ui" | "Music";
export const BUSES: Bus[] = ["Engine", "World", "Police", "Ui", "Music"];
export type Priority = "Low" | "Normal" | "High" | "Critical";
export type RadioPriority = "Chatter" | "Routine" | "Dispatch" | "Pursuit" | "Urgent";

export interface VoiceId {
  entity: number;
  slot: number;
}

/** Final per-frame voice parameters (distance gain and Doppler already applied). */
export interface VoiceParams {
  gain: number;
  pitch: number;
  freq_hz: number;
  load: number;
  phase: number;
  lowpass_hz: number;
  doppler: number;
  pos: [number, number, number];
}

export type AudioCommand =
  | {
      Play: {
        voice: VoiceId;
        bus: Bus;
        pack: string;
        cue: string;
        looped: boolean;
        priority: Priority;
        params: VoiceParams;
        synth: Record<string, number>;
      };
    }
  | { Set: { voice: VoiceId; params: VoiceParams } }
  | { Stop: { voice: VoiceId; fade_s: number } }
  | { Duck: { bus: Bus; gain: number; fade_s: number } }
  | {
      RadioSay: {
        line: number;
        unit: number;
        text: string;
        priority: RadioPriority;
        duration_s: number;
        voice_pitch: number;
      };
    }
  | { RadioEnd: { line: number; interrupted: boolean } };

export interface DirectorStats {
  looped: number;
  one_shots: number;
  per_bus: Partial<Record<Bus, number>>;
  culled: number;
  vehicles: number;
  contact_pairs: number;
  radio_queued: number;
  radio_dropped: number;
  unregistered: number;
}

export interface FrameOut {
  commands: AudioCommand[];
  stats: DirectorStats;
  /** Siren v2 acoustics (wp_audio::siren_space::SirenSpaceFrame). */
  siren?: SirenSpaceFrame;
}

/** Listener surroundings for the siren effects (wp_audio::siren_space::Room). */
export interface SirenRoom {
  slap_left_s: number;
  slap_right_s: number;
  slap_gain: [number, number];
  slap_feedback: number;
  tunnel: number;
  rt60_s: number;
  street_m: number;
}

/** One playing siren's occlusion / interior muffle (wp_audio::siren_space::VoiceSpace). */
export interface SirenVoiceSpace {
  entity: number;
  pattern: string;
  occlusion: number;
  gain: number;
  lowpass_hz: number;
  interior: boolean;
}

/** wp_audio::siren_space::SirenSpaceFrame. */
export interface SirenSpaceFrame {
  room: SirenRoom;
  voices: SirenVoiceSpace[];
  engine_duck: number;
  radio_duck: number;
}

export type CueSource =
  | { Samples: { files: string[] } }
  | { Synth: { synth: string; params: Record<string, number> } };

export interface CueDef {
  source: CueSource;
  gain: number;
  looped: boolean;
  duration_s?: number | null;
  root_speed_mps?: number | null;
}

export interface SamplePack {
  id: string;
  kind: "Engine" | "World" | "Siren" | "Radio";
  description: string;
  /** Procedural pack played instead when the user picks the procedural fallback. */
  fallback?: string;
  cues: Record<string, CueDef>;
}

export interface AudioLibrary {
  config: { max_voices: number; bus_max_voices: Partial<Record<Bus, number>> };
  packs: Record<string, SamplePack>;
}

export const voiceKey = (v: VoiceId): string => `${v.entity}:${v.slot}`;

/** Where cue audio comes from: recorded samples (default) or the procedural fallback packs. */
export type SoundSource = "recorded" | "procedural";

/** Sample loading progress (panel + e2e). */
export interface SampleStatus {
  total: number;
  loaded: number;
  loading: number;
  failed: string[];
}
