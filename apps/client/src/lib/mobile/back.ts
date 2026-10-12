import type { View } from "$lib/navigation";

export type MobileBackAction =
  | "consume-feature-layer"
  | "close-nested-route"
  | "navigate-calendar"
  | "release-to-system";

export interface MobileBackState {
  featureLayerOpen: boolean;
  nestedRouteOpen: boolean;
  currentView: View;
}

export interface MobileBackListener {
  unregister(): Promise<void>;
}

export type MobileBackListenerFactory = (
  handler: () => void,
) => Promise<MobileBackListener>;

/**
 * Keep Tauri's Android Back listener installed only while the app can consume Back.
 *
 * Tauri delegates to Android only when no JavaScript listener exists, so the true
 * app root must release the listener instead of trying to return a value from it.
 */
export class MobileBackListenerController {
  readonly #factory: MobileBackListenerFactory;
  readonly #handler: () => void;
  readonly #onError: (error: unknown) => void;
  #desired = false;
  #disposed = false;
  #listener: MobileBackListener | null = null;
  #operation: Promise<void> = Promise.resolve();

  constructor(
    factory: MobileBackListenerFactory,
    handler: () => void,
    onError: (error: unknown) => void,
  ) {
    this.#factory = factory;
    this.#handler = handler;
    this.#onError = onError;
  }

  /** Reconcile whether the app should intercept Android Back. */
  setEnabled(enabled: boolean): Promise<void> {
    if (this.#disposed) return this.#operation;
    this.#desired = enabled;
    return this.#enqueueReconcile();
  }

  /** Release the native listener and ignore future state changes. */
  dispose(): Promise<void> {
    this.#disposed = true;
    this.#desired = false;
    return this.#enqueueReconcile();
  }

  #enqueueReconcile(): Promise<void> {
    this.#operation = this.#operation
      .then(() => this.#reconcile())
      .catch((error: unknown) => {
        this.#onError(error);
      });
    return this.#operation;
  }

  async #reconcile(): Promise<void> {
    const shouldListen = this.#desired && !this.#disposed;
    if (shouldListen && !this.#listener) {
      const listener = await this.#factory(this.#handler);
      if (this.#desired && !this.#disposed) {
        this.#listener = listener;
      } else {
        await listener.unregister();
      }
      return;
    }

    if (!shouldListen && this.#listener) {
      const listener = this.#listener;
      this.#listener = null;
      await listener.unregister();
    }
  }
}

/** Choose one deterministic action for an Android system-back press. */
export function resolveMobileBackAction(state: MobileBackState): MobileBackAction {
  if (state.featureLayerOpen) return "consume-feature-layer";
  if (state.nestedRouteOpen) return "close-nested-route";
  if (state.currentView !== "calendar") return "navigate-calendar";
  return "release-to-system";
}
