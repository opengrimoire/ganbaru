export type MusicLibrarySourceKind = "local-file" | "youtube-video";
export type MusicMediaKind = "audio" | "video" | "unknown";
export type MusicReviewState = "unreviewed" | "reviewed" | "deferred" | "ignored";
export type MusicItemAvailability = "available" | "missing" | "unavailable" | "ambiguous" | "unknown";
export type MusicLocationAvailability = "available" | "missing" | "ambiguous" | "unsupported" | "unknown";
export type MusicCollectionKind = "local-root" | "youtube-playlist";
export type MusicRefreshState = "idle" | "queued" | "running" | "partial" | "failed";
export type MusicRefreshJobState = "queued" | "running" | "completed" | "partial" | "failed" | "cancelled";
export type MusicYouTubeResolutionState = "resolving" | "ready" | "unavailable" | "embedding-blocked" | "timed-out";
export type MusicSourceHealth = "healthy" | "stale" | "issues" | "disabled";
export type MusicRelinkPlanState = "planning" | "ready" | "applied" | "cancelled";
export type MusicRelinkMatchKind = "exact" | "likely" | "ambiguous" | "missing" | "new";
export type MusicRepairMatchStrength = "exact" | "likely" | "weak";
export type MusicWeight = "rarely" | "less-often" | "normal" | "more-often" | "much-more-often";
export type MusicIntendedUse = "general" | "focus" | "reading" | "relaxation" | "energizing";
export type MusicItemSignal = "lyrics" | "sudden-changes" | "high-intensity" | "calm" | "repetitive" | "energizing";
export type MusicSnoozeScope = "playlist" | "all-playlists";
export type MusicRepeatMode = "off" | "all" | "one";
export type MusicPlaybackMode = "in-order" | "shuffle" | "mix";
export type MusicListDestination = "review" | "library" | "playlist";
export type MusicItemSort = "title" | "artist" | "album" | "source-order" | "discovered-at" | "added-to-playlist" | "last-played-at" | "play-count" | "manual-position";
export type MusicSortDirection = "ascending" | "descending";
export type MusicGroupBy = "none" | "source-kind" | "review-state" | "availability" | "album" | "folder" | "source-collection";
export type LocalRootBindingStatus = "available" | "missing" | "needs-relink";

export interface MusicWriteReceipt { id: string; version: number }
export interface MusicPlaylistDeleteImpact {
  membershipCount: number;
  projectFocusAssignmentCount: number;
  projectBreakAssignmentCount: number;
  calendarAssignmentCount: number;
  contextAssignmentCount: number;
  assignments: MusicPlaylistAssignmentReference[];
}
export type MusicPlaylistAssignmentKind = "project-focus" | "project-break" | "calendar-event" | "context-assignment";
export interface MusicPlaylistAssignmentReference { kind: MusicPlaylistAssignmentKind; id: string; label: string }
export interface MusicPlaylistCreate {
  id: string;
  name: string;
  icon: string;
  shuffleEnabled: boolean;
  mixEnabled: boolean;
  repeatMode: MusicRepeatMode;
  intendedUses: MusicIntendedUse[];
  createdAtMs: number;
}
export interface MusicPlaylistUpdate extends Omit<MusicPlaylistCreate, "createdAtMs"> {
  expectedVersion: number;
  updatedAtMs: number;
}
export interface MusicPlaylistOrderEntry { playlistId: string; expectedVersion: number }
export interface MusicPlaylistsReorder { playlists: MusicPlaylistOrderEntry[]; updatedAtMs: number }
export interface MusicPlaylistDuplicate {
  sourcePlaylistId: string;
  newPlaylistId: string;
  name: string;
  createdAtMs: number;
}
export interface MusicPlaylistDelete {
  playlistId: string;
  replacementPlaylistId: string | null;
  expectedVersion: number;
  expectedImpact: MusicPlaylistDeleteImpact;
}
export interface MusicReviewWrite {
  itemId: string;
  reviewState: MusicReviewState;
  deferredUntil: number | null;
  expectedVersion: number;
  updatedAtMs: number;
}
export interface MusicMetadataOverrideWrite {
  itemId: string;
  titleOverride: string | null;
  artistOverride: string | null;
  albumOverride: string | null;
  artworkOverride: string | null;
  expectedVersion: number;
  updatedAtMs: number;
}
export interface MusicMembershipWrite {
  id: string;
  playlistId: string;
  itemId: string;
  position: number;
  weight: MusicWeight;
  enabled: boolean;
  startMs: number | null;
  endMs: number | null;
  volume: number | null;
  rate: number | null;
  expectedVersion: number | null;
  updatedAtMs: number;
}
export interface MusicBulkMembershipWrite { memberships: MusicMembershipWrite[] }
export interface MusicBulkMembershipEdit {
  actionId: string;
  itemIds: string[];
  addPlaylistIds: string[];
  removePlaylistIds: string[];
  weightPlaylistIds: string[];
  weight: MusicWeight | null;
  updatedAtMs: number;
}
export interface MusicBulkMembershipResult { changedCount: number }
export interface MusicMembershipMatrixEntry { itemId: string; playlistId: string; weight: MusicWeight }
export interface MusicPlaylistReorder { playlistId: string; itemId: string; targetIndex: number; updatedAtMs: number }
export interface MusicPlaylistReorderResult { itemIds: string[] }
export interface MusicPlaylistPlaybackEntry {
  membershipId: string;
  itemId: string;
  identityKey: string;
  sourceKind: MusicLibrarySourceKind;
  youtubeVideoId: string | null;
  youtubeResolutionState: MusicYouTubeResolutionState | null;
  title: string;
  originalArtworkIdentity: string | null;
  artworkOverride: string | null;
  availability: MusicItemAvailability;
  rootId: string | null;
  relativePath: string | null;
  position: number;
  weight: MusicWeight;
  enabled: boolean;
  startMs: number | null;
  endMs: number | null;
  volume: number | null;
  rate: number | null;
  snoozed: boolean;
  snoozedUntil: number | null;
  snoozedIndefinitely: boolean;
  skipRanges: MusicMembershipSkipRange[];
}
export type MusicSelectionKind = "automatic" | "manual";
export type MusicListeningOutcome = "started" | "completed" | "skipped";
export interface MusicListeningUpdate {
  playlistId: string | null;
  itemId: string;
  selectionKind: MusicSelectionKind;
  outcome: MusicListeningOutcome;
  occurredAt: number;
}
export interface MusicRecentSelection { itemId: string; selectedAtMs: number }
export interface MusicVersionedItem { itemId: string; expectedVersion: number }
export interface MusicBulkReviewWrite { items: MusicVersionedItem[]; reviewState: MusicReviewState; deferredUntil: number | null; updatedAtMs: number }
export interface MusicReviewSelectionWrite {
  actionId: string;
  items: MusicVersionedItem[];
  reviewState: Extract<MusicReviewState, "reviewed" | "ignored">;
  addPlaylistIds: string[];
  removePlaylistIds: string[];
  updatedAtMs: number;
}
export interface MusicReviewSelectionResult {
  membershipChangedCount: number;
  reviewChangedCount: number;
  items: MusicWriteReceipt[];
}
export interface MusicBulkSnoozeWrite {
  actionId: string;
  itemIds: string[];
  scope: MusicSnoozeScope;
  playlistId: string | null;
  startsAtMs: number;
  endsAtMs: number | null;
  reason: string;
  createdAtMs: number;
}
export interface MusicMembershipRemove { membershipIds: string[] }
export interface MusicMembershipSkipRange { id: string; membershipId: string; startMs: number; endMs: number; sortOrder: number }
export interface MusicAdvancedMembershipWrite { membership: MusicMembershipWrite; skipRanges: MusicMembershipSkipRange[] }
export interface MusicSnoozeWrite {
  id: string;
  itemId: string;
  scope: MusicSnoozeScope;
  playlistId: string | null;
  startsAtMs: number;
  endsAtMs: number | null;
  reason: string;
  createdAtMs: number;
}
export interface MusicInterchangeImportResult { playlistCount: number; itemCount: number; membershipCount: number; assignmentCount: number }
export interface MusicItemSignalsWrite { itemIds: string[]; signals: MusicItemSignal[]; updatedAtMs: number }
export interface MusicCollectionWrite {
  id: string;
  kind: MusicCollectionKind;
  identityKey: string;
  name: string;
  localRootId: string | null;
  youtubePlaylistId: string | null;
  updatedAtMs: number;
}
export interface MusicLocalRootCreate {
  rootId: string;
  collectionId: string;
  identityKey: string;
  name: string;
  createdAtMs: number;
}
export interface MusicItemRepairPreview {
  itemId: string;
  folderPath: string;
  relativePath: string;
  title: string;
  artist: string;
  album: string;
  durationMs: number | null;
  fileSizeBytes: number;
  lightweightFingerprint: string;
  strongFingerprint: string;
  matchStrength: MusicRepairMatchStrength;
  reasons: string[];
}
export interface MusicItemRepairApply {
  itemId: string;
  rootId: string;
  locationId: string;
  rootName: string;
  folderPath: string;
  relativePath: string;
  expectedStrongFingerprint: string;
  acceptWeakMismatch: boolean;
  appliedAt: number;
}
export interface MusicItemWindowRequest {
  destination: MusicListDestination;
  playlistId: string | null;
  search: string;
  sourceKind: MusicLibrarySourceKind | null;
  availability: MusicItemAvailability | null;
  reviewState: MusicReviewState | null;
  sourceCollectionId: string | null;
  membershipPlaylistId: string | null;
  snoozed: boolean | null;
  sort: MusicItemSort;
  direction: MusicSortDirection;
  groupBy: MusicGroupBy;
  nowMs: number;
  offset: number;
  limit: number;
}

export interface MusicItemListEntry {
  id: string;
  identityKey: string;
  sourceKind: MusicLibrarySourceKind;
  mediaKind: MusicMediaKind;
  title: string;
  artist: string;
  album: string;
  localRootId: string | null;
  relativePath: string | null;
  sourceCollectionIds: string[];
  originalArtworkIdentity: string | null;
  artworkOverride: string | null;
  durationMs: number | null;
  availability: MusicItemAvailability;
  reviewState: MusicReviewState;
  discoveredAtMs: number;
  updatedAtMs: number;
  version: number;
  playlistCount: number;
  activeSnoozeCount: number;
  lastPlayedAtMs: number | null;
  playCount: number;
  membershipId: string | null;
  membershipPosition: number | null;
  membershipWeight: MusicWeight | null;
  membershipEnabled: boolean | null;
  membershipVersion: number | null;
}
export interface MusicGroupCount { key: string; count: number }
export interface MusicItemWindow {
  items: MusicItemListEntry[];
  groups: MusicGroupCount[];
  totalCount: number;
  offset: number;
  limit: number;
}
export interface MusicPlaylistSummary {
  id: string;
  name: string;
  icon: string;
  shuffleEnabled: boolean;
  mixEnabled: boolean;
  repeatMode: MusicRepeatMode;
  intendedUses: MusicIntendedUse[];
  sortOrder: number;
  totalCount: number;
  eligibleCount: number;
  unavailableCount: number;
  snoozedCount: number;
  localCount: number;
  onlineCount: number;
  version: number;
}
export interface MusicSourceSummary {
  id: string;
  kind: MusicCollectionKind;
  name: string;
  refreshState: MusicRefreshState;
  lastSuccessfulRefreshAtMs: number | null;
  localRootId: string | null;
  youtubePlaylistId: string | null;
  itemCount: number;
  missingCount: number;
  newCount: number;
  unreviewedCount: number;
  unavailableCount: number;
  ambiguousCount: number;
  openIssueCount: number;
  health: MusicSourceHealth;
  discoveryEnabled: boolean;
  version: number;
}
export interface MusicIssue {
  id: string;
  issueKind: string;
  itemId: string | null;
  playlistId: string | null;
  collectionId: string | null;
  rootId: string | null;
  relativePath: string | null;
  actionRequired: boolean;
  message: string;
  createdAtMs: number;
}
export interface MusicLibraryItem {
  id: string;
  identityKey: string;
  sourceKind: MusicLibrarySourceKind;
  mediaKind: MusicMediaKind;
  youtubeVideoId: string | null;
  originalTitle: string;
  originalArtist: string;
  originalAlbum: string;
  originalTrackNumber: number | null;
  originalArtworkIdentity: string | null;
  youtubeResolutionState: MusicYouTubeResolutionState | null;
  titleOverride: string | null;
  artistOverride: string | null;
  albumOverride: string | null;
  artworkOverride: string | null;
  durationMs: number | null;
  availability: MusicItemAvailability;
  reviewState: MusicReviewState;
  reviewChangedAtMs: number | null;
  reviewDeferredUntilMs: number | null;
  discoveredAtMs: number;
  updatedAtMs: number;
  version: number;
}
export interface MusicLocalRefreshRequest {
  jobId: string;
  rootId: string;
  collectionId: string;
  folderPath: string;
  availableRoots: MusicAvailableRootPath[];
  requestedAtMs: number;
}
export interface MusicAvailableRootPath { rootId: string; folderPath: string }
export interface MusicYouTubeVideoWrite {
  videoId: string;
  title: string;
  channel: string;
  durationMs: number | null;
  resolutionState: MusicYouTubeResolutionState;
  resolvedAtMs: number;
}
export interface MusicYouTubePlaylistSnapshotWrite {
  collectionId: string;
  playlistId: string;
  name: string;
  videoIds: string[];
  videos?: MusicYouTubePlaylistVideoWrite[];
  resolvedAtMs: number;
}
export interface MusicYouTubePlaylistVideoWrite {
  videoId: string;
  title: string;
  channel: string;
}
export interface MusicYouTubeSourceFailureWrite {
  collectionId: string;
  playlistId: string;
  name: string;
  resolutionState: MusicYouTubeResolutionState;
  errorCode: string;
  occurredAt: number;
}
export interface MusicYouTubeSnapshotResult {
  collectionId: string;
  canonicalItemCount: number;
  newlyDiscoveredCount: number;
  repeatedVideoCount: number;
  generation: number;
}
export interface MusicRefreshJobProgress {
  jobId: string;
  collectionId: string;
  rootId: string | null;
  kind: MusicCollectionKind;
  state: MusicRefreshJobState;
  generation: number;
  discoveredCount: number;
  processedCount: number;
  skippedCount: number;
  issueCount: number;
  truncatedCount: number;
  absenceDetermined: boolean;
  statusMessage: string;
  requestedAtMs: number;
  startedAtMs: number | null;
  finishedAtMs: number | null;
  updatedAtMs: number;
}
export interface MusicLocalLocation {
  id: string;
  itemId: string;
  rootId: string;
  relativePath: string;
  fileSizeBytes: number | null;
  modifiedAtMs: number | null;
  lightweightFingerprint: string | null;
  strongFingerprint: string | null;
  availability: MusicLocationAvailability;
  lastSeenGeneration: number | null;
  firstSeenAtMs: number;
  updatedAtMs: number;
}
export interface MusicPlaylistMembership extends Omit<MusicMembershipWrite, "expectedVersion"> {
  createdAtMs: number;
  version: number;
}
export interface MusicSnooze extends MusicSnoozeWrite {}
export interface MusicListeningStatistics {
  itemId: string;
  lastPlayedAtMs: number | null;
  playCount: number;
  completionCount: number;
  skipCount: number;
  updatedAtMs: number;
}
export interface MusicInspectorDetail {
  item: MusicLibraryItem;
  locations: MusicLocalLocation[];
  memberships: MusicPlaylistMembership[];
  membershipSkipRanges: MusicMembershipSkipRange[];
  snoozes: MusicSnooze[];
  signals: MusicItemSignal[];
  statistics: MusicListeningStatistics | null;
  sourceCollectionIds: string[];
}
export interface MusicLocalRoot { id: string; name: string; createdAtMs: number; updatedAtMs: number; version: number }
export interface MusicSourceCollection {
  id: string;
  kind: MusicCollectionKind;
  identityKey: string;
  name: string;
  localRootId: string | null;
  youtubePlaylistId: string | null;
  refreshState: MusicRefreshState;
  lastSuccessfulRefreshAtMs: number | null;
  previousSuccessfulRefreshAtMs: number | null;
  lastRefreshErrorCode: string | null;
  snapshotGeneration: number;
  createdAtMs: number;
  updatedAtMs: number;
  version: number;
  discoveryEnabled: boolean;
  removedAtMs: number | null;
}
export interface MusicRelinkPlanRequest {
  planId: string;
  rootId: string;
  replacementFolderPath: string;
  createdAtMs: number;
}
export interface MusicRelinkPlanSummary {
  id: string;
  rootId: string;
  state: MusicRelinkPlanState;
  exactCount: number;
  likelyCount: number;
  ambiguousCount: number;
  missingCount: number;
  newCount: number;
  createdAtMs: number;
  updatedAtMs: number;
}
export interface MusicRelinkPlanEntry {
  id: string;
  matchKind: MusicRelinkMatchKind;
  oldLocationId: string | null;
  suggestedItemId: string | null;
  candidateRelativePath: string | null;
  candidateItemIds: string[];
  fileSizeBytes: number | null;
  resolvedItemId: string | null;
  resolvedAtMs: number | null;
}
export interface MusicRelinkPlanWindow {
  entries: MusicRelinkPlanEntry[];
  totalCount: number;
  offset: number;
  limit: number;
}
export interface MusicRelinkDecision { entryId: string; itemId: string }
export interface MusicRelinkApplyRequest {
  planId: string;
  decisions: MusicRelinkDecision[];
  appliedAt: number;
}
export interface MusicSourceRemovalImpact {
  collectionId: string;
  itemCount: number;
  membershipCount: number;
  sharedItemCount: number;
  orphanedItemCount: number;
  activeRefreshCount: number;
}
export interface MusicSourceRemovalRequest {
  collectionId: string;
  expectedVersion: number;
  expectedImpact: MusicSourceRemovalImpact;
  removeOrphanedItems: boolean;
  removedAtMs: number;
}
export interface MusicPlaylist {
  id: string;
  name: string;
  icon: string;
  shuffleEnabled: boolean;
  mixEnabled: boolean;
  repeatMode: MusicRepeatMode;
  intendedUses: MusicIntendedUse[];
  sortOrder: number;
  createdAtMs: number;
  updatedAtMs: number;
  version: number;
}
export interface MusicSearchRebuildResult { indexedItemCount: number; schemaVersion: number; fingerprint: string; rebuiltAtMs: number }
export interface LocalRootBinding { rootId: string; folderPath: string | null; status: LocalRootBindingStatus }

const sourceKinds = ["local-file", "youtube-video"] as const;
const mediaKinds = ["audio", "video", "unknown"] as const;
const reviewStates = ["unreviewed", "reviewed", "deferred", "ignored"] as const;
const itemAvailability = ["available", "missing", "unavailable", "ambiguous", "unknown"] as const;
const locationAvailability = ["available", "missing", "ambiguous", "unsupported", "unknown"] as const;
const collectionKinds = ["local-root", "youtube-playlist"] as const;
const refreshStates = ["idle", "queued", "running", "partial", "failed"] as const;
const refreshJobStates = ["queued", "running", "completed", "partial", "failed", "cancelled"] as const;
const youtubeResolutionStates = ["resolving", "ready", "unavailable", "embedding-blocked", "timed-out"] as const;
const sourceHealthStates = ["healthy", "stale", "issues", "disabled"] as const;
const relinkPlanStates = ["planning", "ready", "applied", "cancelled"] as const;
const relinkMatchKinds = ["exact", "likely", "ambiguous", "missing", "new"] as const;
const repairMatchStrengths = ["exact", "likely", "weak"] as const;
const weights = ["rarely", "less-often", "normal", "more-often", "much-more-often"] as const;
const intendedUses = ["general", "focus", "reading", "relaxation", "energizing"] as const;
const signals = ["lyrics", "sudden-changes", "high-intensity", "calm", "repetitive", "energizing"] as const;
const snoozeScopes = ["playlist", "all-playlists"] as const;
const repeatModes = ["off", "all", "one"] as const;
const rootStatuses = ["available", "missing", "needs-relink"] as const;
const playlistAssignmentKinds = ["project-focus", "project-break", "calendar-event", "context-assignment"] as const;

function object(value: unknown, label: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error(`${label} must be an object`);
  return value as Record<string, unknown>;
}
function string(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`${label} must be a string`);
  return value;
}
function number(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) throw new Error(`${label} must be a safe integer`);
  return value;
}
function finite(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`${label} must be finite`);
  return value;
}
function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} must be a boolean`);
  return value;
}
function nullable<T>(value: unknown, parse: (value: unknown, label: string) => T, label: string): T | null {
  return value === null ? null : parse(value, label);
}
function array<T>(value: unknown, parse: (value: unknown, label: string) => T, label: string): T[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((entry, index) => parse(entry, `${label}[${index}]`));
}
function enumeration<const T extends readonly string[]>(value: unknown, values: T, label: string): T[number] {
  if (typeof value !== "string" || !values.includes(value)) throw new Error(`${label} is not supported`);
  return value as T[number];
}

export function parseWriteReceipt(value: unknown, label = "music write receipt"): MusicWriteReceipt {
  const row = object(value, label); return { id: string(row.id, `${label}.id`), version: number(row.version, `${label}.version`) };
}
export function parseMusicCount(value: unknown, label = "music count"): number {
  return number(value, label);
}
export function parseItemRepairPreview(value: unknown): MusicItemRepairPreview {
  const row = object(value, "music item repair preview");
  return {
    itemId: string(row.itemId, "music item repair preview.itemId"),
    folderPath: string(row.folderPath, "music item repair preview.folderPath"),
    relativePath: string(row.relativePath, "music item repair preview.relativePath"),
    title: string(row.title, "music item repair preview.title"),
    artist: string(row.artist, "music item repair preview.artist"),
    album: string(row.album, "music item repair preview.album"),
    durationMs: nullable(row.durationMs, number, "music item repair preview.durationMs"),
    fileSizeBytes: number(row.fileSizeBytes, "music item repair preview.fileSizeBytes"),
    lightweightFingerprint: string(row.lightweightFingerprint, "music item repair preview.lightweightFingerprint"),
    strongFingerprint: string(row.strongFingerprint, "music item repair preview.strongFingerprint"),
    matchStrength: enumeration(row.matchStrength, repairMatchStrengths, "music item repair preview.matchStrength"),
    reasons: array(row.reasons, string, "music item repair preview.reasons"),
  };
}
export function parseYouTubeSnapshotResult(value: unknown): MusicYouTubeSnapshotResult {
  const row = object(value, "YouTube playlist snapshot");
  return {
    collectionId: string(row.collectionId, "YouTube playlist snapshot.collectionId"),
    canonicalItemCount: number(row.canonicalItemCount, "YouTube playlist snapshot.canonicalItemCount"),
    newlyDiscoveredCount: number(row.newlyDiscoveredCount, "YouTube playlist snapshot.newlyDiscoveredCount"),
    repeatedVideoCount: number(row.repeatedVideoCount, "YouTube playlist snapshot.repeatedVideoCount"),
    generation: number(row.generation, "YouTube playlist snapshot.generation"),
  };
}
export function parseRefreshJobProgress(value: unknown, label = "music refresh progress"): MusicRefreshJobProgress {
  const row = object(value, label); return {
    jobId: string(row.jobId, `${label}.jobId`), collectionId: string(row.collectionId, `${label}.collectionId`), rootId: nullable(row.rootId, string, `${label}.rootId`), kind: enumeration(row.kind, collectionKinds, `${label}.kind`), state: enumeration(row.state, refreshJobStates, `${label}.state`), generation: number(row.generation, `${label}.generation`), discoveredCount: number(row.discoveredCount, `${label}.discoveredCount`), processedCount: number(row.processedCount, `${label}.processedCount`), skippedCount: number(row.skippedCount, `${label}.skippedCount`), issueCount: number(row.issueCount, `${label}.issueCount`), truncatedCount: number(row.truncatedCount, `${label}.truncatedCount`), absenceDetermined: boolean(row.absenceDetermined, `${label}.absenceDetermined`), statusMessage: string(row.statusMessage, `${label}.statusMessage`), requestedAtMs: number(row.requestedAtMs, `${label}.requestedAtMs`), startedAtMs: nullable(row.startedAtMs, number, `${label}.startedAtMs`), finishedAtMs: nullable(row.finishedAtMs, number, `${label}.finishedAtMs`), updatedAtMs: number(row.updatedAtMs, `${label}.updatedAtMs`),
  };
}
export function parseDeleteImpact(value: unknown, label = "playlist delete impact"): MusicPlaylistDeleteImpact {
  const row = object(value, label); return {
    membershipCount: number(row.membershipCount, `${label}.membershipCount`),
    projectFocusAssignmentCount: number(row.projectFocusAssignmentCount, `${label}.projectFocusAssignmentCount`),
    projectBreakAssignmentCount: number(row.projectBreakAssignmentCount, `${label}.projectBreakAssignmentCount`),
    calendarAssignmentCount: number(row.calendarAssignmentCount, `${label}.calendarAssignmentCount`),
    contextAssignmentCount: number(row.contextAssignmentCount, `${label}.contextAssignmentCount`),
    assignments: array(row.assignments, (entry, entryLabel) => {
      const assignment = object(entry, entryLabel);
      return { kind: enumeration(assignment.kind, playlistAssignmentKinds, `${entryLabel}.kind`), id: string(assignment.id, `${entryLabel}.id`), label: string(assignment.label, `${entryLabel}.label`) };
    }, `${label}.assignments`),
  };
}
function parseItemEntry(value: unknown, label: string): MusicItemListEntry {
  const row = object(value, label); return {
    id: string(row.id, `${label}.id`), identityKey: string(row.identityKey, `${label}.identityKey`),
    sourceKind: enumeration(row.sourceKind, sourceKinds, `${label}.sourceKind`), mediaKind: enumeration(row.mediaKind, mediaKinds, `${label}.mediaKind`),
    title: string(row.title, `${label}.title`), artist: string(row.artist, `${label}.artist`), album: string(row.album, `${label}.album`),
    localRootId: nullable(row.localRootId, string, `${label}.localRootId`), relativePath: nullable(row.relativePath, string, `${label}.relativePath`),
    sourceCollectionIds: array(row.sourceCollectionIds, string, `${label}.sourceCollectionIds`),
    originalArtworkIdentity: nullable(row.originalArtworkIdentity, string, `${label}.originalArtworkIdentity`), artworkOverride: nullable(row.artworkOverride, string, `${label}.artworkOverride`),
    durationMs: nullable(row.durationMs, number, `${label}.durationMs`), availability: enumeration(row.availability, itemAvailability, `${label}.availability`),
    reviewState: enumeration(row.reviewState, reviewStates, `${label}.reviewState`), discoveredAtMs: number(row.discoveredAtMs, `${label}.discoveredAtMs`),
    updatedAtMs: number(row.updatedAtMs, `${label}.updatedAtMs`), version: number(row.version, `${label}.version`), playlistCount: number(row.playlistCount, `${label}.playlistCount`),
    activeSnoozeCount: number(row.activeSnoozeCount, `${label}.activeSnoozeCount`), lastPlayedAtMs: nullable(row.lastPlayedAtMs, number, `${label}.lastPlayedAtMs`),
    playCount: number(row.playCount, `${label}.playCount`), membershipId: nullable(row.membershipId, string, `${label}.membershipId`),
    membershipPosition: nullable(row.membershipPosition, number, `${label}.membershipPosition`),
    membershipWeight: row.membershipWeight === null ? null : enumeration(row.membershipWeight, weights, `${label}.membershipWeight`),
    membershipEnabled: nullable(row.membershipEnabled, boolean, `${label}.membershipEnabled`), membershipVersion: nullable(row.membershipVersion, number, `${label}.membershipVersion`),
  };
}
export function parseItemWindow(value: unknown, label = "music item window"): MusicItemWindow {
  const row = object(value, label); return {
    items: array(row.items, parseItemEntry, `${label}.items`),
    groups: array(row.groups, (entry, entryLabel) => { const group = object(entry, entryLabel); return { key: string(group.key, `${entryLabel}.key`), count: number(group.count, `${entryLabel}.count`) }; }, `${label}.groups`),
    totalCount: number(row.totalCount, `${label}.totalCount`), offset: number(row.offset, `${label}.offset`), limit: number(row.limit, `${label}.limit`),
  };
}
function parsePlaylistSummary(value: unknown, label: string): MusicPlaylistSummary {
  const row = object(value, label); return {
    id: string(row.id, `${label}.id`), name: string(row.name, `${label}.name`), icon: string(row.icon, `${label}.icon`),
    shuffleEnabled: boolean(row.shuffleEnabled, `${label}.shuffleEnabled`), mixEnabled: boolean(row.mixEnabled, `${label}.mixEnabled`), repeatMode: enumeration(row.repeatMode, repeatModes, `${label}.repeatMode`),
    intendedUses: array(row.intendedUses, (entry, entryLabel) => enumeration(entry, intendedUses, entryLabel), `${label}.intendedUses`),
    sortOrder: number(row.sortOrder, `${label}.sortOrder`),
    totalCount: number(row.totalCount, `${label}.totalCount`), eligibleCount: number(row.eligibleCount, `${label}.eligibleCount`), unavailableCount: number(row.unavailableCount, `${label}.unavailableCount`),
    snoozedCount: number(row.snoozedCount, `${label}.snoozedCount`), localCount: number(row.localCount, `${label}.localCount`), onlineCount: number(row.onlineCount, `${label}.onlineCount`), version: number(row.version, `${label}.version`),
  };
}
export const parsePlaylistSummaries = (value: unknown): MusicPlaylistSummary[] => array(value, parsePlaylistSummary, "playlist summaries");
function parseSourceSummary(value: unknown, label: string): MusicSourceSummary {
  const row = object(value, label); return {
    id: string(row.id, `${label}.id`), kind: enumeration(row.kind, collectionKinds, `${label}.kind`), name: string(row.name, `${label}.name`),
    refreshState: enumeration(row.refreshState, refreshStates, `${label}.refreshState`), lastSuccessfulRefreshAtMs: nullable(row.lastSuccessfulRefreshAtMs, number, `${label}.lastSuccessfulRefreshAtMs`),
    localRootId: nullable(row.localRootId, string, `${label}.localRootId`), youtubePlaylistId: nullable(row.youtubePlaylistId, string, `${label}.youtubePlaylistId`),
    itemCount: number(row.itemCount, `${label}.itemCount`), missingCount: number(row.missingCount, `${label}.missingCount`), newCount: number(row.newCount, `${label}.newCount`), unreviewedCount: number(row.unreviewedCount, `${label}.unreviewedCount`), unavailableCount: number(row.unavailableCount, `${label}.unavailableCount`), ambiguousCount: number(row.ambiguousCount, `${label}.ambiguousCount`), openIssueCount: number(row.openIssueCount, `${label}.openIssueCount`), health: enumeration(row.health, sourceHealthStates, `${label}.health`), discoveryEnabled: boolean(row.discoveryEnabled, `${label}.discoveryEnabled`), version: number(row.version, `${label}.version`),
  };
}
export const parseSourceSummaries = (value: unknown): MusicSourceSummary[] => array(value, parseSourceSummary, "source summaries");
function parseIssue(value: unknown, label: string): MusicIssue {
  const row = object(value, label); return { id: string(row.id, `${label}.id`), issueKind: string(row.issueKind, `${label}.issueKind`), itemId: nullable(row.itemId, string, `${label}.itemId`), playlistId: nullable(row.playlistId, string, `${label}.playlistId`), collectionId: nullable(row.collectionId, string, `${label}.collectionId`), rootId: nullable(row.rootId, string, `${label}.rootId`), relativePath: nullable(row.relativePath, string, `${label}.relativePath`), actionRequired: boolean(row.actionRequired, `${label}.actionRequired`), message: string(row.message, `${label}.message`), createdAtMs: number(row.createdAtMs, `${label}.createdAtMs`) };
}
export const parseIssues = (value: unknown): MusicIssue[] => array(value, parseIssue, "music issues");
function parseLibraryItem(value: unknown, label: string): MusicLibraryItem {
  const row = object(value, label); return {
    id: string(row.id, `${label}.id`), identityKey: string(row.identityKey, `${label}.identityKey`), sourceKind: enumeration(row.sourceKind, sourceKinds, `${label}.sourceKind`), mediaKind: enumeration(row.mediaKind, mediaKinds, `${label}.mediaKind`),
    youtubeVideoId: nullable(row.youtubeVideoId, string, `${label}.youtubeVideoId`), originalTitle: string(row.originalTitle, `${label}.originalTitle`), originalArtist: string(row.originalArtist, `${label}.originalArtist`), originalAlbum: string(row.originalAlbum, `${label}.originalAlbum`), originalTrackNumber: nullable(row.originalTrackNumber, number, `${label}.originalTrackNumber`), originalArtworkIdentity: nullable(row.originalArtworkIdentity, string, `${label}.originalArtworkIdentity`), youtubeResolutionState: nullable(row.youtubeResolutionState, (entry, entryLabel) => enumeration(entry, youtubeResolutionStates, entryLabel), `${label}.youtubeResolutionState`),
    titleOverride: nullable(row.titleOverride, string, `${label}.titleOverride`), artistOverride: nullable(row.artistOverride, string, `${label}.artistOverride`), albumOverride: nullable(row.albumOverride, string, `${label}.albumOverride`), artworkOverride: nullable(row.artworkOverride, string, `${label}.artworkOverride`),
    durationMs: nullable(row.durationMs, number, `${label}.durationMs`), availability: enumeration(row.availability, itemAvailability, `${label}.availability`), reviewState: enumeration(row.reviewState, reviewStates, `${label}.reviewState`), reviewChangedAtMs: nullable(row.reviewChangedAtMs, number, `${label}.reviewChangedAtMs`), reviewDeferredUntilMs: nullable(row.reviewDeferredUntilMs, number, `${label}.reviewDeferredUntilMs`), discoveredAtMs: number(row.discoveredAtMs, `${label}.discoveredAtMs`), updatedAtMs: number(row.updatedAtMs, `${label}.updatedAtMs`), version: number(row.version, `${label}.version`),
  };
}
function parseLocation(value: unknown, label: string): MusicLocalLocation {
  const row = object(value, label); return { id: string(row.id, `${label}.id`), itemId: string(row.itemId, `${label}.itemId`), rootId: string(row.rootId, `${label}.rootId`), relativePath: string(row.relativePath, `${label}.relativePath`), fileSizeBytes: nullable(row.fileSizeBytes, number, `${label}.fileSizeBytes`), modifiedAtMs: nullable(row.modifiedAtMs, number, `${label}.modifiedAtMs`), lightweightFingerprint: nullable(row.lightweightFingerprint, string, `${label}.lightweightFingerprint`), strongFingerprint: nullable(row.strongFingerprint, string, `${label}.strongFingerprint`), availability: enumeration(row.availability, locationAvailability, `${label}.availability`), lastSeenGeneration: nullable(row.lastSeenGeneration, number, `${label}.lastSeenGeneration`), firstSeenAtMs: number(row.firstSeenAtMs, `${label}.firstSeenAtMs`), updatedAtMs: number(row.updatedAtMs, `${label}.updatedAtMs`) };
}
function parseMembership(value: unknown, label: string): MusicPlaylistMembership {
  const row = object(value, label); return { id: string(row.id, `${label}.id`), playlistId: string(row.playlistId, `${label}.playlistId`), itemId: string(row.itemId, `${label}.itemId`), position: number(row.position, `${label}.position`), weight: enumeration(row.weight, weights, `${label}.weight`), enabled: boolean(row.enabled, `${label}.enabled`), startMs: nullable(row.startMs, number, `${label}.startMs`), endMs: nullable(row.endMs, number, `${label}.endMs`), volume: nullable(row.volume, finite, `${label}.volume`), rate: nullable(row.rate, finite, `${label}.rate`), updatedAtMs: number(row.updatedAtMs, `${label}.updatedAtMs`), createdAtMs: number(row.createdAtMs, `${label}.createdAtMs`), version: number(row.version, `${label}.version`) };
}
function parseMembershipSkipRange(value: unknown, label: string): MusicMembershipSkipRange {
  const row = object(value, label); return { id: string(row.id, `${label}.id`), membershipId: string(row.membershipId, `${label}.membershipId`), startMs: number(row.startMs, `${label}.startMs`), endMs: number(row.endMs, `${label}.endMs`), sortOrder: number(row.sortOrder, `${label}.sortOrder`) };
}
function parseSnooze(value: unknown, label: string): MusicSnooze {
  const row = object(value, label); return { id: string(row.id, `${label}.id`), itemId: string(row.itemId, `${label}.itemId`), scope: enumeration(row.scope, snoozeScopes, `${label}.scope`), playlistId: nullable(row.playlistId, string, `${label}.playlistId`), startsAtMs: number(row.startsAtMs, `${label}.startsAtMs`), endsAtMs: nullable(row.endsAtMs, number, `${label}.endsAtMs`), reason: string(row.reason, `${label}.reason`), createdAtMs: number(row.createdAtMs, `${label}.createdAtMs`) };
}
function parseStatistics(value: unknown, label: string): MusicListeningStatistics {
  const row = object(value, label); return { itemId: string(row.itemId, `${label}.itemId`), lastPlayedAtMs: nullable(row.lastPlayedAtMs, number, `${label}.lastPlayedAtMs`), playCount: number(row.playCount, `${label}.playCount`), completionCount: number(row.completionCount, `${label}.completionCount`), skipCount: number(row.skipCount, `${label}.skipCount`), updatedAtMs: number(row.updatedAtMs, `${label}.updatedAtMs`) };
}
export function parseInspectorDetail(value: unknown): MusicInspectorDetail {
  const row = object(value, "music inspector"); return { item: parseLibraryItem(row.item, "music inspector.item"), locations: array(row.locations, parseLocation, "music inspector.locations"), memberships: array(row.memberships, parseMembership, "music inspector.memberships"), membershipSkipRanges: array(row.membershipSkipRanges, parseMembershipSkipRange, "music inspector.membershipSkipRanges"), snoozes: array(row.snoozes, parseSnooze, "music inspector.snoozes"), signals: array(row.signals, (entry, label) => enumeration(entry, signals, label), "music inspector.signals"), statistics: row.statistics === null ? null : parseStatistics(row.statistics, "music inspector.statistics"), sourceCollectionIds: array(row.sourceCollectionIds, string, "music inspector.sourceCollectionIds") };
}
function parseRoot(value: unknown, label: string): MusicLocalRoot { const row = object(value, label); return { id: string(row.id, `${label}.id`), name: string(row.name, `${label}.name`), createdAtMs: number(row.createdAtMs, `${label}.createdAtMs`), updatedAtMs: number(row.updatedAtMs, `${label}.updatedAtMs`), version: number(row.version, `${label}.version`) }; }
export const parseRoots = (value: unknown): MusicLocalRoot[] => array(value, parseRoot, "music roots");
function parseCollection(value: unknown, label: string): MusicSourceCollection { const row = object(value, label); return { id: string(row.id, `${label}.id`), kind: enumeration(row.kind, collectionKinds, `${label}.kind`), identityKey: string(row.identityKey, `${label}.identityKey`), name: string(row.name, `${label}.name`), localRootId: nullable(row.localRootId, string, `${label}.localRootId`), youtubePlaylistId: nullable(row.youtubePlaylistId, string, `${label}.youtubePlaylistId`), refreshState: enumeration(row.refreshState, refreshStates, `${label}.refreshState`), lastSuccessfulRefreshAtMs: nullable(row.lastSuccessfulRefreshAtMs, number, `${label}.lastSuccessfulRefreshAtMs`), previousSuccessfulRefreshAtMs: nullable(row.previousSuccessfulRefreshAtMs, number, `${label}.previousSuccessfulRefreshAtMs`), lastRefreshErrorCode: nullable(row.lastRefreshErrorCode, string, `${label}.lastRefreshErrorCode`), snapshotGeneration: number(row.snapshotGeneration, `${label}.snapshotGeneration`), createdAtMs: number(row.createdAtMs, `${label}.createdAtMs`), updatedAtMs: number(row.updatedAtMs, `${label}.updatedAtMs`), version: number(row.version, `${label}.version`), discoveryEnabled: boolean(row.discoveryEnabled, `${label}.discoveryEnabled`), removedAtMs: nullable(row.removedAtMs, number, `${label}.removedAtMs`) }; }
export const parseCollections = (value: unknown): MusicSourceCollection[] => array(value, parseCollection, "music collections");
export function parsePlaylist(value: unknown): MusicPlaylist { const row = object(value, "music playlist"); return { id: string(row.id, "music playlist.id"), name: string(row.name, "music playlist.name"), icon: string(row.icon, "music playlist.icon"), shuffleEnabled: boolean(row.shuffleEnabled, "music playlist.shuffleEnabled"), mixEnabled: boolean(row.mixEnabled, "music playlist.mixEnabled"), repeatMode: enumeration(row.repeatMode, repeatModes, "music playlist.repeatMode"), intendedUses: array(row.intendedUses, (entry, label) => enumeration(entry, intendedUses, label), "music playlist.intendedUses"), sortOrder: number(row.sortOrder, "music playlist.sortOrder"), createdAtMs: number(row.createdAtMs, "music playlist.createdAtMs"), updatedAtMs: number(row.updatedAtMs, "music playlist.updatedAtMs"), version: number(row.version, "music playlist.version") }; }
function parseBinding(value: unknown, label: string): LocalRootBinding { const row = object(value, label); return { rootId: string(row.rootId, `${label}.rootId`), folderPath: nullable(row.folderPath, string, `${label}.folderPath`), status: enumeration(row.status, rootStatuses, `${label}.status`) }; }
export const parseBindings = (value: unknown): LocalRootBinding[] => array(value, parseBinding, "music root bindings");
export const parseBindingResult = (value: unknown): LocalRootBinding => parseBinding(value, "music root binding");
function parsePlaylistPlaybackEntry(value: unknown, label: string): MusicPlaylistPlaybackEntry {
  const row = object(value, label);
  return {
    membershipId: string(row.membershipId, `${label}.membershipId`),
    itemId: string(row.itemId, `${label}.itemId`),
    identityKey: string(row.identityKey, `${label}.identityKey`),
    sourceKind: enumeration(row.sourceKind, sourceKinds, `${label}.sourceKind`),
    youtubeVideoId: nullable(row.youtubeVideoId, string, `${label}.youtubeVideoId`),
    youtubeResolutionState: nullable(row.youtubeResolutionState, (entry, entryLabel) => enumeration(entry, youtubeResolutionStates, entryLabel), `${label}.youtubeResolutionState`),
    title: string(row.title, `${label}.title`),
    originalArtworkIdentity: nullable(row.originalArtworkIdentity, string, `${label}.originalArtworkIdentity`),
    artworkOverride: nullable(row.artworkOverride, string, `${label}.artworkOverride`),
    availability: enumeration(row.availability, itemAvailability, `${label}.availability`),
    rootId: nullable(row.rootId, string, `${label}.rootId`),
    relativePath: nullable(row.relativePath, string, `${label}.relativePath`),
    position: number(row.position, `${label}.position`),
    weight: enumeration(row.weight, weights, `${label}.weight`),
    enabled: boolean(row.enabled, `${label}.enabled`),
    startMs: nullable(row.startMs, number, `${label}.startMs`),
    endMs: nullable(row.endMs, number, `${label}.endMs`),
    volume: nullable(row.volume, number, `${label}.volume`),
    rate: nullable(row.rate, number, `${label}.rate`),
    snoozed: boolean(row.snoozed, `${label}.snoozed`),
    snoozedUntil: nullable(row.snoozedUntil, number, `${label}.snoozedUntil`),
    snoozedIndefinitely: boolean(row.snoozedIndefinitely, `${label}.snoozedIndefinitely`),
    skipRanges: array(row.skipRanges, parseMembershipSkipRange, `${label}.skipRanges`),
  };
}
export const parsePlaylistPlaybackEntries = (value: unknown): MusicPlaylistPlaybackEntry[] => array(value, parsePlaylistPlaybackEntry, "playlist playback entries");
export function parseRelinkPlanSummary(value: unknown): MusicRelinkPlanSummary {
  const row = object(value, "music relink plan");
  return {
    id: string(row.id, "music relink plan.id"),
    rootId: string(row.rootId, "music relink plan.rootId"),
    state: enumeration(row.state, relinkPlanStates, "music relink plan.state"),
    exactCount: number(row.exactCount, "music relink plan.exactCount"),
    likelyCount: number(row.likelyCount, "music relink plan.likelyCount"),
    ambiguousCount: number(row.ambiguousCount, "music relink plan.ambiguousCount"),
    missingCount: number(row.missingCount, "music relink plan.missingCount"),
    newCount: number(row.newCount, "music relink plan.newCount"),
    createdAtMs: number(row.createdAtMs, "music relink plan.createdAtMs"),
    updatedAtMs: number(row.updatedAtMs, "music relink plan.updatedAtMs"),
  };
}
export function parseRelinkPlanWindow(value: unknown): MusicRelinkPlanWindow {
  const row = object(value, "music relink plan window");
  return {
    entries: array(row.entries, (entry, label) => {
      const item = object(entry, label);
      return {
        id: string(item.id, `${label}.id`),
        matchKind: enumeration(item.matchKind, relinkMatchKinds, `${label}.matchKind`),
        oldLocationId: nullable(item.oldLocationId, string, `${label}.oldLocationId`),
        suggestedItemId: nullable(item.suggestedItemId, string, `${label}.suggestedItemId`),
        candidateRelativePath: nullable(item.candidateRelativePath, string, `${label}.candidateRelativePath`),
        candidateItemIds: array(item.candidateItemIds, string, `${label}.candidateItemIds`),
        fileSizeBytes: nullable(item.fileSizeBytes, number, `${label}.fileSizeBytes`),
        resolvedItemId: nullable(item.resolvedItemId, string, `${label}.resolvedItemId`),
        resolvedAtMs: nullable(item.resolvedAtMs, number, `${label}.resolvedAtMs`),
      };
    }, "music relink plan window.entries"),
    totalCount: number(row.totalCount, "music relink plan window.totalCount"),
    offset: number(row.offset, "music relink plan window.offset"),
    limit: number(row.limit, "music relink plan window.limit"),
  };
}
export function parseSourceRemovalImpact(value: unknown): MusicSourceRemovalImpact {
  const row = object(value, "music source removal impact");
  return {
    collectionId: string(row.collectionId, "music source removal impact.collectionId"),
    itemCount: number(row.itemCount, "music source removal impact.itemCount"),
    membershipCount: number(row.membershipCount, "music source removal impact.membershipCount"),
    sharedItemCount: number(row.sharedItemCount, "music source removal impact.sharedItemCount"),
    orphanedItemCount: number(row.orphanedItemCount, "music source removal impact.orphanedItemCount"),
    activeRefreshCount: number(row.activeRefreshCount, "music source removal impact.activeRefreshCount"),
  };
}
