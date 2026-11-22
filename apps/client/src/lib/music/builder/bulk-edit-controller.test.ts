import { describe, expect, it, vi } from "vitest";
import type { MusicLibraryController } from "$lib/music/library/controller.svelte";
import type { MusicItemSignal, MusicItemSignalsWrite, MusicPlaylistSummary } from "$lib/music/library/contracts";
import { MusicBulkEditController } from "./bulk-edit-controller.svelte";

const { applyMusicReviewSelection, bulkEditMusicMemberships, bulkSetMusicReviewState, getMusicInspectorDetail, getMusicMembershipMatrix, setMusicItemSignals } = vi.hoisted(() => ({
  getMusicInspectorDetail: vi.fn(async (itemId: string) => ({ item: { id: itemId }, signals: [] as MusicItemSignal[] })),
  setMusicItemSignals: vi.fn(async (_write: MusicItemSignalsWrite) => undefined),
  applyMusicReviewSelection: vi.fn(async () => ({
    membershipChangedCount: 2,
    reviewChangedCount: 2,
    items: [{ id: "item-1", version: 2 }, { id: "item-2", version: 3 }],
  })),
  bulkEditMusicMemberships: vi.fn(async () => ({ changedCount: 1 })),
  bulkSetMusicReviewState: vi.fn(async () => ({ changedCount: 2 })),
  getMusicMembershipMatrix: vi.fn(async () => [
    { itemId: "item-1", playlistId: "focus", weight: "normal" },
    { itemId: "item-2", playlistId: "all", weight: "normal" },
    { itemId: "item-1", playlistId: "all", weight: "normal" },
  ]),
}));

vi.mock("$lib/api/music-library", () => ({
  applyMusicReviewSelection,
  getMusicInspectorDetail,
  setMusicItemSignals,
  getMusicMembershipMatrix,
  bulkEditMusicMemberships,
  bulkSetMusicReviewState,
  createMusicPlaylist: vi.fn(async () => ({ id: "created", version: 1 })),
}));

const summary = (id: string): MusicPlaylistSummary => ({
  id, orderKey: "a0", name: id, icon: "lucide:list-music", shuffleEnabled: true, mixEnabled: false, repeatMode: "all", intendedUses: [],
  totalCount: 0, eligibleCount: 0, unavailableCount: 0, snoozedCount: 0, localCount: 0,
  onlineCount: 0, version: 1,
});

describe("MusicBulkEditController", () => {
  it("projects checked, mixed, and unchecked playlist states without losing mixed intent", async () => {
    const library = { refreshAfterMutation: vi.fn(async () => true) } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    await controller.open(["item-1", "item-2"], [summary("all"), summary("focus"), summary("empty")]);
    expect(controller.states).toEqual({ all: "checked", focus: "mixed", empty: "unchecked" });
    expect(controller.hasExistingMemberships).toBe(true);
    controller.toggle("focus");
    expect(controller.states.focus).toBe("checked");
    expect(controller.membershipsChanged).toBe(true);
    controller.toggle("focus");
    expect(controller.states.focus).toBe("unchecked");
    controller.toggle("focus");
    expect(controller.states.focus).toBe("mixed");
    expect(controller.membershipsChanged).toBe(false);
    expect(controller.touchedPlaylistIds).not.toContain("focus");
  });

  it("applies memberships and marks every selected track reviewed without replacing the item window", async () => {
    const library = {
      currentWindow: { items: [
        { id: "item-1", reviewState: "unreviewed", updatedAtMs: 1, version: 1 },
        { id: "item-2", reviewState: "deferred", updatedAtMs: 1, version: 2 },
      ] },
      refreshAfterMutation: vi.fn(async () => true),
      refreshSummariesAfterMutation: vi.fn(async () => true),
    } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    await controller.open(["item-1", "item-2"], [summary("all"), summary("focus")]);
    controller.toggle("focus");

    expect(await controller.saveReviewSelection()).toBe(true);
    expect(applyMusicReviewSelection).toHaveBeenCalledWith(expect.objectContaining({
      items: [{ itemId: "item-1", expectedVersion: 1 }, { itemId: "item-2", expectedVersion: 2 }],
      reviewState: "reviewed",
      addPlaylistIds: ["focus"],
    }));
    expect(library.currentWindow.items).toEqual([
      { id: "item-1", reviewState: "reviewed", updatedAtMs: 100, version: 2 },
      { id: "item-2", reviewState: "reviewed", updatedAtMs: 100, version: 3 },
    ]);
    expect(library.refreshAfterMutation).not.toHaveBeenCalled();
    expect(library.refreshSummariesAfterMutation).toHaveBeenCalledOnce();
  });

  it("ignores an unassigned selection while retaining rows for local visibility filtering", async () => {
    getMusicMembershipMatrix.mockResolvedValueOnce([]);
    const library = {
      currentWindow: {
        items: [
          { id: "item-1", reviewState: "unreviewed", updatedAtMs: 1, version: 1 },
          { id: "item-2", reviewState: "deferred", updatedAtMs: 1, version: 2 },
        ],
        totalCount: 4,
      },
      refreshAfterMutation: vi.fn(async () => true),
      refreshSummariesAfterMutation: vi.fn(async () => true),
    } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    await controller.open(["item-1", "item-2"], [summary("all"), summary("focus")]);

    expect(controller.hasExistingMemberships).toBe(false);
    expect(await controller.ignoreReviewSelection()).toBe(true);
    expect(applyMusicReviewSelection).toHaveBeenCalledWith({
      actionId: "action",
      items: [{ itemId: "item-1", expectedVersion: 1 }, { itemId: "item-2", expectedVersion: 2 }],
      reviewState: "ignored",
      addPlaylistIds: [],
      removePlaylistIds: [],
      updatedAtMs: 100,
    });
    expect(library.currentWindow.items).toEqual([
      { id: "item-1", reviewState: "ignored", updatedAtMs: 100, version: 2 },
      { id: "item-2", reviewState: "ignored", updatedAtMs: 100, version: 3 },
    ]);
    expect(library.currentWindow.totalCount).toBe(4);
    expect(library.refreshAfterMutation).not.toHaveBeenCalled();
    expect(library.refreshSummariesAfterMutation).toHaveBeenCalledOnce();
  });

  it("updates all selected rows together before persistence completes", async () => {
    getMusicMembershipMatrix.mockResolvedValueOnce([]);
    let resolveRequest!: (value: {
      membershipChangedCount: number;
      reviewChangedCount: number;
      items: Array<{ id: string; version: number }>;
    }) => void;
    applyMusicReviewSelection.mockImplementationOnce(() => new Promise((resolve) => {
      resolveRequest = resolve;
    }));
    const library = {
      currentWindow: {
        items: [
          { id: "item-1", reviewState: "unreviewed", updatedAtMs: 1, version: 1 },
          { id: "item-2", reviewState: "deferred", updatedAtMs: 1, version: 2 },
        ],
        totalCount: 2,
      },
      refreshAfterMutation: vi.fn(async () => true),
      refreshSummariesAfterMutation: vi.fn(async () => true),
    } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    await controller.open(["item-1", "item-2"], [summary("all")]);

    const pending = controller.ignoreReviewSelection();
    expect(library.currentWindow.items).toEqual([
      { id: "item-1", reviewState: "ignored", updatedAtMs: 100, version: 1 },
      { id: "item-2", reviewState: "ignored", updatedAtMs: 100, version: 2 },
    ]);
    resolveRequest({
      membershipChangedCount: 0,
      reviewChangedCount: 2,
      items: [{ id: "item-1", version: 2 }, { id: "item-2", version: 3 }],
    });
    expect(await pending).toBe(true);
    expect(library.currentWindow.totalCount).toBe(2);
  });

  it("returns an ignored selection to the unreviewed state", async () => {
    const library = {
      currentWindow: {
        items: [
          { id: "item-1", reviewState: "ignored", updatedAtMs: 1, version: 2 },
          { id: "item-2", reviewState: "ignored", updatedAtMs: 1, version: 3 },
        ],
      },
      refreshAfterMutation: vi.fn(async () => true),
    } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    controller.useSelection(["item-1", "item-2"]);

    expect(await controller.setReviewState("unreviewed")).toBe(true);
    expect(bulkSetMusicReviewState).toHaveBeenCalledWith({
      items: [{ itemId: "item-1", expectedVersion: 2 }, { itemId: "item-2", expectedVersion: 3 }],
      reviewState: "unreviewed",
      deferredUntil: null,
      updatedAtMs: 100,
    });
    expect(library.refreshAfterMutation).toHaveBeenCalledOnce();
  });

  it("ignores a selection without changing existing or pending playlist memberships", async () => {
    const library = {
      currentWindow: {
        items: [
          { id: "item-1", reviewState: "unreviewed", updatedAtMs: 1, version: 1 },
          { id: "item-2", reviewState: "unreviewed", updatedAtMs: 1, version: 1 },
        ],
        totalCount: 2,
      },
      refreshSummariesAfterMutation: vi.fn(async () => true),
    } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    await controller.open(["item-1", "item-2"], [summary("all"), summary("focus")]);
    controller.toggle("focus");
    const callCount = applyMusicReviewSelection.mock.calls.length;

    expect(await controller.ignoreReviewSelection()).toBe(true);
    expect(applyMusicReviewSelection).toHaveBeenCalledTimes(callCount + 1);
    expect(applyMusicReviewSelection).toHaveBeenLastCalledWith(expect.objectContaining({
      reviewState: "ignored",
      addPlaylistIds: [],
      removePlaylistIds: [],
    }));
  });

  it("keeps explicit playlist choices while the inline tree selection changes", async () => {
    const library = { refreshAfterMutation: vi.fn(async () => true) } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    await controller.open(["item-1"], [summary("all"), summary("empty")]);
    controller.toggle("empty");
    await controller.open(["item-1", "item-2"], [summary("all"), summary("empty")], true);

    expect(controller.states.empty).toBe("checked");
    expect(controller.touchedPlaylistIds).toEqual(new Set(["empty"]));
  });

  it("keeps the current playlist projection visible while an expanded selection loads", async () => {
    const library = { refreshAfterMutation: vi.fn(async () => true) } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    await controller.open(["item-1"], [summary("all"), summary("empty")]);
    controller.toggle("empty");
    let resolveMatrix!: (value: Array<{ itemId: string; playlistId: string; weight: "normal" }>) => void;
    getMusicMembershipMatrix.mockImplementationOnce(() => new Promise((resolve) => { resolveMatrix = resolve; }));

    const loading = controller.open(["item-1", "item-2"], [summary("all"), summary("empty")], true);

    expect(controller.loading).toBe(true);
    expect(controller.states.empty).toBe("checked");
    resolveMatrix([{ itemId: "item-1", playlistId: "all", weight: "normal" }]);
    expect(await loading).toBe(true);
    expect(controller.states.empty).toBe("checked");
  });

  it("keeps a membership save that lands after a new selection opened off the new selection", async () => {
    const library = { refreshAfterMutation: vi.fn(async () => true) } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    getMusicMembershipMatrix.mockImplementationOnce(async () => []);
    await controller.open(["item-1"], [summary("all"), summary("empty")]);
    controller.toggle("empty");
    let rejectSave!: (error: Error) => void;
    bulkEditMusicMemberships.mockImplementationOnce(() => new Promise((_, reject) => { rejectSave = reject; }));
    let resolveSave!: (value: { changedCount: number }) => void;
    bulkEditMusicMemberships.mockImplementationOnce(() => new Promise((resolve) => { resolveSave = resolve; }));

    const failing = controller.saveMemberships();
    getMusicMembershipMatrix.mockImplementationOnce(async () => [{ itemId: "item-2", playlistId: "all", weight: "normal" }]);
    await controller.open(["item-2", "item-3"], [summary("all"), summary("empty")]);
    rejectSave(new Error("late failure"));
    expect(await failing).toBe(false);
    expect(controller.error).toBeNull();

    controller.toggle("all");
    const saving = controller.saveMemberships();
    getMusicMembershipMatrix.mockImplementationOnce(async () => []);
    await controller.open(["item-4"], [summary("all"), summary("empty")]);
    resolveSave({ changedCount: 1 });
    expect(await saving).toBe(true);
    expect(controller.initialCounts).toEqual({});
    expect(controller.states).toEqual({ all: "unchecked", empty: "unchecked" });
  });

  it("keeps a playlist created during a selection change checked once the new selection loads", async () => {
    const library = { refreshSummariesAfterMutation: vi.fn(async () => true) } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "created");
    await controller.open(["item-1"], [summary("all")]);
    let resolveMatrix!: (value: Array<{ itemId: string; playlistId: string; weight: "normal" }>) => void;
    getMusicMembershipMatrix.mockImplementationOnce(() => new Promise((resolve) => { resolveMatrix = resolve; }));

    const loading = controller.open(["item-1", "item-2"], [summary("all")], true);
    expect(await controller.createPlaylistAndSelect("Late", "music")).toBe("created");
    resolveMatrix([]);
    expect(await loading).toBe(true);

    expect(controller.states.created).toBe("checked");
    expect(controller.membershipsChanged).toBe(true);
  });

  it("creates a playlist without checking it in an edit that started over meanwhile", async () => {
    let finishRefresh!: (value: boolean) => void;
    const library = {
      refreshSummariesAfterMutation: vi.fn(() => new Promise<boolean>((resolve) => { finishRefresh = resolve; })),
    } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "created");
    await controller.open(["item-1"], [summary("all")]);

    const creating = controller.createPlaylistAndSelect("Late", "music");
    await vi.waitFor(() => expect(library.refreshSummariesAfterMutation).toHaveBeenCalledOnce());
    await controller.open(["item-9"], [summary("all")]);
    finishRefresh(true);

    expect(await creating).toBe("created");
    expect(controller.states.created).toBeUndefined();
    expect(controller.touchedPlaylistIds.has("created")).toBe(false);
  });

  it("describes and restores the tracks selected when describing started after the selection changes", async () => {
    const runOptimistic = vi.fn(async (action: { persist: () => Promise<unknown>; undo: () => Promise<void> }) => {
      await action.persist();
      return action;
    });
    const library = { runOptimistic, refreshAfterMutation: vi.fn(async () => true) } as unknown as MusicLibraryController;
    const controller = new MusicBulkEditController(library, () => 100, () => "action");
    controller.useSelection(["item-1", "item-2"]);
    let releasePrior!: () => void;
    const priorRead = new Promise<void>((resolve) => { releasePrior = resolve; });
    getMusicInspectorDetail.mockImplementation(async (itemId: string) => {
      await priorRead;
      return { item: { id: itemId }, signals: ["calm"] as MusicItemSignal[] };
    });

    const describing = controller.setSignals(["lyrics"]);
    controller.useSelection(["item-3"]);
    releasePrior();
    expect(await describing).toBe(true);

    expect(setMusicItemSignals).toHaveBeenCalledWith({ itemIds: ["item-1", "item-2"], signals: ["lyrics"], updatedAtMs: 100 });
    const action = runOptimistic.mock.calls[0]?.[0];
    expect(action).toMatchObject({ key: "bulk-signals:item-1:item-2" });
    setMusicItemSignals.mockClear();
    await action?.undo();
    expect(setMusicItemSignals.mock.calls.map(([write]) => write)).toEqual([
      { itemIds: ["item-1"], signals: ["calm"], updatedAtMs: 100 },
      { itemIds: ["item-2"], signals: ["calm"], updatedAtMs: 100 },
    ]);
  });
});
