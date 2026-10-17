import { describe, expect, it } from "vitest";
import {
  createMusicBuilderContextViewState,
  createMusicReviewTreeViewState,
  createMusicReviewWorkspaceViewState,
} from "./view-state";

describe("music builder view state", () => {
  it("creates isolated session state for Review and contextual panels", () => {
    const firstTree = createMusicReviewTreeViewState();
    const secondTree = createMusicReviewTreeViewState();
    firstTree.selectedItemIds.push("track-1");
    expect(secondTree.selectedItemIds).toEqual([]);

    expect(createMusicReviewWorkspaceViewState()).toMatchObject({
      inlineCreateOpen: false,
      managingPlaylists: false,
      newPlaylistIcon: "lucide:list-music",
    });
    expect(createMusicBuilderContextViewState()).toMatchObject({
      contextPanelOpen: false,
      selectedSourceId: null,
      reviewPanel: "folders",
      reviewIssueGroup: null,
      soundscapeFilter: "all",
    });
  });
});
