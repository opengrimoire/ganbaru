function unsupportedMobileExport<T>(): Promise<T> {
  return Promise.reject(new Error("Notes file export is unavailable in the mobile composition"));
}

export function importNotesHtmlExportDialog(): Promise<
  typeof import("$lib/components/notes/transfer/NotesHtmlExportDialog.svelte")
> {
  return unsupportedMobileExport();
}

export function importNotesAgentBridgeExportDialog(): Promise<
  typeof import("$lib/components/notes/transfer/NotesAgentBridgeExportDialog.svelte")
> {
  return unsupportedMobileExport();
}

export function importNotesDatabaseCsvExportPanel(): Promise<
  typeof import("$lib/components/notes/database/NotesDatabaseCsvExportPanel.svelte")
> {
  return unsupportedMobileExport();
}
