import { projectViewLabel } from "$lib/components/projects/sidebar/project-view-presentation";
import type { Translate } from "$lib/i18n/translator.svelte";
import { notesPageTitle } from "$lib/notes/pages/title";
import { isProjectHostedViewType } from "$lib/projects/views";
import type { WorkspaceLocationAdapter } from "$lib/stores/workspace-tabs.svelte";
import type { getChat } from "$lib/stores/chat.svelte";
import type { NotesWorkspace } from "$lib/stores/notes.svelte";
import type { getProjects } from "$lib/stores/projects.svelte";
import { createWorkspaceLocationAdapter, type WorkspaceLocationStores } from "./location-adapter";

/** The app's view stores that workspace tab locations describe. */
export interface AppWorkspaceLocationStores {
  projects: ReturnType<typeof getProjects>;
  notes: NotesWorkspace;
  chat: ReturnType<typeof getChat>;
  /** The Notes workspace of the secondary pane scope, or none where split views are unavailable. */
  secondaryNotes?: NotesWorkspace;
}

type AppProjectView = ReturnType<ReturnType<typeof getProjects>["viewsForProject"]>[number];

function locationStores(
  { projects, notes, chat }: Omit<AppWorkspaceLocationStores, "secondaryNotes">,
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
      channelName: (projectId, channelId) => (
        chat.channelsForProject(projectId).find((channel) => channel.id === channelId)?.name ?? null
      ),
    },
  };
}

/**
 * Bind workspace tab locations to the app's Projects, Notes, and Chat stores. Notes locations follow the
 * main editor pane of the workspace a tab shows through, the one a tab restores into.
 */
export function createAppWorkspaceLocationAdapter(
  { secondaryNotes, ...stores }: AppWorkspaceLocationStores,
  t: Translate,
): WorkspaceLocationAdapter {
  const primary = locationStores(stores, t);
  const secondary = secondaryNotes ? locationStores({ ...stores, notes: secondaryNotes }, t) : primary;
  return createWorkspaceLocationAdapter(primary, secondary);
}
