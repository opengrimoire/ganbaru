/**
 * Returns the group whose projects the Projects sidebar lists: the group the person browsed to while it still exists,
 * otherwise the selected project's group, otherwise the first visible group.
 */
export function resolveProjectSidebarGroupId(
  browsedGroupId: string | null,
  selectedGroupId: string | null,
  visibleGroupIds: readonly string[],
): string | null {
  if (browsedGroupId && visibleGroupIds.includes(browsedGroupId)) return browsedGroupId;
  if (selectedGroupId && visibleGroupIds.includes(selectedGroupId)) return selectedGroupId;
  return visibleGroupIds[0] ?? null;
}
