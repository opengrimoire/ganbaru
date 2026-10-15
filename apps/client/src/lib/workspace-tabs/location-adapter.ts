import type { View } from "$lib/navigation";
import type { WorkspaceLocationAdapter } from "$lib/stores/workspace-tabs.svelte";
import { kindLocation, type WorkspaceTabItem, type WorkspaceTabLocation } from "./model";

/** The Projects store surface a tab location reads and restores. */
export interface WorkspaceLocationProjects<TView extends { id: string }> {
  readonly selectedProjectId: string | null;
  readonly activeViewId: string | null;
  ensureLoaded(): Promise<void>;
  hasProject(projectId: string): boolean;
  projectName(projectId: string): string | null;
  /** Display name of a project view, or null when it is not loaded. */
  viewName(projectId: string, viewId: string): string | null;
  viewsForProject(projectId: string): readonly TView[];
  selectProject(projectId: string): Promise<void>;
  selectView(view: TView): void;
}

/** The Notes main editor surface a tab location reads and restores. */
export interface WorkspaceLocationNotes {
  readonly selectedPageId: string | null;
  selectPage(pageId: string): Promise<void>;
  /** Title of a loaded page, or null when it is not loaded. */
  pageTitle(pageId: string): string | null;
}

/** The Chat store surface a tab location reads and restores. */
export interface WorkspaceLocationChat {
  readonly selectedChannelId: string | null;
  selectChannel(channelId: string): Promise<void>;
  /** Name of a loaded channel, or null when it is not loaded. */
  channelName(projectId: string, channelId: string): string | null;
}

export interface WorkspaceLocationStores<TView extends { id: string }> {
  projects: WorkspaceLocationProjects<TView>;
  notes: WorkspaceLocationNotes;
  chat: WorkspaceLocationChat;
}

function readItem<TView extends { id: string }>(
  kind: View,
  stores: WorkspaceLocationStores<TView>,
): WorkspaceTabItem | null {
  if (kind === "projects") {
    const id = stores.projects.activeViewId;
    return id ? { type: "project-view", id } : null;
  }
  if (kind === "notes") {
    const id = stores.notes.selectedPageId;
    return id ? { type: "notes-page", id } : null;
  }
  if (kind === "chat") {
    const id = stores.chat.selectedChannelId;
    return id ? { type: "chat-channel", id } : null;
  }
  return null;
}

function itemTitle<TView extends { id: string }>(
  location: WorkspaceTabLocation,
  stores: WorkspaceLocationStores<TView>,
): string | null {
  const { item, projectId } = location;
  if (!item) return null;
  if (item.type === "notes-page") return stores.notes.pageTitle(item.id);
  if (!projectId) return null;
  if (item.type === "project-view") return stores.projects.viewName(projectId, item.id);
  if (item.type === "chat-channel") return stores.chat.channelName(projectId, item.id);
  return null;
}

async function applyItem<TView extends { id: string }>(
  location: WorkspaceTabLocation,
  stores: WorkspaceLocationStores<TView>,
): Promise<void> {
  const item = location.item;
  if (!item) return;
  if (item.type === "project-view") {
    if (!location.projectId) return;
    const view = stores.projects.viewsForProject(location.projectId).find((candidate) => candidate.id === item.id);
    if (view) stores.projects.selectView(view);
  } else if (item.type === "notes-page") {
    if (stores.notes.selectedPageId !== item.id) await stores.notes.selectPage(item.id);
  } else if (item.type === "chat-channel") {
    if (stores.chat.selectedChannelId !== item.id) await stores.chat.selectChannel(item.id);
  }
}

/**
 * Map tab locations onto the shared view stores. Reading tracks the stores' reactive selection, so the tab
 * store records where a tab is as the user navigates. Restoring skips projects and items that no longer
 * exist, leaving the stores where they are.
 */
export function createWorkspaceLocationAdapter<TView extends { id: string }>(
  stores: WorkspaceLocationStores<TView>,
): WorkspaceLocationAdapter {
  return {
    read(kind) {
      if (kind === "calendar") return kindLocation(kind);
      return { kind, projectId: stores.projects.selectedProjectId, item: readItem(kind, stores) };
    },
    title(level, location) {
      if (level === "item") return itemTitle(location, stores);
      if (level === "kind" || !location.projectId) return null;
      return stores.projects.projectName(location.projectId);
    },
    async apply(location, isCurrent) {
      if (location.kind === "calendar") return;
      const projectId = location.projectId;
      if (projectId && stores.projects.selectedProjectId !== projectId) {
        await stores.projects.ensureLoaded();
        if (!isCurrent() || !stores.projects.hasProject(projectId)) return;
        await stores.projects.selectProject(projectId);
        if (!isCurrent() || stores.projects.selectedProjectId !== projectId) return;
      }
      await applyItem(location, stores);
    },
  };
}
