import { projectViewLabel } from "$lib/components/projects/sidebar/project-view-presentation";
import type { Translate } from "$lib/i18n/translator.svelte";
import { notesPageTitle } from "$lib/notes/pages/title";
import { isProjectHostedViewType } from "$lib/projects/views";
import type { WorkspaceLocationAdapter } from "$lib/stores/workspace-tabs.svelte";
import type { ChatStore } from "$lib/stores/chat.svelte";
import type { NotesWorkspace } from "$lib/stores/notes.svelte";
import type { ProjectsStore } from "$lib/stores/projects.svelte";
import { createWorkspaceLocationAdapter, type WorkspaceLocationStores } from "./location-adapter";

/** The view stores of one pane scope that workspace tab locations describe. */
export interface AppPaneLocationStores {
  projects: ProjectsStore;
  notes: NotesWorkspace;
  chat: ChatStore;
}

/** The app's view stores that workspace tab locations describe. */
export interface AppWorkspaceLocationStores extends AppPaneLocationStores {
  /** The stores of the secondary pane scope, or none where split views are unavailable. */
  secondary?: AppPaneLocationStores;
}

type AppProjectView = ReturnType<ProjectsStore["viewsForProject"]>[number];

interface PaneLocationStores extends AppPaneLocationStores {
  /** Name of a channel any pane's Chat store has loaded. */
  channelName: (projectId: string, channelId: string) => string | null;
}

function locationStores(
  { projects, notes, chat, channelName }: PaneLocationStores,
  t: Translate,
): WorkspaceLocationStores<AppProjectView> {
  const notesMainPaneStore = () => notes.editorPanes.find((pane) => pane.id === notes.mainPaneId)?.store ?? null;
  return {
    projects: {
      get selectedProjectId() { return projects.selectedProjectId; },
      get activeViewId() { return projects.activeViewId; },
      ensureLoaded: () => projects.ensureLoaded(),
      hasProject: (projectId) => Boolean(projects.projectById(projectId)),
      projectName: (projectId) => projects.projectById(projectId)?.name ?? null,
      viewName: (projectId, viewId) => {
        const view = projects.viewsForProject(projectId).find((candidate) => candidate.id === viewId);
        return view ? projectViewLabel(t, view) : null;
      },
      viewsForProject: (projectId) => projects.viewsForProject(projectId),
      hostedView: (projectId, viewId) => {
        const view = projects.viewsForProject(projectId).find((candidate) => candidate.id === viewId);
        return view && isProjectHostedViewType(view.viewType) ? view.viewType : null;
      },
      selectProject: (projectId) => projects.selectProject(projectId),
      selectView: (view) => projects.selectView(view),
    },
    notes: {
      get selectedPageId() {
        return notesMainPaneStore()?.selectedPageId ?? null;
      },
      selectPage: (pageId) => notes.selectPage(pageId, { openMode: "full" }),
      pageTitle: (pageId) => {
        const page = notesMainPaneStore()?.navigationPages.find((candidate) => candidate.id === pageId);
        return page ? notesPageTitle(page, t("notes.untitled")) : null;
      },
    },
    chat: {
      get selectedChannelId() { return chat.selectedChannelId; },
      selectChannel: (channelId) => chat.selectChannel(channelId),
      channelName,
    },
  };
}

/**
 * Bind workspace tab locations to the app's Projects, Notes, and Chat stores. Locations follow the stores of the
 * pane scope a tab shows through, the ones a tab restores into; for Notes, its main editor pane.
 */
export function createAppWorkspaceLocationAdapter(
  { projects, notes, chat, secondary: secondaryStores }: AppWorkspaceLocationStores,
  t: Translate,
): WorkspaceLocationAdapter {
  const chats = secondaryStores ? [chat, secondaryStores.chat] : [chat];
  const channelName = (projectId: string, channelId: string) => {
    for (const store of chats) {
      const channel = store.channelsForProject(projectId).find((candidate) => candidate.id === channelId);
      if (channel) return channel.name;
    }
    return null;
  };
  const primary = locationStores({ projects, notes, chat, channelName }, t);
  const secondary = secondaryStores ? locationStores({ ...secondaryStores, channelName }, t) : primary;
  return createWorkspaceLocationAdapter(primary, secondary);
}
