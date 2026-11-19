import type { ProviderInstanceId, ProviderInstanceRead, ProviderUpdateRead } from "$lib/chat/contracts";
import { providersInPickerOrder } from "$lib/chat/composer/model-picker-model";
import type { UpdateStatus } from "$lib/stores/updates.svelte";

/** A provider CLI update paired with the configuration it belongs to. */
export interface CliUpdateEntry {
  provider: ProviderInstanceRead;
  update: ProviderUpdateRead;
}

/**
 * Whether the app update belongs in the updates panel: from the moment a release is found until
 * the app restarts into it.
 *
 * @param status The app updater's current state.
 */
export function appUpdatePending(status: UpdateStatus): boolean {
  return status === "available" || status === "downloading" || status === "installed";
}

/**
 * Pairs each CLI update with its provider configuration, in model picker order, dropping updates
 * whose configuration no longer exists.
 *
 * @param updates Updates from the latest provider check.
 * @param providers Configured provider instances.
 * @param nowMs Current instant, which decides plan-limit readiness in the order.
 */
export function cliUpdateEntries(
  updates: readonly ProviderUpdateRead[],
  providers: readonly ProviderInstanceRead[],
  nowMs: number,
): CliUpdateEntry[] {
  const byInstance = new Map(updates.map((update) => [update.instanceId, update]));
  return providersInPickerOrder(providers, nowMs).flatMap((provider) => {
    const update = byInstance.get(provider.configuration.instanceId);
    return update ? [{ provider, update }] : [];
  });
}

/**
 * The provider instances whose CLI update Ganbaru can run itself, in panel order.
 *
 * @param entries The CLI updates the panel lists.
 */
export function runnableCliUpdates(entries: readonly CliUpdateEntry[]): ProviderInstanceId[] {
  return entries.filter((entry) => entry.update.runsInApp).map((entry) => entry.update.instanceId);
}

/**
 * Whether the panel offers one action that applies every update, which pays off once at least two updates can run
 * without a terminal.
 *
 * @param appSelfUpdate Whether the app update installs itself from the panel.
 * @param runnableCliCount How many CLI updates Ganbaru can run itself.
 */
export function offersUpdateAll(appSelfUpdate: boolean, runnableCliCount: number): boolean {
  return runnableCliCount + (appSelfUpdate ? 1 : 0) >= 2;
}
