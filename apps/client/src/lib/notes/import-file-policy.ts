import type { NotesMediaBlockType } from "$lib/notes/block-types/media";

export const NOTES_IMPORT_FILE_CONTEXTS = [
  "notion_export",
  "html_import",
  "markdown_import",
] as const;

export const NOTES_IMPORT_FILE_CHOICES = [
  "copy_local_file",
  "keep_external_reference",
  "skip",
] as const;

export type NotesImportFileContext = (typeof NOTES_IMPORT_FILE_CONTEXTS)[number];
export type NotesImportFileChoice = (typeof NOTES_IMPORT_FILE_CHOICES)[number];
export type NotesImportFileDiagnosticSeverity = "info" | "warning" | "error";
export type NotesImportFileReferenceKind =
  | "empty"
  | "external_url"
  | "managed_asset"
  | "local_file";

export type NotesImportFileAction =
  | "copied_asset"
  | "external_reference"
  | "skipped"
  | "blocked";

export interface NotesImportFileReferenceRequest {
  importContext: NotesImportFileContext;
  choice: NotesImportFileChoice;
  reference: string;
  importRoot?: string | null;
  blockType?: NotesMediaBlockType | null;
  originalName?: string | null;
}

export interface NotesImportFileDiagnostic {
  code: string;
  severity: NotesImportFileDiagnosticSeverity;
  message: string;
}

export function classifyNotesImportFileReference(
  reference: string,
): NotesImportFileReferenceKind {
  const trimmed = reference.trim();
  if (!trimmed) return "empty";
  const lower = trimmed.toLowerCase();
  if (lower.startsWith("ganbaru-asset:")) return "managed_asset";
  if (lower.startsWith("http://") || lower.startsWith("https://")) return "external_url";
  return "local_file";
}

export function notesImportFileChoicesForReference(
  reference: string,
  hasImportRoot: boolean,
): NotesImportFileChoice[] {
  const kind = classifyNotesImportFileReference(reference);
  if (kind === "empty" || kind === "managed_asset") return ["skip"];
  if (kind === "external_url") return ["keep_external_reference", "skip"];
  return hasImportRoot ? ["copy_local_file", "skip"] : ["skip"];
}
