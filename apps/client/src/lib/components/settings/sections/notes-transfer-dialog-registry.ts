import {
  createLazyComponentLoader,
  type LazyComponentImporter,
} from "$lib/lazy-component-loader";
import type { NotesTransferOperation } from "$lib/settings/types";

export type LoadedNotesTransferDialog =
  | {
    operation: "html-import";
    component: typeof import("$lib/components/notes/transfer/NotesHtmlImportDialog.svelte").default;
  }
  | {
    operation: "notion-api-import";
    component: typeof import("$lib/components/notes/transfer/NotesNotionApiImportDialog.svelte").default;
  }
  | {
    operation: "notion-export-import";
    component: typeof import("$lib/components/notes/transfer/NotesNotionExportImportDialog.svelte").default;
  }
  | {
    operation: "json-graph-export";
    component: typeof import("$lib/components/notes/transfer/NotesJsonGraphExportDialog.svelte").default;
  };

const DIALOG_IMPORTERS = {
  "html-import": () => import("$lib/components/notes/transfer/NotesHtmlImportDialog.svelte")
    .then((module) => ({
      default: { operation: "html-import" as const, component: module.default },
    })),
  "notion-api-import": () => import("$lib/components/notes/transfer/NotesNotionApiImportDialog.svelte")
    .then((module) => ({
      default: { operation: "notion-api-import" as const, component: module.default },
    })),
  "notion-export-import": () => import("$lib/components/notes/transfer/NotesNotionExportImportDialog.svelte")
    .then((module) => ({
      default: { operation: "notion-export-import" as const, component: module.default },
    })),
  "json-graph-export": () => import("$lib/components/notes/transfer/NotesJsonGraphExportDialog.svelte")
    .then((module) => ({
      default: { operation: "json-graph-export" as const, component: module.default },
    })),
} satisfies Readonly<
  Record<NotesTransferOperation, LazyComponentImporter<LoadedNotesTransferDialog>>
>;

const loader = createLazyComponentLoader<NotesTransferOperation, LoadedNotesTransferDialog>(
  DIALOG_IMPORTERS,
);

/** Loads and caches one Notes transfer dialog constructor. */
export function loadNotesTransferDialog(
  operation: NotesTransferOperation,
): Promise<LoadedNotesTransferDialog> {
  return loader.load(operation);
}

/** Retries a failed Notes transfer dialog import. */
export function retryNotesTransferDialog(
  operation: NotesTransferOperation,
): Promise<LoadedNotesTransferDialog> {
  return loader.retry(operation);
}

/** Reports whether a Notes transfer dialog constructor is already cached. */
export function notesTransferDialogHasLoaded(operation: NotesTransferOperation): boolean {
  return loader.hasLoaded(operation);
}
