import * as chatApi from "$lib/api/chat/settings";
import type { ProviderInstanceId, ProviderUpdateRead, ProviderUpdateRunRead } from "$lib/chat/contracts";
import { shouldRunAutomaticUpdateCheck, type AutomaticUpdateCheckKind } from "$lib/stores/update-policy";

/** The calls the store makes, injectable for tests. */
export interface ProviderUpdatesApi {
  checkChatProviderUpdates(): Promise<ProviderUpdateRead[]>;
  runChatProviderUpdate(instanceId: ProviderInstanceId, targetVersion: string): Promise<ProviderUpdateRunRead>;
}

/**
 * Newer provider CLI releases found by the daily check, kept in memory for Settings > Chat.
 *
 * Results are not stored, because the startup check runs again on every launch.
 */
export class ProviderUpdatesStore {
  updates = $state<ProviderUpdateRead[]>([]);
  /** The instance whose update command is running, since only one runs at a time. */
  running = $state<ProviderInstanceId | null>(null);
  private lastCheckAt: string | null = null;
  private pendingCheck: Promise<void> | null = null;
  private readonly api: ProviderUpdatesApi;

  constructor(api: ProviderUpdatesApi = chatApi) {
    this.api = api;
  }

  /**
   * The update found for `instanceId`, if any.
   *
   * @param instanceId Provider instance to look up.
   * @returns The newer release, or null when the instance is current or unchecked.
   */
  forInstance(instanceId: ProviderInstanceId): ProviderUpdateRead | null {
    return this.updates.find((update) => update.instanceId === instanceId) ?? null;
  }

  /**
   * Runs the automatic check when the update notification preference allows it.
   *
   * @param enabled Whether automatic update notifications are on.
   * @param kind Startup checks run once per launch; periodic ones at most daily.
   * @param nowMs Current wall-clock time.
   */
  async checkAutomatically(enabled: boolean, kind: AutomaticUpdateCheckKind, nowMs = Date.now()): Promise<void> {
    if (!shouldRunAutomaticUpdateCheck(enabled, this.lastCheckAt, nowMs, kind)) return;
    await this.check(nowMs);
  }

  /**
   * Checks every enabled provider now, joining a check already in flight. A failed check keeps the
   * previous results, since a release does not disappear because the network did.
   *
   * @param nowMs Current wall-clock time, recorded as the last check.
   */
  async check(nowMs = Date.now()): Promise<void> {
    if (this.pendingCheck) return this.pendingCheck;
    this.lastCheckAt = new Date(nowMs).toISOString();
    this.pendingCheck = this.api
      .checkChatProviderUpdates()
      .then((updates) => {
        this.updates = updates;
      })
      .catch((error: unknown) => {
        console.error("Failed to check provider CLI updates", error);
      })
      .finally(() => {
        this.pendingCheck = null;
      });
    return this.pendingCheck;
  }

  /**
   * Runs the provider's update command toward the release found for it, and forgets the update
   * once it succeeds.
   *
   * @param instanceId Provider instance to update.
   * @returns How the command ended.
   * @throws When no update is known for the instance, another update is running, or the command
   *   cannot start.
   */
  async run(instanceId: ProviderInstanceId): Promise<ProviderUpdateRunRead> {
    const update = this.forInstance(instanceId);
    if (!update) throw new Error("No update is known for this provider");
    if (this.running) throw new Error("A provider update is already running");
    this.running = instanceId;
    try {
      const result = await this.api.runChatProviderUpdate(instanceId, update.latestVersion);
      if (result.successful) {
        this.updates = this.updates.filter((entry) => entry.instanceId !== instanceId);
      }
      return result;
    } finally {
      this.running = null;
    }
  }
}

let store: ProviderUpdatesStore | null = null;

/** The app's shared provider update state. */
export function getProviderUpdates(): ProviderUpdatesStore {
  store ??= new ProviderUpdatesStore();
  return store;
}
