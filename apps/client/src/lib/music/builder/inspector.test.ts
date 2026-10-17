import { describe, expect, it } from "vitest";
import type { MusicInspectorDetail } from "$lib/music/library/contracts";
import { MusicBuilderInspectorController } from "./inspector.svelte";

function detail(id: string): MusicInspectorDetail {
  return {
    item: {
      id, identityKey: `local:${id}`, sourceKind: "local-file", mediaKind: "audio",
      youtubeVideoId: null, originalTitle: id, originalArtist: "", originalAlbum: "",
      originalTrackNumber: null, originalArtworkIdentity: null, youtubeResolutionState: null,
      titleOverride: null, artistOverride: null, albumOverride: null, artworkOverride: null,
      durationMs: null, availability: "available", reviewState: "unreviewed",
      reviewChangedAtMs: null, reviewDeferredUntilMs: null, discoveredAtMs: 1, updatedAtMs: 1, version: 1,
    },
    locations: [], memberships: [], membershipSkipRanges: [], snoozes: [], signals: [], statistics: null,
    sourceCollectionIds: [],
  };
}

describe("music builder inspector controller", () => {
  it("rejects stale details after selection changes", async () => {
    let resolveOld!: (detail: MusicInspectorDetail) => void;
    const old = new Promise<MusicInspectorDetail>((resolve) => { resolveOld = resolve; });
    const controller = new MusicBuilderInspectorController({
      detail: (id) => id === "old" ? old : Promise.resolve(detail(id)),
      setSignals: async () => 2,
    });
    const oldSelection = controller.select("old");
    expect(await controller.select("new")).toBe(true);
    resolveOld(detail("old"));
    expect(await oldSelection).toBe(false);
    expect(controller.detail?.item.id).toBe("new");
  });

  it("reuses a prefetched detail for the next selection", async () => {
    const requestedIds: string[] = [];
    const controller = new MusicBuilderInspectorController({
      detail: async (id) => {
        requestedIds.push(id);
        return detail(id);
      },
      setSignals: async () => 2,
    });

    await controller.prefetch(["next"]);
    expect(await controller.select("next")).toBe(true);

    expect(controller.detail?.item.id).toBe("next");
    expect(controller.busy).toBe(false);
    expect(requestedIds).toEqual(["next"]);
  });

  it("switches synchronously to a prefetched detail", async () => {
    const controller = new MusicBuilderInspectorController({
      detail: async (id) => detail(id),
      setSignals: async () => 2,
    });
    await controller.prefetch(["next"]);

    expect(controller.selectCached("next")).toBe(true);
    expect(controller.itemId).toBe("next");
    expect(controller.detail?.item.id).toBe("next");
    expect(controller.busy).toBe(false);
  });

  it("shares an in-flight detail request between prefetch and selection", async () => {
    let resolveDetail!: (value: MusicInspectorDetail) => void;
    const pendingDetail = new Promise<MusicInspectorDetail>((resolve) => { resolveDetail = resolve; });
    let requestCount = 0;
    const controller = new MusicBuilderInspectorController({
      detail: () => {
        requestCount += 1;
        return pendingDetail;
      },
      setSignals: async () => 2,
    });

    const prefetch = controller.prefetch(["next"]);
    const selection = controller.select("next");
    resolveDetail(detail("next"));

    await prefetch;
    expect(await selection).toBe(true);
    expect(requestCount).toBe(1);
  });

  it("saves canonical signals optimistically and restores them on failure", async () => {
    let shouldFail = false;
    const controller = new MusicBuilderInspectorController({
      detail: async (id) => detail(id),
      setSignals: async () => {
        if (shouldFail) throw new Error("save failed");
        return 2;
      },
    });
    await controller.select("item-1");
    expect(await controller.saveSignals(["lyrics", "calm"])).toBe(true);
    expect(controller.detail?.signals).toEqual(["lyrics", "calm"]);
    expect(controller.detail?.item.version).toBe(2);

    expect(await controller.undoSignals()).toBe(true);
    expect(controller.detail?.signals).toEqual([]);

    shouldFail = true;
    expect(await controller.saveSignals(["energizing"])).toBe(false);
    expect(controller.detail?.signals).toEqual([]);
  });

  it("preserves section expansion while inspector presentation changes", () => {
    const controller = new MusicBuilderInspectorController();
    controller.toggleSection("provenance");
    expect(controller.expandedSections.has("provenance")).toBe(true);
    controller.toggleSection("provenance");
    expect(controller.expandedSections.has("provenance")).toBe(false);
  });
});
