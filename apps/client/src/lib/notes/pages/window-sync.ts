import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";

const pageChanges = createVaultChangeChannel("notes-page-changed");

/** Tell other windows of this vault that the content of these pages changed. */
export function publishNotesPageChanges(pageIds: Iterable<string>): void {
  pageChanges.publish(pageIds);
}

/** Receive the ids of pages other windows changed, gathered per pause in their editing. */
export function listenForNotesPageChanges(onChange: (pageIds: ReadonlySet<string>) => void): Promise<() => void> {
  return pageChanges.listen(onChange);
}
