import type { NotesUnsupportedBlockPayload } from "$lib/notes/types";

export const NOTES_UNSUPPORTED_CONVERSION_TARGETS = ["paragraph", "code"] as const;
export type NotesUnsupportedConversionTarget = (typeof NOTES_UNSUPPORTED_CONVERSION_TARGETS)[number];

function normalizedText(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

/** Return the imported block kind that should be shown to users. */
export function unsupportedBlockTypeName(payload: NotesUnsupportedBlockPayload): string {
  return normalizedText(payload.block_type) || normalizedText(payload.source_type);
}

/** Return import warnings that are safe to show in the editor. */
export function unsupportedBlockWarnings(payload: NotesUnsupportedBlockPayload): string[] {
  if (!Array.isArray(payload.warnings)) return [];
  return payload.warnings.map(normalizedText).filter(Boolean);
}

export function unsupportedBlockHasRawPayload(payload: NotesUnsupportedBlockPayload): boolean {
  return isRecord(payload.raw);
}

/** Return searchable placeholder text for an unsupported block. */
export function unsupportedBlockPlainText(payload: NotesUnsupportedBlockPayload): string {
  const importedType = unsupportedBlockTypeName(payload);
  const sourceType = normalizedText(payload.source_type);
  return [
    importedType,
    sourceType && sourceType !== importedType ? sourceType : "",
    ...unsupportedBlockWarnings(payload),
    unsupportedBlockHasRawPayload(payload) ? "raw payload preserved" : "",
  ]
    .map((part) => part.trim())
    .filter(Boolean)
    .join(" ");
}

export function unsupportedBlockSummaryText(payload: NotesUnsupportedBlockPayload): string {
  const importedType = unsupportedBlockTypeName(payload) || "unknown";
  const warnings = unsupportedBlockWarnings(payload);
  const rawStatus = unsupportedBlockHasRawPayload(payload)
    ? "Raw payload preserved"
    : "No raw payload available";
  return [
    `Unsupported block: ${importedType}`,
    ...warnings.map((warning) => `Warning: ${warning}`),
    rawStatus,
  ].join("\n");
}

export function unsupportedBlockJsonText(payload: NotesUnsupportedBlockPayload): string {
  const source = unsupportedBlockHasRawPayload(payload) ? payload.raw : payload;
  return JSON.stringify(source, null, 2);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
