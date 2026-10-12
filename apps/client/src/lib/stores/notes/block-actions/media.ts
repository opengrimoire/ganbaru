import { invalidateAssetUrl } from "$lib/api/asset-url-cache";
import {
  blockWithBookmark,
  blockWithEmbedUrl,
  blockWithEquationExpression,
  blockWithLinkPreviewUrl,
  blockWithMedia,
  type NotesMediaAssetChange,
} from "$lib/notes/blocks/factory";
import { mediaManagedAssetPath } from "$lib/notes/block-types/media";
import type {
  NotesBlock,
  NotesBlockUpdate,
} from "$lib/notes/types";
import type {
  NotesUndoRecordOptions,
  NotesUndoSnapshot,
} from "$lib/notes/history/undo-history";

interface NotesMediaBlockActionsContext {
  blockById: (blockId: string) => NotesBlock | undefined;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  scheduleBlockSave: (blockId: string, update: NotesBlockUpdate) => void;
  createUndoSnapshot: (focusBlockId: string | null) => NotesUndoSnapshot | null;
  recordUndo: (options: Omit<NotesUndoRecordOptions, "id">) => void;
}

export interface NotesMediaBlockActions {
  updateBookmark: (blockId: string, url: string, caption: string) => Promise<void>;
  updateEmbedUrl: (blockId: string, url: string) => Promise<void>;
  updateLinkPreviewUrl: (blockId: string, url: string) => Promise<void>;
  updateEquationExpression: (blockId: string, expression: string) => Promise<void>;
  updateMedia: (
    blockId: string,
    url: string,
    caption: string,
    name?: string,
    assetChange?: NotesMediaAssetChange,
  ) => Promise<void>;
}

function managedMediaAssetPath(value: NotesBlock | NotesBlockUpdate): string | null {
  if (value.type === "image") return mediaManagedAssetPath(value.image);
  if (value.type === "video") return mediaManagedAssetPath(value.video);
  if (value.type === "audio") return mediaManagedAssetPath(value.audio);
  if (value.type === "file") return mediaManagedAssetPath(value.file);
  if (value.type === "pdf") return mediaManagedAssetPath(value.pdf);
  return null;
}

export function invalidateReplacedNotesMediaAsset(
  previous: NotesBlock | undefined,
  next: NotesBlockUpdate,
): void {
  if (!previous) return;
  const previousPath = managedMediaAssetPath(previous);
  if (!previousPath || previousPath === managedMediaAssetPath(next)) return;
  invalidateAssetUrl("notes-file", previousPath);
}

/** Create media and external-content block mutations. */
export function createNotesMediaBlockActions(
  context: NotesMediaBlockActionsContext,
): NotesMediaBlockActions {
  function applyScheduledUpdate(
    block: NotesBlock,
    update: NotesBlockUpdate,
    groupKey: string,
  ): void {
    const before = context.createUndoSnapshot(block.id);
    context.localApplyBlockUpdate(block.id, update);
    context.scheduleBlockSave(block.id, update);
    context.recordUndo({
      kind: "update",
      before,
      after: context.createUndoSnapshot(block.id),
      groupKey,
    });
  }

  async function updateBookmark(blockId: string, url: string, caption: string): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type !== "bookmark") return;
    applyScheduledUpdate(block, blockWithBookmark(block, url, caption), `update:${blockId}`);
  }

  async function updateEmbedUrl(blockId: string, url: string): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type !== "embed") return;
    applyScheduledUpdate(block, blockWithEmbedUrl(block, url), `update:${blockId}`);
  }

  async function updateLinkPreviewUrl(blockId: string, url: string): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type !== "link_preview") return;
    applyScheduledUpdate(block, blockWithLinkPreviewUrl(block, url), `update:${blockId}`);
  }

  async function updateEquationExpression(blockId: string, expression: string): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type !== "equation") return;
    applyScheduledUpdate(
      block,
      blockWithEquationExpression(block, expression),
      `update:${blockId}`,
    );
  }

  async function updateMedia(
    blockId: string,
    url: string,
    caption: string,
    name?: string,
    assetChange?: NotesMediaAssetChange,
  ): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || !["image", "video", "audio", "file", "pdf"].includes(block.type)) return;
    const update = blockWithMedia(block, url, caption, name, assetChange);
    invalidateReplacedNotesMediaAsset(block, update);
    applyScheduledUpdate(block, update, `update:${blockId}`);
  }

  return {
    updateBookmark,
    updateEmbedUrl,
    updateLinkPreviewUrl,
    updateEquationExpression,
    updateMedia,
  };
}
