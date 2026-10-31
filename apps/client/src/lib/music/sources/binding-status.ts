import type { LocalRootBinding, LocalRootBindingStatus, MusicItemListEntry } from "$lib/music/library/contracts";

/**
 * Reports the binding status of the local root an item belongs to on this device.
 *
 * Returns `null` when the item has no local root, when its root is bound and available, or when this device has
 * not read a binding for the root yet. `unbound` means the root arrived from a linked device and no folder was
 * chosen here, so its items are listed but cannot play on this device.
 */
export function itemBindingProblem(
  item: Pick<MusicItemListEntry, "sourceKind" | "localRootId">,
  bindings: readonly LocalRootBinding[],
): Exclude<LocalRootBindingStatus, "available"> | null {
  if (item.sourceKind !== "local-file" || !item.localRootId) return null;
  const status = bindings.find((binding) => binding.rootId === item.localRootId)?.status;
  return status && status !== "available" ? status : null;
}
