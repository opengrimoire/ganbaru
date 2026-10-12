/** Track the open block context menu while rows mount and unmount. */
export function createNotesBlockHandleController() {
  let openMenuBlockId = $state<string | null>(null);

  return {
    get openMenuBlockId() {
      return openMenuBlockId;
    },
    setMenuOpen(blockId: string, open: boolean): void {
      if (open) {
        openMenuBlockId = blockId;
        return;
      }
      if (openMenuBlockId === blockId) openMenuBlockId = null;
    },
  };
}
