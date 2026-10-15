import { projectViewLabel } from "$lib/components/projects/sidebar/project-view-presentation";
import type { Translate } from "$lib/i18n/translator.svelte";
import { notesPageTitle } from "$lib/notes/pages/title";
import type { WorkspaceLocationAdapter } from "$lib/stores/workspace-tabs.svelte";
import type { getChat } from "$lib/stores/chat.svelte";
import type { getNotes } from "$lib/stores/notes.svelte";
import type { getProjects } from "$lib/stores/projects.svelte";
import { createWorkspaceLocationAdapter } from "./location-adapter";

/** The app's view stores that workspace tab locations describe. */
export interface AppWorkspaceLocationStores {
  projects: ReturnType<typeof getProjects>;
  notes: ReturnType<typeof getNotes>;
  chat: ReturnType<typeof getChat>;
}

/**
 * Bind workspace tab locations to the app's Projects, Notes, and Chat stores. Notes locations follow the
 * main editor pane, the one a tab restores into.
 */
export function createAppWorkspaceLocationAdapter(
  { projects, notes, chat }: AppWorkspaceLocationStores,
  t: Translate,
): WorkspaceLocationAdapter {
  const notesMainPaneStore = () => notes.editorPanes.find((pane) => pane.id === notes.mainPaneId)?.store ?? null;
  return createWorkspaceLocationAdapter({
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
      channelName: (projectId, channelId) => (
        chat.channelsForProject(projectId).find((channel) => channel.id === channelId)?.name ?? null
      ),
    },
  });
}
