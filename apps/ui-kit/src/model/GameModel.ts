/**
 * `GameModel`: the single observable interface every screen reads from.
 *
 * A provider implements it. The kit ships the mock provider (`createMockModel`); the real one
 * (wp_wasm in the browser, wp_net on a client) implements the same three methods.
 */
import { createContext } from "preact";
import { useContext, useEffect, useRef, useState } from "preact/hooks";
import type { UiCommand } from "./commands";
import type { UiState } from "./types";

/** The one interface between the UI and the game. */
export interface GameModel {
  /** Current state snapshot. Treat as immutable: providers replace objects, never mutate. */
  get(): UiState;
  /** Called after every state change. Returns an unsubscribe function. */
  subscribe(listener: (state: UiState) => void): () => void;
  /** Request an action. The provider validates it and updates the state (or sets `notice`). */
  send(command: UiCommand): void;
}

/**
 * Minimal observable store used by providers. `update` replaces the state and notifies
 * listeners synchronously.
 */
export class Store<S> {
  private state: S;
  private listeners = new Set<(s: S) => void>();

  /** New store with an initial state. */
  constructor(initial: S) {
    this.state = initial;
  }

  /** Current state. */
  get(): S {
    return this.state;
  }

  /** Replace the state (via a function of the previous one) and notify. */
  update(fn: (s: S) => S): void {
    const next = fn(this.state);
    if (next === this.state) return;
    this.state = next;
    for (const l of this.listeners) l(next);
  }

  /** Listen for changes. */
  subscribe(l: (s: S) => void): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }
}

/** Preact context carrying the model to every screen. */
export const ModelContext = createContext<GameModel | null>(null);

/** The model from context (throws outside a `ModelContext.Provider`). */
export function useModel(): GameModel {
  const m = useContext(ModelContext);
  if (!m) throw new Error("useModel: no GameModel in context (mount via mountUi)");
  return m;
}

/**
 * Subscribe to a slice of the state. Re-renders only when the selected value changes by
 * reference (providers replace changed slices, so this is cheap).
 */
export function useSlice<T>(select: (s: UiState) => T): T {
  const model = useModel();
  const [value, setValue] = useState(() => select(model.get()));
  const selectRef = useRef(select);
  selectRef.current = select;
  useEffect(() => {
    // Catch a change between render and subscribe.
    setValue(() => selectRef.current(model.get()));
    return model.subscribe((s) => {
      const v = selectRef.current(s);
      setValue((prev) => (Object.is(prev, v) ? prev : v));
    });
  }, [model]);
  return value;
}

/** `send` bound to the model in context. */
export function useSend(): (c: UiCommand) => void {
  const model = useModel();
  return (c) => model.send(c);
}
