import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";
import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
import { completeMusicAssignmentDrafts } from "$lib/music/assignment-draft";

const library = vi.hoisted(() => ({
  getMusicContextAssignments: vi.fn(),
  getMusicPlaylistSummaries: vi.fn(),
}));

vi.mock("$lib/music/platform-library", () => library);

const { EventPanelMusicAssignmentsController } = await import("./music-assignments-controller.svelte");

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function playlistAssignment(playlistId: string): MusicContextAssignmentDraft[] {
  const [first, ...rest] = completeMusicAssignmentDrafts([]);
  return [{ ...first!, behavior: "play-automatically", playlistId }, ...rest];
}

const playlists: MusicPlaylistSummary[] = [{
  id: "reading", name: "Reading", icon: "music", shuffleEnabled: false,
  mixEnabled: false, repeatMode: "off", intendedUses: [], orderKey: "a0",
  totalCount: 2, eligibleCount: 2, unavailableCount: 0, snoozedCount: 0,
  localCount: 2, onlineCount: 0, version: 1,
}];

function setup(overrides: { eventId?: string | null; projectId?: string; available?: boolean } = {}) {
  const state = { initKey: "key-1", eventId: overrides.eventId ?? null, projectId: overrides.projectId };
  const controller = new EventPanelMusicAssignmentsController({
    available: overrides.available ?? true,
    initKey: () => state.initKey,
    eventId: () => state.eventId,
    projectId: () => state.projectId,
  });
  return { controller, state };
}

beforeEach(() => {
  library.getMusicContextAssignments.mockReset();
  library.getMusicPlaylistSummaries.mockReset();
  library.getMusicPlaylistSummaries.mockResolvedValue(playlists);
});

describe("EventPanelMusicAssignmentsController", () => {
  it("does nothing where Music assignments are unavailable", async () => {
    const { controller } = setup({ available: false, eventId: "event-1" });
    await controller.initializeMusicAssignments("key-1");
    await controller.adoptProjectMusicSnapshot("project-1");
    expect(library.getMusicContextAssignments).not.toHaveBeenCalled();
    expect(controller.musicAssignmentsReadyKey).toBeNull();
  });

  it("loads an event's snapshot and override assignments as saved and clean", async () => {
    library.getMusicContextAssignments.mockImplementation(async (kind: string) =>
      kind === "event-snapshot" ? playlistAssignment("snapshot") : playlistAssignment("override"));
    const { controller } = setup({ eventId: "event-1" });
    await controller.initializeMusicAssignments("key-1");
    expect(library.getMusicContextAssignments).toHaveBeenCalledWith("event-snapshot", "event-1");
    expect(library.getMusicContextAssignments).toHaveBeenCalledWith("event-override", "event-1");
    expect(controller.musicSnapshots[0]!.playlistId).toBe("snapshot");
    expect(controller.musicOverrides[0]!.playlistId).toBe("override");
    expect(controller.musicPlaylists).toBe(playlists);
    expect(controller.musicAssignmentsReadyKey).toBe("key-1");
    expect(controller.musicAssignmentsLoading).toBe(false);
    expect(controller.musicAssignmentsDirty).toBe(false);
  });

  it("copies project defaults into the snapshot of a new event", async () => {
    library.getMusicContextAssignments.mockResolvedValue(playlistAssignment("project"));
    const { controller } = setup({ projectId: "project-1" });
    await controller.initializeMusicAssignments("key-1");
    expect(library.getMusicContextAssignments).toHaveBeenCalledWith("project-default", "project-1");
    expect(controller.musicSnapshots[0]).toMatchObject({
      playlistId: "project",
      provenanceKind: "copied-project",
      provenanceId: "project-1",
    });
    expect(controller.musicOverrides.every((assignment) => assignment.playlistId === null)).toBe(true);
  });

  it("discards a load for an initialization the panel moved past", async () => {
    library.getMusicContextAssignments.mockResolvedValue(playlistAssignment("stale"));
    const { controller, state } = setup({ eventId: "event-1" });
    const load = controller.initializeMusicAssignments("key-1");
    state.initKey = "key-2";
    await load;
    expect(controller.musicAssignmentsReadyKey).toBeNull();
    expect(controller.musicAssignmentsLoading).toBe(true);
  });

  it("keeps override edits made while a load is in flight and reports them dirty", async () => {
    const overrides = deferred<MusicContextAssignmentDraft[]>();
    library.getMusicContextAssignments.mockImplementation((kind: string) =>
      kind === "event-override" ? overrides.promise : Promise.resolve([]));
    const { controller } = setup({ eventId: "event-1" });
    const load = controller.initializeMusicAssignments("key-1");
    const edited = playlistAssignment("edited");
    controller.setMusicOverrides(edited);
    overrides.resolve(playlistAssignment("loaded"));
    await load;
    expect(controller.musicOverrides[0]!.playlistId).toBe("edited");
    expect(controller.savedMusicOverrides[0]!.playlistId).toBe("loaded");
    expect(controller.musicAssignmentsDirty).toBe(true);
    controller.markMusicAssignmentsSaved();
    expect(controller.musicAssignmentsDirty).toBe(false);
  });

  it("reports load failures and still records the playlists", async () => {
    library.getMusicContextAssignments.mockRejectedValue(new Error("Library offline"));
    const { controller } = setup({ eventId: "event-1" });
    await controller.initializeMusicAssignments("key-1");
    expect(controller.musicAssignmentsError).toBe("Library offline");
    expect(controller.musicAssignmentsReadyKey).toBeNull();
    expect(controller.musicPlaylists).toBe(playlists);
    expect(controller.musicAssignmentsLoading).toBe(false);
  });

  it("adopts a selected project's defaults without changing the saved snapshot", async () => {
    library.getMusicContextAssignments.mockResolvedValue([]);
    const { controller } = setup({ eventId: "event-1" });
    await controller.initializeMusicAssignments("key-1");
    library.getMusicContextAssignments.mockResolvedValue(playlistAssignment("project"));
    await controller.adoptProjectMusicSnapshot("project-2");
    expect(controller.musicSnapshots[0]).toMatchObject({ playlistId: "project", provenanceId: "project-2" });
    expect(controller.savedMusicSnapshots[0]!.playlistId).toBeNull();
    expect(controller.musicAssignmentsDirty).toBe(true);
  });

  it("clears the snapshot to unassigned project copies when the project is removed", async () => {
    const { controller } = setup();
    await controller.adoptProjectMusicSnapshot(undefined);
    expect(library.getMusicContextAssignments).not.toHaveBeenCalled();
    expect(controller.musicSnapshots.every((assignment) => (
      assignment.provenanceKind === "copied-project" && assignment.provenanceId === null
    ))).toBe(true);
  });

  it("lets a newer project adoption win over an older one", async () => {
    const first = deferred<MusicContextAssignmentDraft[]>();
    library.getMusicContextAssignments
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(playlistAssignment("second"));
    const { controller } = setup();
    const older = controller.adoptProjectMusicSnapshot("project-1");
    await controller.adoptProjectMusicSnapshot("project-2");
    first.resolve(playlistAssignment("first"));
    await older;
    expect(controller.musicSnapshots[0]!.playlistId).toBe("second");
    expect(controller.musicAssignmentsLoading).toBe(false);
  });

  it("discards every load in flight after invalidation", async () => {
    const assignments = deferred<MusicContextAssignmentDraft[]>();
    library.getMusicContextAssignments.mockReturnValue(assignments.promise);
    const { controller } = setup({ eventId: "event-1" });
    const load = controller.initializeMusicAssignments("key-1");
    controller.invalidate();
    assignments.resolve(playlistAssignment("late"));
    await load;
    expect(controller.musicAssignmentsReadyKey).toBeNull();
    expect(controller.musicSnapshots[0]!.playlistId).toBeNull();
  });
});
