import {
  buildNotesBlockLink,
  buildNotesPageLink,
  isNotesUuid,
} from "$lib/notes/links/block-link";

export type NotesRoundTripDiagnosticSeverity = "info" | "warning" | "error";

export type NotesRoundTripDiagnosticCategory =
  | "preserved"
  | "approximated"
  | "skipped"
  | "unsupported"
  | "error"
  | "warning"
  | "info";

export interface NotesRoundTripDiagnosticInput {
  code: string;
  severity: NotesRoundTripDiagnosticSeverity;
  message: string;
  sourceLabel?: string | null;
  sourceHref?: string | null;
}

export interface NotesRoundTripDiagnosticItem {
  code: string;
  severity: NotesRoundTripDiagnosticSeverity;
  category: NotesRoundTripDiagnosticCategory;
  message: string;
  sourceLabel: string | null;
  sourceHref: string | null;
}

export interface NotesRoundTripCountItem {
  id: string;
  label: string;
  value: number;
}

export interface NotesRoundTripCategoryCount {
  category: NotesRoundTripDiagnosticCategory;
  count: number;
}

const ROUND_TRIP_CATEGORY_ORDER: NotesRoundTripDiagnosticCategory[] = [
  "error",
  "unsupported",
  "skipped",
  "approximated",
  "preserved",
  "warning",
  "info",
];

/**
 * Classify a backend diagnostic by the round-trip data consequence visible to the user.
 */
export function classifyRoundTripDiagnostic(
  code: string,
  severity: NotesRoundTripDiagnosticSeverity,
  message = "",
): NotesRoundTripDiagnosticCategory {
  const text = `${code} ${message}`.toLowerCase();
  if (text.includes("unsupported")) return "unsupported";
  if (text.includes("approximated") || text.includes("approximation")) return "approximated";
  if (
    text.includes("skipped")
    || text.includes("excluded")
    || text.includes("omitted")
    || text.includes("not_exported")
    || text.includes("not_included")
    || text.includes("missing")
    || text.includes("invalid_cell")
  ) {
    return "skipped";
  }
  if (
    text.includes("preserved")
    || text.includes("preservation")
    || text.includes("external_reference")
    || text.includes("external file")
    || text.includes("raw payload")
    || text.includes("copied")
  ) {
    return "preserved";
  }
  return severity;
}

/**
 * Normalize a backend diagnostic into the shared round-trip presentation shape.
 */
export function toRoundTripDiagnosticItem(
  diagnostic: NotesRoundTripDiagnosticInput,
): NotesRoundTripDiagnosticItem {
  return {
    code: diagnostic.code,
    severity: diagnostic.severity,
    category: classifyRoundTripDiagnostic(
      diagnostic.code,
      diagnostic.severity,
      diagnostic.message,
    ),
    message: diagnostic.message,
    sourceLabel: diagnostic.sourceLabel?.trim() || null,
    sourceHref: diagnostic.sourceHref?.trim() || null,
  };
}

/**
 * Count diagnostic categories in the display order used by the summary panel.
 */
export function roundTripCategoryCounts(
  diagnostics: NotesRoundTripDiagnosticItem[],
): NotesRoundTripCategoryCount[] {
  const counts = new Map<NotesRoundTripDiagnosticCategory, number>();
  for (const diagnostic of diagnostics) {
    counts.set(diagnostic.category, (counts.get(diagnostic.category) ?? 0) + 1);
  }
  return ROUND_TRIP_CATEGORY_ORDER
    .map((category) => ({ category, count: counts.get(category) ?? 0 }))
    .filter((entry) => entry.count > 0);
}

/**
 * Count warning and error diagnostics for completion summaries.
 */
export function roundTripWarningCount(
  diagnostics: Pick<NotesRoundTripDiagnosticItem, "severity">[],
): number {
  return diagnostics.filter((diagnostic) => diagnostic.severity !== "info").length;
}

/**
 * Build a local Notes link when the source can be navigated in the current app.
 */
export function buildRoundTripNotesSourceHref(
  baseHref: string,
  source: {
    pageId?: string | null;
    blockId?: string | null;
  },
): string | null {
  if (!isNotesUuid(source.pageId)) return null;
  try {
    if (isNotesUuid(source.blockId)) {
      return buildNotesBlockLink(baseHref, {
        pageId: source.pageId,
        blockId: source.blockId,
      });
    }
    return buildNotesPageLink(baseHref, { pageId: source.pageId });
  } catch {
    return null;
  }
}

export function visibleRoundTripCounts(
  counts: NotesRoundTripCountItem[],
): NotesRoundTripCountItem[] {
  return counts.filter((count) => Number.isFinite(count.value) && count.value >= 0);
}
