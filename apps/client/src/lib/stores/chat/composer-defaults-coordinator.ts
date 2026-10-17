import {
  composerModelSelectionsEqual,
  resolveComposerDefaults,
} from "$lib/chat/composer/defaults";
import type { ChatComposerSnapshot } from "$lib/chat/composer/controller";
import type {
  ChatSettingsRead,
  InteractionMode,
  ProviderInstanceId,
  SafetyMode,
  VersionedJson,
} from "$lib/chat/contracts";

export interface ChatComposerDefaultsCoordinatorOptions {
  settings: () => ChatSettingsRead | null;
  composer: () => ChatComposerSnapshot;
  setProvider: (providerInstanceId: ProviderInstanceId | null) => void;
  setModel: (modelSelection: VersionedJson | null) => void;
  setModes: (
    safetyMode: SafetyMode | null,
    interactionMode: InteractionMode | null,
  ) => void;
  flush: () => Promise<void>;
}

/** Reconciles persisted provider defaults after settings or draft changes. */
export class ChatComposerDefaultsCoordinator {
  private queued = false;

  constructor(private readonly options: ChatComposerDefaultsCoordinatorOptions) {}

  schedule(): void {
    if (this.queued) return;
    this.queued = true;
    queueMicrotask(() => {
      this.queued = false;
      this.apply();
    });
  }

  private apply(): void {
    const settings = this.options.settings();
    const snapshot = this.options.composer();
    if (!settings || snapshot.loading) return;
    const defaults = resolveComposerDefaults(settings, snapshot);
    if (!defaults) return;
    let changed = false;
    if (snapshot.providerInstanceId !== defaults.providerInstanceId) {
      this.options.setProvider(defaults.providerInstanceId);
      changed = true;
    }
    if (!composerModelSelectionsEqual(snapshot.modelSelection, defaults.modelSelection)) {
      this.options.setModel(defaults.modelSelection);
      changed = true;
    }
    if (snapshot.safetyMode !== defaults.safetyMode
      || snapshot.interactionMode !== defaults.interactionMode) {
      this.options.setModes(defaults.safetyMode, defaults.interactionMode);
      changed = true;
    }
    if (!changed) return;
    void this.options.flush().catch((error: unknown) => {
      console.error("Chat composer defaults could not be saved", error);
    });
  }
}
