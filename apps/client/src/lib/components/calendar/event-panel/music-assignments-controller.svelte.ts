/**
 * Music assignment drafts of the event panel: loading the event's snapshot and override assignments
 * (or a copy of the project defaults for a new event), adopting a new project's defaults, tracking
 * edits against the saved drafts, and discarding responses that a newer load or edit superseded.
 */
import { getMusicContextAssignments, getMusicPlaylistSummaries } from "$lib/music/platform-library";
import { completeMusicAssignmentDrafts, musicAssignmentDraftsEqual } from "$lib/music/assignment-draft";
import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";
import type { MusicPlaylistSummary } from "$lib/music/library/contracts";

export interface EventPanelMusicAssignmentsControllerOptions {
  /** Whether this platform offers Music context assignments; every load is skipped when it does not. */
  available: boolean;
  /** Key of the current panel initialization; a load for another key is discarded. */
  initKey: () => string;
  /** Event, or series an occurrence belongs to, whose assignments the panel edits; null for a new event. */
  eventId: () => string | null;
  projectId: () => string | undefined;
}

function copiedProjectSnapshot(
  assignments: readonly MusicContextAssignmentDraft[],
  projectId: string,
): MusicContextAssignmentDraft[] {
  return completeMusicAssignmentDrafts(assignments).map((assignment) => ({
    ...assignment,
    provenanceKind: "copied-project",
    provenanceId: projectId,
  }));
}

/** Owns the event panel's Music snapshot and override drafts, their saved copies, and the playlists. */
export class EventPanelMusicAssignmentsController {
  musicSnapshots = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  savedMusicSnapshots = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  musicOverrides = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  savedMusicOverrides = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  musicPlaylists = $state<MusicPlaylistSummary[]>([]);
  musicAssignmentsLoading = $state(false);
  musicAssignmentsError = $state<string | null>(null);
  musicAssignmentsReadyKey = $state<string | null>(null);

  private musicLoadGeneration = 0;
  private musicSnapshotGeneration = 0;
  private musicOverrideGeneration = 0;

  constructor(private readonly options: EventPanelMusicAssignmentsControllerOptions) {}

  /** Whether the loaded drafts differ from the saved ones for the current initialization. */
  get musicAssignmentsDirty(): boolean {
    return this.musicAssignmentsReadyKey === this.options.initKey()
      && (!musicAssignmentDraftsEqual(this.musicSnapshots, this.savedMusicSnapshots)
        || !musicAssignmentDraftsEqual(this.musicOverrides, this.savedMusicOverrides));
  }

  async initializeMusicAssignments(key: string): Promise<void> {
    if (!this.options.available) return;
    const generation = ++this.musicLoadGeneration;
    const snapshotGeneration = ++this.musicSnapshotGeneration;
    const overrideGeneration = ++this.musicOverrideGeneration;
    this.musicAssignmentsLoading = true;
    this.musicAssignmentsError = null;
    const eventId = this.options.eventId();
    const projectId = this.options.projectId();
    const assignmentRequests = eventId
      ? Promise.all([
          getMusicContextAssignments("event-snapshot", eventId),
          getMusicContextAssignments("event-override", eventId),
        ])
      : projectId
        ? getMusicContextAssignments("project-default", projectId).then((assignments) => [
            copiedProjectSnapshot(assignments, this.options.projectId() as string),
            [],
          ] as const)
        : Promise.resolve([[], []] as const);
    const [assignmentResult, playlistResult] = await Promise.allSettled([
      assignmentRequests,
      getMusicPlaylistSummaries(Date.now(), 0, 500),
    ]);
    if (generation !== this.musicLoadGeneration || key !== this.options.initKey()) return;
    if (assignmentResult.status === "fulfilled") {
      const [snapshots, overrides] = assignmentResult.value;
      this.savedMusicSnapshots = completeMusicAssignmentDrafts(snapshots);
      if (snapshotGeneration === this.musicSnapshotGeneration) {
        this.musicSnapshots = completeMusicAssignmentDrafts(snapshots);
      }
      this.savedMusicOverrides = completeMusicAssignmentDrafts(overrides);
      if (overrideGeneration === this.musicOverrideGeneration) {
        this.musicOverrides = completeMusicAssignmentDrafts(overrides);
      }
      this.musicAssignmentsReadyKey = key;
    }
    if (playlistResult.status === "fulfilled") this.musicPlaylists = playlistResult.value;
    const failures = [assignmentResult, playlistResult]
      .filter((result): result is PromiseRejectedResult => result.status === "rejected")
      .map((result) => result.reason instanceof Error ? result.reason.message : String(result.reason));
    this.musicAssignmentsError = failures.length > 0 ? failures.join(" ") : null;
    this.musicAssignmentsLoading = false;
  }

  async adoptProjectMusicSnapshot(projectId: string | undefined): Promise<void> {
    if (!this.options.available) return;
    const generation = ++this.musicSnapshotGeneration;
    this.musicAssignmentsLoading = true;
    this.musicAssignmentsError = null;
    try {
      const [assignments, playlists] = await Promise.all([
        projectId ? getMusicContextAssignments("project-default", projectId) : Promise.resolve([]),
        this.musicPlaylists.length === 0
          ? getMusicPlaylistSummaries(Date.now(), 0, 500)
          : Promise.resolve(this.musicPlaylists),
      ]);
      if (generation !== this.musicSnapshotGeneration) return;
      this.musicPlaylists = playlists;
      this.musicSnapshots = projectId
        ? copiedProjectSnapshot(assignments, projectId)
        : completeMusicAssignmentDrafts([]).map((assignment) => ({
            ...assignment,
            provenanceKind: "copied-project" as const,
            provenanceId: null,
          }));
    } catch (cause) {
      if (generation !== this.musicSnapshotGeneration) return;
      this.musicAssignmentsError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (generation === this.musicSnapshotGeneration) this.musicAssignmentsLoading = false;
    }
  }

  /** Applies override edits from the Music section, so a load still in flight keeps them. */
  setMusicOverrides(assignments: MusicContextAssignmentDraft[]): void {
    this.musicOverrideGeneration += 1;
    this.musicOverrides = assignments;
  }

  /** Records the current drafts as saved after a successful save. */
  markMusicAssignmentsSaved(): void {
    this.savedMusicSnapshots = completeMusicAssignmentDrafts(this.musicSnapshots);
    this.savedMusicOverrides = completeMusicAssignmentDrafts(this.musicOverrides);
  }

  /** Discards every load still in flight, as when the panel unmounts. */
  invalidate(): void {
    this.musicLoadGeneration += 1;
    this.musicSnapshotGeneration += 1;
    this.musicOverrideGeneration += 1;
  }
}
