export interface MusicReviewTreeViewState {
  search: string;
  collapsedFolderIds: string[];
  selectedItemIds: string[];
  selectedFolderIds: string[];
  scrollTop: number;
}

export interface MusicReviewWorkspaceViewState {
  inlineCreateOpen: boolean;
  newPlaylistName: string;
  newPlaylistIcon: string;
  managingPlaylists: boolean;
  membershipBaselines: Record<string, string>;
  sessionSkippedIds: string[];
}

export type SoundscapeFilter = "all" | "generated" | "local" | `group:${string}`;

export interface MusicBuilderContextViewState {
  contextPanelOpen: boolean;
  selectedSourceId: string | null;
  reviewPanel: "folders" | "issues";
  reviewIssueGroup: import("$lib/music/library/issue-presentation").MusicIssueGroup | null;
  soundscapeFilter: SoundscapeFilter;
}

export function createMusicReviewTreeViewState(): MusicReviewTreeViewState {
  return { search: "", collapsedFolderIds: [], selectedItemIds: [], selectedFolderIds: [], scrollTop: 0 };
}

export function createMusicReviewWorkspaceViewState(): MusicReviewWorkspaceViewState {
  return {
    inlineCreateOpen: false,
    newPlaylistName: "",
    newPlaylistIcon: "lucide:list-music",
    managingPlaylists: false,
    membershipBaselines: {},
    sessionSkippedIds: [],
  };
}

export function createMusicBuilderContextViewState(): MusicBuilderContextViewState {
  return {
    contextPanelOpen: false,
    selectedSourceId: null,
    reviewPanel: "folders",
    reviewIssueGroup: null,
    soundscapeFilter: "all",
  };
}
