/** Reject desktop project-history UI in the mobile composition. */
export function importNotesProjectVersionHistoryModal(): Promise<
  typeof import("$lib/components/notes/history/NotesProjectVersionHistoryModal.svelte")
> {
  return Promise.reject(
    new Error("Notes project history is unavailable in the mobile composition"),
  );
}
