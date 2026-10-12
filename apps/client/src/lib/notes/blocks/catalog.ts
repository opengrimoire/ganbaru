import { NOTES_BLOCK_TYPES, type NotesBlockType } from "$lib/notes/types";

export const NOTES_BLOCK_CATALOG_GATE_REQUIREMENTS = [
  "rich_editor_p0_complete",
  "current_block_quality_complete",
  "honest_docs",
  "usable_editing_ui",
  "persistence",
  "focused_tests",
  "validation_gate",
] as const;

export type NotesBlockCatalogGateRequirement =
  (typeof NOTES_BLOCK_CATALOG_GATE_REQUIREMENTS)[number];

export type NotesBlockCatalogGateEvidence = Record<NotesBlockCatalogGateRequirement, boolean>;

export const NOTES_BLOCK_CATALOG_BLOCKED_FUTURE_TYPES = [
  "meeting_notes",
  "transcription",
] as const;

export type NotesBlockCatalogBlockedFutureType =
  (typeof NOTES_BLOCK_CATALOG_BLOCKED_FUTURE_TYPES)[number];

export type NotesBlockCatalogGateReason =
  | "already_registered"
  | "blocked_future_type"
  | "missing_requirement"
  | "ready_for_registration";

export interface NotesBlockCatalogGateInput {
  blockType: string;
  evidence: NotesBlockCatalogGateEvidence;
}

export interface NotesBlockCatalogGateDecision {
  blockType: string;
  allowed: boolean;
  reason: NotesBlockCatalogGateReason;
  missingRequirements: NotesBlockCatalogGateRequirement[];
}

const REGISTERED_BLOCK_TYPES = new Set<string>(NOTES_BLOCK_TYPES);
const BLOCKED_FUTURE_BLOCK_TYPES = new Set<string>(NOTES_BLOCK_CATALOG_BLOCKED_FUTURE_TYPES);

/** Return the block types that are already registered in the local Notes catalog. */
export function notesBlockCatalogRegisteredTypes(): readonly NotesBlockType[] {
  return NOTES_BLOCK_TYPES;
}

/** Check whether a value is a block type that the current local editor can persist and render. */
export function isNotesCatalogRegisteredBlockType(value: unknown): value is NotesBlockType {
  return typeof value === "string" && REGISTERED_BLOCK_TYPES.has(value);
}

/** Check whether a Notion block type is explicitly known but not yet shippable locally. */
export function isNotesCatalogBlockedFutureType(
  value: unknown,
): value is NotesBlockCatalogBlockedFutureType {
  return typeof value === "string" && BLOCKED_FUTURE_BLOCK_TYPES.has(value);
}

/** Build complete gate evidence for tests and future catalog registration reviews. */
export function completeNotesBlockCatalogGateEvidence(): NotesBlockCatalogGateEvidence {
  return {
    rich_editor_p0_complete: true,
    current_block_quality_complete: true,
    honest_docs: true,
    usable_editing_ui: true,
    persistence: true,
    focused_tests: true,
    validation_gate: true,
  };
}

/** Decide whether a block type can be registered as a shippable local block type. */
export function notesBlockCatalogGateDecision(
  input: NotesBlockCatalogGateInput,
): NotesBlockCatalogGateDecision {
  const blockType = input.blockType.trim();
  if (isNotesCatalogRegisteredBlockType(blockType)) {
    return {
      blockType,
      allowed: true,
      reason: "already_registered",
      missingRequirements: [],
    };
  }

  const missingRequirements = NOTES_BLOCK_CATALOG_GATE_REQUIREMENTS.filter(
    (requirement) => !input.evidence[requirement],
  );
  if (missingRequirements.length > 0) {
    return {
      blockType,
      allowed: false,
      reason: isNotesCatalogBlockedFutureType(blockType)
        ? "blocked_future_type"
        : "missing_requirement",
      missingRequirements,
    };
  }

  return {
    blockType,
    allowed: true,
    reason: "ready_for_registration",
    missingRequirements: [],
  };
}
