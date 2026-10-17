import { parseNotesBlock } from "$lib/notes/blocks/validation";
import {
  childIdsForParent,
  parentIdForBlock,
  type NotesTreeState,
} from "$lib/notes/blocks/tree";
import { cloneNotesJson } from "$lib/notes/json-clone";
import type { NotesDocumentSelection, NotesTextSelection } from "$lib/notes/editor/selection";
import type { NotesBlock } from "$lib/notes/types";

export type NotesUndoKind =
  | "typing"
  | "formatting"
  | "link"
  | "mention"
  | "equation"
  | "paste"
  | "create"
  | "convert"
  | "move"
  | "duplicate"
  | "delete"
  | "template"
  | "button"
  | "update";

const NOTES_UNDO_KINDS: readonly NotesUndoKind[] = [
  "typing",
  "formatting",
  "link",
  "mention",
  "equation",
  "paste",
  "create",
  "convert",
  "move",
  "duplicate",
  "delete",
  "template",
  "button",
  "update",
];

export interface NotesUndoSnapshot {
  /** Associates one historical boundary with its bounded canonical editor receipt. */
  nativeEditId?: string;
  pageId: string;
  blocks: NotesBlock[];
  childIdsByParentId: Record<string, string[]>;
  focusBlockId: string | null;
  focusSelection: NotesTextSelection | null;
  documentSelection?: NotesDocumentSelection | null;
}

export interface NotesUndoEntry {
  id: string;
  kind: NotesUndoKind;
  groupKey: string | null;
  before: NotesUndoSnapshot;
  after: NotesUndoSnapshot;
  createdAt: number;
  updatedAt: number;
}

export interface NotesUndoState {
  undo: NotesUndoEntry[];
  redo: NotesUndoEntry[];
}

export interface NotesUndoShortcutInput {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

export interface NotesUndoRecordOptions {
  id: string;
  kind: NotesUndoKind;
  before: NotesUndoSnapshot | null;
  after: NotesUndoSnapshot | null;
  groupKey?: string | null;
  now?: number;
}

export const NOTES_UNDO_SCHEMA_VERSION = 1;
export const NOTES_UNDO_STACK_LIMIT = 40;
export const NOTES_UNDO_GROUP_MS = 1250;
export const NOTES_UNDO_STATE_MAX_BYTES = 512 * 1024;

interface SerializedNotesUndoState {
  schema_version: typeof NOTES_UNDO_SCHEMA_VERSION;
  undo: NotesUndoEntry[];
  redo: NotesUndoEntry[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readString(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`${label} must be a string`);
  return value;
}

function readNumber(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`${label} must be a finite number`);
  }
  return value;
}

function readNullableString(value: unknown, label: string): string | null {
  if (value === null) return null;
  return readString(value, label);
}

function readUndoKind(value: unknown, label: string): NotesUndoKind {
  const kind = readString(value, label);
  if (!NOTES_UNDO_KINDS.includes(kind as NotesUndoKind)) {
    throw new Error(`${label} is not a supported undo kind`);
  }
  return kind as NotesUndoKind;
}

function parseSnapshotBlock(value: unknown, label: string): NotesBlock {
  try {
    return parseNotesBlock(value);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`${label}: ${message}`);
  }
}

function cloneBlock(block: NotesBlock): NotesBlock {
  return parseNotesBlock(cloneNotesJson(block));
}

function cloneSnapshot(snapshot: NotesUndoSnapshot): NotesUndoSnapshot {
  return {
    ...(snapshot.nativeEditId ? { nativeEditId: snapshot.nativeEditId } : {}),
    pageId: snapshot.pageId,
    blocks: snapshot.blocks.map(cloneBlock),
    childIdsByParentId: Object.fromEntries(
      Object.entries(snapshot.childIdsByParentId).map(([parentId, childIds]) => [
        parentId,
        [...childIds],
      ]),
    ),
    focusBlockId: snapshot.focusBlockId,
    focusSelection: snapshot.focusSelection ? { ...snapshot.focusSelection } : null,
    ...(snapshot.documentSelection ? {
      documentSelection: {
        anchor: { ...snapshot.documentSelection.anchor },
        focus: { ...snapshot.documentSelection.focus },
      },
    } : {}),
  };
}

function snapshotContentKey(snapshot: NotesUndoSnapshot): string {
  return JSON.stringify({
    pageId: snapshot.pageId,
    blocks: snapshot.blocks,
    childIdsByParentId: snapshot.childIdsByParentId,
  });
}

function snapshotContentsEqual(
  left: NotesUndoSnapshot | null,
  right: NotesUndoSnapshot | null,
): boolean {
  if (!left || !right) return left === right;
  return snapshotContentKey(left) === snapshotContentKey(right);
}

function mergeSnapshotChanges(
  existing: NotesUndoSnapshot,
  next: NotesUndoSnapshot,
): NotesUndoSnapshot {
  const nextBlockIds = new Set(next.blocks.map((block) => block.id));
  return cloneSnapshot({
    pageId: next.pageId,
    blocks: [
      ...existing.blocks.filter((block) => !nextBlockIds.has(block.id)),
      ...next.blocks,
    ],
    childIdsByParentId: {
      ...existing.childIdsByParentId,
      ...next.childIdsByParentId,
    },
    focusBlockId: next.focusBlockId,
    focusSelection: next.focusSelection,
    documentSelection: next.documentSelection,
  });
}

/**
 * Derive the caret ranges before and after one controlled plain-text edit.
 */
export function notesTextChangeUndoSelections(
  beforeText: string,
  afterText: string,
  afterSelection: NotesTextSelection | null,
): { before: NotesTextSelection; after: NotesTextSelection } {
  let prefixLength = 0;
  const sharedLength = Math.min(beforeText.length, afterText.length);
  while (
    prefixLength < sharedLength
    && beforeText[prefixLength] === afterText[prefixLength]
  ) {
    prefixLength += 1;
  }
  let suffixLength = 0;
  while (
    suffixLength < beforeText.length - prefixLength
    && suffixLength < afterText.length - prefixLength
    && beforeText[beforeText.length - suffixLength - 1]
      === afterText[afterText.length - suffixLength - 1]
  ) {
    suffixLength += 1;
  }
  const fallbackAfterOffset = afterText.length - suffixLength;
  return {
    before: {
      start: prefixLength,
      end: beforeText.length - suffixLength,
    },
    after: afterSelection
      ? { ...afterSelection }
      : { start: fallbackAfterOffset, end: fallbackAfterOffset },
  };
}

function collectSnapshotBlockIds(
  state: NotesTreeState,
  pageId: string,
  extraBlocks: readonly NotesBlock[],
): string[] {
  const ids: string[] = [];
  const visited = new Set<string>();
  const visit = (parentId: string) => {
    for (const childId of childIdsForParent(state, parentId)) {
      if (visited.has(childId)) continue;
      const block = state.blocksById[childId];
      if (!block) continue;
      visited.add(childId);
      ids.push(childId);
      visit(childId);
    }
  };
  visit(pageId);
  for (const block of extraBlocks) {
    if (visited.has(block.id)) continue;
    visited.add(block.id);
    ids.push(block.id);
  }
  return ids;
}

export function createNotesUndoSnapshot(
  pageId: string | null,
  state: NotesTreeState,
  focusBlockId: string | null,
  extraBlocks: readonly NotesBlock[] = [],
  focusSelection: NotesTextSelection | null = null,
): NotesUndoSnapshot | null {
  if (!pageId) return null;
  const extraBlocksById = Object.fromEntries(extraBlocks.map((block) => [block.id, block]));
  const childIdsByParentId = Object.fromEntries(Object.entries(state.childIdsByParentId)
    .map(([parentId, ids]) => [parentId, [...ids]]));
  for (const block of extraBlocks) {
    const parentId = parentIdForBlock(block);
    for (const [key, ids] of Object.entries(childIdsByParentId)) {
      if (key !== parentId) childIdsByParentId[key] = ids.filter((id) => id !== block.id);
    }
    const siblings = childIdsByParentId[parentId] ?? [];
    if (!siblings.includes(block.id)) siblings.push(block.id);
    childIdsByParentId[parentId] = siblings;
  }
  const mergedState: NotesTreeState = {
    blocksById: { ...state.blocksById, ...extraBlocksById },
    childIdsByParentId,
  };
  const blockIds = collectSnapshotBlockIds(mergedState, pageId, extraBlocks);
  return {
    pageId,
    blocks: blockIds
      .map((blockId) => mergedState.blocksById[blockId])
      .filter((block): block is NotesBlock => block !== undefined)
      .map(cloneBlock),
    childIdsByParentId: Object.fromEntries(
      Object.entries(mergedState.childIdsByParentId)
        .filter(([parentId, childIds]) =>
          parentId === pageId
          || blockIds.includes(parentId)
          || childIds.some((childId) => blockIds.includes(childId))
        )
        .map(([parentId, childIds]) => [
          parentId,
          childIds.filter((childId) => blockIds.includes(childId)),
        ]),
    ),
    focusBlockId,
    focusSelection: focusSelection ? { ...focusSelection } : null,
  };
}

/**
 * Capture only blocks affected by a local editor operation.
 */
export function createNotesUndoSnapshotForBlocks(
  pageId: string | null,
  state: NotesTreeState,
  focusBlockId: string | null,
  blockIds: readonly string[],
  extraBlocks: readonly NotesBlock[] = [],
  focusSelection: NotesTextSelection | null = null,
): NotesUndoSnapshot | null {
  if (!pageId) return null;
  const extraBlocksById = new Map(extraBlocks.map((block) => [block.id, block]));
  const uniqueBlockIds = [...new Set(blockIds)];
  return {
    pageId,
    blocks: uniqueBlockIds
      .map((blockId) => extraBlocksById.get(blockId) ?? state.blocksById[blockId])
      .filter((block): block is NotesBlock => block !== undefined)
      .map(cloneBlock),
    childIdsByParentId: Object.fromEntries(
      Object.entries(state.childIdsByParentId)
        .filter(([, ids]) => ids.some((id) => uniqueBlockIds.includes(id)))
        .map(([parentId, ids]) => [parentId, [...ids]]),
    ),
    focusBlockId,
    focusSelection: focusSelection ? { ...focusSelection } : null,
  };
}

export function recordNotesUndoEntry(
  state: NotesUndoState,
  options: NotesUndoRecordOptions,
): NotesUndoState {
  const { before, after } = options;
  if (!before || !after || snapshotContentsEqual(before, after)) return state;
  const now = options.now ?? Date.now();
  const groupKey = options.groupKey ?? null;
  const latest = state.undo.at(-1);
  if (
    latest
    && latest.groupKey !== null
    && latest.groupKey === groupKey
    && latest.kind === options.kind
    && now - latest.updatedAt <= NOTES_UNDO_GROUP_MS
  ) {
    return {
      undo: [
        ...state.undo.slice(0, -1),
        {
          ...latest,
          before: mergeSnapshotChanges({
            ...before,
            blocks: before.blocks.filter((block) => !latest.after.blocks.some((existing) => existing.id === block.id)),
          }, latest.before),
          after: mergeSnapshotChanges(latest.after, after),
          updatedAt: now,
        },
      ],
      redo: [],
    };
  }
  return {
    undo: [
      ...state.undo.slice(-(NOTES_UNDO_STACK_LIMIT - 1)),
      {
        id: options.id,
        kind: options.kind,
        groupKey,
        before: cloneSnapshot(before),
        after: cloneSnapshot(after),
        createdAt: now,
        updatedAt: now,
      },
    ],
    redo: [],
  };
}

export function notesUndoShortcutAction(
  input: NotesUndoShortcutInput,
): "undo" | "redo" | null {
  if (!(input.ctrlKey || input.metaKey) || input.altKey) return null;
  const key = input.key.toLowerCase();
  if (!input.shiftKey && key === "z") return "undo";
  if (input.shiftKey && key === "z") return "redo";
  if (!input.shiftKey && key === "y") return "redo";
  return null;
}

/** Read the optional document range; block-local edits need no document selection. */
function parseDocumentSelection(value: unknown): NotesDocumentSelection | null {
  if (value === undefined || value === null) return null;
  if (!isRecord(value)) throw new Error("documentSelection must be an object");
  const point = (value: unknown): NotesDocumentSelection["anchor"] => {
    if (!isRecord(value)) throw new Error("documentSelection point must be an object");
    const blockId = readString(value.blockId, "documentSelection.blockId");
    const offset = readNumber(value.offset, "documentSelection.offset");
    if (!Number.isSafeInteger(offset) || offset < 0) throw new Error("documentSelection offset must be a nonnegative safe integer");
    return { blockId, offset };
  };
  return { anchor: point(value.anchor), focus: point(value.focus) };
}

function parseSnapshot(value: unknown, label: string): NotesUndoSnapshot {
  if (!isRecord(value)) throw new Error(`${label} must be an object`);
  const blocksValue = value.blocks;
  if (!Array.isArray(blocksValue)) throw new Error(`${label}.blocks must be an array`);
  const childIdsValue = value.childIdsByParentId;
  if (!isRecord(childIdsValue)) {
    throw new Error(`${label}.childIdsByParentId must be an object`);
  }
  const documentSelection = parseDocumentSelection(value.documentSelection);
  const focusSelectionValue = value.focusSelection;
  let focusSelection: NotesTextSelection | null = null;
  if (focusSelectionValue !== undefined && focusSelectionValue !== null) {
    if (!isRecord(focusSelectionValue)) {
      throw new Error(`${label}.focusSelection must be an object or null`);
    }
    const start = readNumber(focusSelectionValue.start, `${label}.focusSelection.start`);
    const end = readNumber(focusSelectionValue.end, `${label}.focusSelection.end`);
    if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end < start) {
      throw new Error(`${label}.focusSelection must be a valid text range`);
    }
    focusSelection = { start, end };
  }
  return {
    ...(value.nativeEditId === undefined ? {} : { nativeEditId: readString(value.nativeEditId, `${label}.nativeEditId`) }),
    pageId: readString(value.pageId, `${label}.pageId`),
    blocks: blocksValue.map((block, index) =>
      parseSnapshotBlock(block, `${label}.blocks[${index}]`)
    ),
    childIdsByParentId: Object.fromEntries(
      Object.entries(childIdsValue).map(([parentId, childIds]) => {
        if (!Array.isArray(childIds)) {
          throw new Error(`${label}.childIdsByParentId.${parentId} must be an array`);
        }
        return [
          parentId,
          childIds.map((childId, index) =>
            readString(childId, `${label}.childIdsByParentId.${parentId}[${index}]`)
          ),
        ];
      }),
    ),
    focusBlockId: readNullableString(value.focusBlockId, `${label}.focusBlockId`),
    focusSelection,
    ...(documentSelection ? { documentSelection } : {}),
  };
}

function parseEntry(value: unknown, label: string): NotesUndoEntry {
  if (!isRecord(value)) throw new Error(`${label} must be an object`);
  return {
    id: readString(value.id, `${label}.id`),
    kind: readUndoKind(value.kind, `${label}.kind`),
    groupKey: readNullableString(value.groupKey, `${label}.groupKey`),
    before: parseSnapshot(value.before, `${label}.before`),
    after: parseSnapshot(value.after, `${label}.after`),
    createdAt: readNumber(value.createdAt, `${label}.createdAt`),
    updatedAt: readNumber(value.updatedAt, `${label}.updatedAt`),
  };
}

export function parseNotesUndoStateJson(stateJson: string): NotesUndoState {
  const parsed: unknown = JSON.parse(stateJson);
  if (!isRecord(parsed)) throw new Error("notes undo state must be an object");
  if (parsed.schema_version !== NOTES_UNDO_SCHEMA_VERSION) {
    throw new Error("notes undo state schema version is unsupported");
  }
  if (!Array.isArray(parsed.undo)) throw new Error("notes undo state undo stack must be an array");
  if (!Array.isArray(parsed.redo)) throw new Error("notes undo state redo stack must be an array");
  return {
    undo: parsed.undo.map((entry, index) => parseEntry(entry, `undo[${index}]`)),
    redo: parsed.redo.map((entry, index) => parseEntry(entry, `redo[${index}]`)),
  };
}

export function serializeNotesUndoState(state: NotesUndoState): string {
  const serialized: SerializedNotesUndoState = {
    schema_version: NOTES_UNDO_SCHEMA_VERSION,
    undo: state.undo,
    redo: state.redo,
  };
  return JSON.stringify(serialized);
}

/**
 * Measure serialized undo recovery state using the backend's UTF-8 byte semantics.
 */
export function notesUndoStateByteLength(state: NotesUndoState): number {
  return new TextEncoder().encode(serializeNotesUndoState(state)).byteLength;
}

/**
 * Drop the oldest recovery entries until the serialized state fits the storage limit.
 */
export function trimNotesUndoStateToByteLimit(
  state: NotesUndoState,
  maxBytes = NOTES_UNDO_STATE_MAX_BYTES,
): NotesUndoState {
  let undo = [...state.undo];
  let redo = [...state.redo];
  while (undo.length > 0 || redo.length > 0) {
    const candidate = { undo, redo };
    if (notesUndoStateByteLength(candidate) <= maxBytes) return candidate;
    const oldestUndo = undo[0];
    const oldestRedo = redo[0];
    if (!oldestRedo || (oldestUndo && oldestUndo.updatedAt <= oldestRedo.updatedAt)) {
      undo = undo.slice(1);
    } else {
      redo = redo.slice(1);
    }
  }
  return { undo: [], redo: [] };
}

export function parentIdsByDepth(snapshot: NotesUndoSnapshot): string[] {
  const blocksById = Object.fromEntries(snapshot.blocks.map((block) => [block.id, block]));
  const depthForParent = (parentId: string): number => {
    const block = blocksById[parentId];
    if (!block) return 0;
    return 1 + depthForParent(parentIdForBlock(block));
  };
  return Object.keys(snapshot.childIdsByParentId)
    .sort((left, right) => depthForParent(left) - depthForParent(right));
}
