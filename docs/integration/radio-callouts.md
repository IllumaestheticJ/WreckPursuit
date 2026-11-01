# Radio callouts silent (owner report 2026-10-31, v0.14.5) — `lead/radio-callouts`

Owner: "police radio callouts aren't working again".

## What was checked

- **Sim → TS:** police `Radio` events come out of the sim in the sandbox night run, sandbox
  Free Roam and career runs. They reach `AudioSystem.update` / `audio_frame`, the director queues
  them, the scanner gets `RadioSay`, and the strip shows them. The #154 sight changes have no
  effect here: the new `wp_wasm` test below gives the same radio transcript on 89a25525 (before
  #154) and on main.
- **Web Audio:** the AudioContext runs, the Police bus user gain and gate are 1, the master is 1.
  Behind the garage only the Ui bus is open (#151's gate) and every bus opens again when the
  car drives out. Garage sounds and the radio do not fight over the context or the master.
- **Speech:** `speechSynthesis.speak` was called for every line, but after any pause **no
  utterance ever started or failed**: `speaking` and `pending` stayed true for good.

## Root cause

`AudioSystem.setPaused` (from 1e125f62 "suspend audio while paused", kept by #151's garage
rework) called `speechSynthesis.pause()` on every pause and `resume()` on unpause. Chromium's
`pause()` on an **idle** synth wedges the speech queue: every later `speak()` stays pending,
never starts and never errors, even after `resume()` (reproduced in Chromium 1194 with a plain
page: pause + resume, then speak → nothing; `cancel()` frees it). The career always goes through
the garage, which is a pause (#151 made the garage a Ui-only pause, #152 made garage → Free Roam
the normal way to play), so in the career no callout was spoken at all. In the sandbox the first
Esc menu killed the voice for the rest of the session. Squelch, static and the radio strip kept
working and nothing logged an error, which is why it looked like the radio was "not working"
rather than "muted".

## Fix (`apps/testbed-web/src/audio`)

- `index.ts` `setPaused`: a pause cancels the line being spoken (`speechSynthesis.cancel()`)
  instead of pausing the synth; unpausing calls `resume()` only if the synth is paused.
- `scanner.ts` `say`: before speaking, a paused synth is resumed and anything still queued
  (stale, or a queue some other path wedged) is cancelled. One transmission at a time was
  already the director's rule, so nothing that should be heard is dropped.
- New e2e counters: `__wpAudio.speech().reached` (utterances the engine took up: started, or
  failed for lack of a voice in headless) and `.unwedged` (stuck queues cleared).

## Tests

| What | How |
|---|---|
| Pause (Esc) then chase: callouts reach the speech engine; garage: Ui bus only, Police bus shut; career chase with a scanner after the garage: buses open, callouts said and spoken | `node apps/testbed-web/e2e/radio_callouts.mjs` (fails on main: sandbox "attempts 6, reached 0") |
| Night-run pursuit through the session API: radio events from the sim, every one said by the director's scanner | `cargo test -p wp_wasm --lib radio_tests` |

## Known gaps

- A career car **without** a police scanner mod hears no radio at all (voice or strip). That is
  the design from 360d09a3 (`docs/integration/mod-effects.md`), not part of this bug, but the
  owner may hit it in Free Roam; worth saying in the release notes.
- Headless Chromium has no voices, so the e2e proves utterances reach the engine, not that they
  sound right.

## Contract change requests

None.
