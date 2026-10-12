import { invoke } from "@tauri-apps/api/core";
import { dbUrl } from "$lib/api/db";
import { parseMusicTransferPreview, parseMusicTransferResult, type MusicTransferCommit, type MusicTransferFormat, type MusicTransferSource } from "$lib/music/library/interchange";
import {
  parseMusicContextAssignments,
  type MusicAssignmentOwnerKind,
  type MusicContextAssignment,
} from "$lib/music/context-assignment";
import {
  parseBindingResult,
  parseBindings,
  parseCollections,
  parseDeleteImpact,
  parseInspectorDetail,
  parseItemRepairPreview,
  parseMusicCount,
  parseIssues,
  parseItemWindow,
  parsePlaylist,
  parsePlaylistSummaries,
  parsePlaylistPlaybackEntries,
  parseRefreshJobProgress,
  parseRelinkPlanSummary,
  parseRelinkPlanWindow,
  parseRoots,
  parseSourceSummaries,
  parseSourceRemovalImpact,
  parseWriteReceipt,
  parseYouTubeSnapshotResult,
  type LocalRootBinding,
  type MusicBulkMembershipWrite,
  type MusicBulkMembershipEdit,
  type MusicBulkMembershipResult,
  type MusicBulkReviewWrite,
  type MusicBulkSnoozeWrite,
  type MusicAdvancedMembershipWrite,
  type MusicCollectionWrite,
  type MusicInspectorDetail,
  type MusicInterchangeImportResult,
  type MusicItemRepairApply,
  type MusicItemRepairPreview,
  type MusicIssue,
  type MusicItemSignalsWrite,
  type MusicItemWindow,
  type MusicItemWindowRequest,
  type MusicLocalRefreshRequest,
  type MusicLocalRootCreate,
  type MusicLocalRoot,
  type MusicMetadataOverrideWrite,
  type MusicMembershipRemove,
  type MusicMembershipMatrixEntry,
  type MusicPlaylist,
  type MusicPlaylistCreate,
  type MusicPlaylistDelete,
  type MusicPlaylistDeleteImpact,
  type MusicPlaylistDuplicate,
  type MusicPlaylistSummary,
  type MusicPlaylistUpdate,
  type MusicPlaylistsReorder,
  type MusicPlaylistPlaybackEntry,
  type MusicReviewWrite,
  type MusicReviewSelectionResult,
  type MusicReviewSelectionWrite,
  type MusicRelinkApplyRequest,
  type MusicRelinkPlanRequest,
  type MusicRelinkPlanSummary,
  type MusicRelinkPlanWindow,
  type MusicRefreshJobProgress,
  type MusicSourceCollection,
  type MusicSourceSummary,
  type MusicSourceRemovalImpact,
  type MusicSourceRemovalRequest,
  type MusicWriteReceipt,
  type MusicYouTubePlaylistSnapshotWrite,
  type MusicYouTubeSnapshotResult,
  type MusicYouTubeSourceFailureWrite,
  type MusicYouTubeVideoWrite,
} from "$lib/music/library/contracts";

export type MusicLibraryApiErrorCode = "validation" | "not-found" | "conflict" | "stale-write" | "database" | "unknown";

export class MusicLibraryApiError extends Error {
  readonly code: MusicLibraryApiErrorCode;
  readonly field: string | null;

  constructor(code: MusicLibraryApiErrorCode, message: string, field: string | null = null) {
    super(message);
    this.name = "MusicLibraryApiError";
    this.code = code;
    this.field = field;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const errorCodes = new Set<MusicLibraryApiErrorCode>([
  "validation",
  "not-found",
  "conflict",
  "stale-write",
  "database",
  "unknown",
]);

export function normalizeMusicLibraryError(error: unknown): MusicLibraryApiError {
  if (error instanceof MusicLibraryApiError) return error;
  if (isRecord(error)) {
    const code = typeof error.code === "string" && errorCodes.has(error.code as MusicLibraryApiErrorCode)
      ? error.code as MusicLibraryApiErrorCode
      : "unknown";
    const message = typeof error.message === "string" && error.message.trim()
      ? error.message
      : "Music library operation failed.";
    const field = typeof error.field === "string" && error.field.trim() ? error.field : null;
    return new MusicLibraryApiError(code, message, field);
  }
  const message = error instanceof Error ? error.message : String(error);
  return new MusicLibraryApiError("unknown", message.trim() || "Music library operation failed.");
}

async function call<T>(command: string, args: Record<string, unknown>, parse: (value: unknown) => T): Promise<T> {
  try {
    return parse(await invoke<unknown>(command, args));
  } catch (error) {
    if (error instanceof Error && !(error instanceof MusicLibraryApiError)) {
      throw new MusicLibraryApiError("database", `Invalid response from ${command}: ${error.message}`);
    }
    throw normalizeMusicLibraryError(error);
  }
}

function databaseArgs(extra: Record<string, unknown> = {}): Record<string, unknown> {
  return { dbUrl: dbUrl(), ...extra };
}

const parseVoid = (_value: unknown): void => undefined;

export const startMusicLocalRefresh = (request: MusicLocalRefreshRequest): Promise<MusicRefreshJobProgress> =>
  call("music_library_start_local_refresh", databaseArgs({ request }), parseRefreshJobProgress);
export const getMusicRefreshProgress = (jobId: string): Promise<MusicRefreshJobProgress> =>
  call("music_library_refresh_progress", databaseArgs({ jobId }), parseRefreshJobProgress);
export const cancelMusicRefresh = (jobId: string, cancelledAt: number): Promise<MusicRefreshJobProgress> =>
  call("music_library_cancel_refresh", databaseArgs({ jobId, cancelledAt }), parseRefreshJobProgress);
export const upsertMusicYouTubeVideo = (request: MusicYouTubeVideoWrite): Promise<MusicWriteReceipt> =>
  call("music_library_upsert_youtube_video", databaseArgs({ request }), parseWriteReceipt);
export const getMusicYouTubeDuplicateCount = (videoIds: string[]): Promise<number> =>
  call("music_library_youtube_duplicate_count", databaseArgs({ videoIds }), parseMusicCount);
export const applyMusicYouTubePlaylistSnapshot = (
  request: MusicYouTubePlaylistSnapshotWrite,
): Promise<MusicYouTubeSnapshotResult> =>
  call("music_library_apply_youtube_playlist_snapshot", databaseArgs({ request }), parseYouTubeSnapshotResult);
export const reportMusicYouTubeSourceFailure = (request: MusicYouTubeSourceFailureWrite): Promise<void> =>
  call("music_library_report_youtube_source_failure", databaseArgs({ request }), parseVoid);
export const createMusicRelinkPlan = (request: MusicRelinkPlanRequest): Promise<MusicRelinkPlanSummary> =>
  call("music_library_create_relink_plan", databaseArgs({ request }), parseRelinkPlanSummary);
export const getMusicRelinkPlanEntries = (
  planId: string,
  offset: number,
  limit: number,
): Promise<MusicRelinkPlanWindow> =>
  call("music_library_relink_plan_entries", databaseArgs({ planId, offset, limit }), parseRelinkPlanWindow);
export const applyMusicRelinkPlan = (request: MusicRelinkApplyRequest): Promise<MusicRelinkPlanSummary> =>
  call("music_library_apply_relink_plan", databaseArgs({ request }), parseRelinkPlanSummary);
export const cancelMusicRelinkPlan = (planId: string, cancelledAt: number): Promise<MusicRelinkPlanSummary> =>
  call("music_library_cancel_relink_plan", databaseArgs({ planId, cancelledAt }), parseRelinkPlanSummary);
export const getMusicSourceRemovalImpact = (collectionId: string): Promise<MusicSourceRemovalImpact> =>
  call("music_library_source_removal_impact", databaseArgs({ collectionId }), parseSourceRemovalImpact);
export const removeMusicSource = (request: MusicSourceRemovalRequest): Promise<MusicSourceRemovalImpact> =>
  call("music_library_remove_source", databaseArgs({ request }), parseSourceRemovalImpact);
export const createMusicPlaylist = (request: MusicPlaylistCreate): Promise<MusicWriteReceipt> =>
  call("music_library_create_playlist", databaseArgs({ request }), parseWriteReceipt);
export const updateMusicPlaylist = (request: MusicPlaylistUpdate): Promise<MusicWriteReceipt> =>
  call("music_library_update_playlist", databaseArgs({ request }), parseWriteReceipt);
export const reorderMusicPlaylists = (request: MusicPlaylistsReorder): Promise<MusicWriteReceipt[]> =>
  call("music_library_reorder_playlists", databaseArgs({ request }), (value) => {
    if (!Array.isArray(value)) throw new Error("playlist reorder receipts must be an array");
    return value.map((entry, index) => parseWriteReceipt(entry, `playlist reorder receipts[${index}]`));
  });
export const duplicateMusicPlaylist = (request: MusicPlaylistDuplicate): Promise<MusicWriteReceipt> =>
  call("music_library_duplicate_playlist", databaseArgs({ request }), parseWriteReceipt);
export const getMusicPlaylistDeleteImpact = (playlistId: string): Promise<MusicPlaylistDeleteImpact> =>
  call("music_library_playlist_delete_impact", databaseArgs({ playlistId }), parseDeleteImpact);
export const deleteMusicPlaylist = (request: MusicPlaylistDelete): Promise<MusicPlaylistDeleteImpact> =>
  call("music_library_delete_playlist", databaseArgs({ request }), parseDeleteImpact);
export const setMusicReviewState = (request: MusicReviewWrite): Promise<MusicWriteReceipt> =>
  call("music_library_set_review_state", databaseArgs({ request }), parseWriteReceipt);
export const setMusicMetadataOverrides = (request: MusicMetadataOverrideWrite): Promise<MusicWriteReceipt> =>
  call("music_library_set_metadata_overrides", databaseArgs({ request }), parseWriteReceipt);
export const setMusicItemSignals = (request: MusicItemSignalsWrite): Promise<MusicWriteReceipt[]> =>
  call("music_library_set_item_signals", databaseArgs({ request }), (value) => {
    if (!Array.isArray(value)) throw new Error("music signal receipts must be an array");
    return value.map((entry, index) => parseWriteReceipt(entry, `music signal receipts[${index}]`));
  });
export const upsertMusicMemberships = (request: MusicBulkMembershipWrite): Promise<MusicWriteReceipt[]> =>
  call("music_library_upsert_memberships", databaseArgs({ request }), (value) => {
    if (!Array.isArray(value)) throw new Error("membership receipts must be an array");
    return value.map((entry, index) => parseWriteReceipt(entry, `membership receipts[${index}]`));
  });
export const bulkEditMusicMemberships = (request: MusicBulkMembershipEdit): Promise<MusicBulkMembershipResult> =>
  call("music_library_bulk_edit_memberships", databaseArgs({ request }), (value) => {
    if (typeof value !== "object" || value === null || !("changedCount" in value) || typeof value.changedCount !== "number") {
      throw new Error("bulk membership result must contain changedCount");
    }
    return { changedCount: value.changedCount };
  });
export const getMusicMembershipMatrix = (itemIds: string[]): Promise<MusicMembershipMatrixEntry[]> =>
  call("music_library_membership_matrix", databaseArgs({ itemIds }), (value) => {
    if (!Array.isArray(value)) throw new Error("membership matrix must be an array");
    const weights = new Set(["rarely", "less-often", "normal", "more-often", "much-more-often"]);
    return value.map((entry, index) => {
      if (typeof entry !== "object" || entry === null) throw new Error(`membership matrix[${index}] must be an object`);
      const row = entry as Record<string, unknown>;
      if (typeof row.itemId !== "string" || typeof row.playlistId !== "string" || typeof row.weight !== "string" || !weights.has(row.weight)) {
        throw new Error(`membership matrix[${index}] is invalid`);
      }
      return { itemId: row.itemId, playlistId: row.playlistId, weight: row.weight as MusicMembershipMatrixEntry["weight"] };
    });
  });
export const getMusicPlaylistPlaybackEntries = (playlistId: string, nowMs: number): Promise<MusicPlaylistPlaybackEntry[]> =>
  call("music_library_playlist_playback_entries", databaseArgs({ playlistId, nowMs }), parsePlaylistPlaybackEntries);
export const getMusicContextAssignments = (
  ownerKind: MusicAssignmentOwnerKind,
  ownerId: string,
): Promise<MusicContextAssignment[]> =>
  call("music_library_context_assignments", databaseArgs({ ownerKind, ownerId }), parseMusicContextAssignments);
export const bulkSetMusicReviewState = (request: MusicBulkReviewWrite): Promise<MusicBulkMembershipResult> =>
  call("music_library_bulk_set_review_state", databaseArgs({ request }), (value) => {
    if (typeof value !== "object" || value === null || !("changedCount" in value) || typeof value.changedCount !== "number") throw new Error("bulk review result must contain changedCount");
    return { changedCount: value.changedCount };
  });
export const applyMusicReviewSelection = (request: MusicReviewSelectionWrite): Promise<MusicReviewSelectionResult> =>
  call("music_library_apply_review_selection", databaseArgs({ request }), (value) => {
    if (!isRecord(value)) throw new Error("review selection result must be an object");
    if (typeof value.membershipChangedCount !== "number" || !Number.isSafeInteger(value.membershipChangedCount)) {
      throw new Error("review selection membership count must be an integer");
    }
    if (typeof value.reviewChangedCount !== "number" || !Number.isSafeInteger(value.reviewChangedCount)) {
      throw new Error("review selection review count must be an integer");
    }
    if (!Array.isArray(value.items)) throw new Error("review selection receipts must be an array");
    const items = value.items.map((entry) => parseWriteReceipt(entry, "review selection receipt"));
    const requestedIds = new Set(request.items.map((item) => item.itemId));
    if (items.length !== requestedIds.size || items.some((item) => !requestedIds.has(item.id)) || new Set(items.map((item) => item.id)).size !== items.length) {
      throw new Error("review selection receipts do not match the requested items");
    }
    return {
      membershipChangedCount: value.membershipChangedCount,
      reviewChangedCount: value.reviewChangedCount,
      items,
    };
  });
export const bulkSnoozeMusicItems = (request: MusicBulkSnoozeWrite): Promise<MusicBulkMembershipResult> =>
  call("music_library_bulk_snooze", databaseArgs({ request }), (value) => {
    if (typeof value !== "object" || value === null || !("changedCount" in value) || typeof value.changedCount !== "number") throw new Error("bulk snooze result must contain changedCount");
    return { changedCount: value.changedCount };
  });
export const saveMusicAdvancedMembership = (request: MusicAdvancedMembershipWrite): Promise<MusicWriteReceipt> =>
  call("music_library_save_advanced_membership", databaseArgs({ request }), parseWriteReceipt);
export const removeMusicMemberships = (request: MusicMembershipRemove): Promise<void> =>
  call("music_library_remove_memberships", databaseArgs({ request }), parseVoid);
export const removeMusicSnooze = (snoozeId: string): Promise<void> =>
  call("music_library_remove_snooze", databaseArgs({ request: { snoozeId } }), parseVoid);
export const previewMusicTransfer = (vaultId: string, source: MusicTransferSource) =>
  call("music_library_preview_transfer", databaseArgs({ vaultId, source }), parseMusicTransferPreview);
export const commitMusicTransfer = (vaultId: string, request: MusicTransferCommit): Promise<MusicInterchangeImportResult> =>
  call("music_library_commit_transfer", databaseArgs({ vaultId, request }), parseMusicTransferResult);
export const exportMusicTransfer = (vaultId: string, playlistIds: string[], format: MusicTransferFormat): Promise<boolean> =>
  call("music_library_export_transfer", databaseArgs({ vaultId, request: { playlistIds, format } }), (value) => {
    if (typeof value !== "boolean") throw new Error("Invalid Music export result");
    return value;
  });
export const getMusicItemWindow = (request: MusicItemWindowRequest): Promise<MusicItemWindow> =>
  call("music_library_item_window", databaseArgs({ request }), parseItemWindow);
export const getMusicPlaylistSummaries = (nowMs: number, offset: number, limit: number): Promise<MusicPlaylistSummary[]> =>
  call("music_library_playlist_summaries", databaseArgs({ nowMs, offset, limit }), parsePlaylistSummaries);
export const getMusicSourceSummaries = (nowMs: number, offset: number, limit: number): Promise<MusicSourceSummary[]> =>
  call("music_library_source_summaries", databaseArgs({ nowMs, offset, limit }), parseSourceSummaries);
export const getMusicIssues = (offset: number, limit: number): Promise<MusicIssue[]> =>
  call("music_library_issues", databaseArgs({ offset, limit }), parseIssues);
export const getMusicInspectorDetail = (itemId: string): Promise<MusicInspectorDetail> =>
  call("music_library_inspector_detail", databaseArgs({ itemId }), parseInspectorDetail);
export const getMusicLocalRoots = (offset: number, limit: number): Promise<MusicLocalRoot[]> =>
  call("music_library_local_roots", databaseArgs({ offset, limit }), parseRoots);
export const createMusicLocalRoot = (request: MusicLocalRootCreate): Promise<MusicWriteReceipt> =>
  call("music_library_create_local_root", databaseArgs({ request }), parseWriteReceipt);
export const previewMusicItemRepair = (itemId: string, filePath: string): Promise<MusicItemRepairPreview> =>
  call("music_library_preview_item_repair", databaseArgs({ itemId, filePath }), parseItemRepairPreview);
export const applyMusicItemRepair = (request: MusicItemRepairApply): Promise<MusicWriteReceipt> =>
  call("music_library_apply_item_repair", databaseArgs({ request }), parseWriteReceipt);
export const undoMusicItemRepair = (locationId: string, rootId: string): Promise<void> =>
  call("music_library_undo_item_repair", databaseArgs({ locationId, rootId }), parseVoid);
export const getMusicSourceCollections = (offset: number, limit: number): Promise<MusicSourceCollection[]> =>
  call("music_library_source_collections", databaseArgs({ offset, limit }), parseCollections);
export const getMusicPlaylist = (playlistId: string): Promise<MusicPlaylist> =>
  call("music_library_playlist_detail", databaseArgs({ playlistId }), parsePlaylist);
export const upsertMusicSourceCollection = (request: MusicCollectionWrite): Promise<MusicWriteReceipt> =>
  call("music_library_upsert_source_collection", databaseArgs({ request }), parseWriteReceipt);

export const getLocalRootBindings = (vaultId: string, rootIds: string[]): Promise<LocalRootBinding[]> =>
  call("music_local_root_bindings", { vaultId, rootIds }, parseBindings);
export const setLocalRootBinding = (vaultId: string, rootId: string, folderPath: string): Promise<LocalRootBinding> =>
  call("music_set_local_root_binding", { vaultId, rootId, folderPath }, parseBindingResult);
export const clearLocalRootBinding = (vaultId: string, rootId: string): Promise<LocalRootBinding> =>
  call("music_clear_local_root_binding", { vaultId, rootId }, parseBindingResult);
