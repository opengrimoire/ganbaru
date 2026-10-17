import Book from "@lucide/svelte/icons/book";
import Calendar from "@lucide/svelte/icons/calendar";
import Folder from "@lucide/svelte/icons/folder";
import MessageSquare from "@lucide/svelte/icons/message-square";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { View } from "$lib/navigation";
import type { WorkspaceTab } from "./model";

export type WorkspaceTabIcon = typeof Calendar;

const KIND_ICONS: Readonly<Record<View, WorkspaceTabIcon>> = {
  calendar: Calendar,
  projects: Folder,
  notes: Book,
  chat: MessageSquare,
};

/** Icon shown for a tab kind in the strip, the launcher, and the mobile bar. */
export function workspaceTabKindIcon(kind: View): WorkspaceTabIcon {
  return KIND_ICONS[kind];
}

/** Localized name of a tab kind. */
export function workspaceTabKindLabel(kind: View, t: Translate): string {
  if (kind === "calendar") return t("titleBar.tab.calendar");
  if (kind === "projects") return t("titleBar.tab.projects");
  if (kind === "notes") return t("titleBar.tab.notes");
  return t("titleBar.tab.chat");
}

/** Name of the place a tab shows: its cached item name when it has one, else its kind. */
function workspaceTabPlaceLabel(tab: Pick<WorkspaceTab, "location" | "title">, t: Translate): string {
  return tab.title ?? workspaceTabKindLabel(tab.location.kind, t);
}

/**
 * Label a tab shows beside its kind icon: its item name for an item tab, its project name for a kind or project
 * tab inside a project, and its kind otherwise. The icon tells the kinds apart, so a project tab skips the kind name.
 */
export function workspaceTabLabel(tab: Pick<WorkspaceTab, "location" | "title" | "projectTitle">, t: Translate): string {
  return tab.title ?? workspaceTabProject(tab) ?? workspaceTabKindLabel(tab.location.kind, t);
}

/** Project name a tab shows beside its label, or null for Calendar and tabs without a known project. */
export function workspaceTabProject(tab: Pick<WorkspaceTab, "location" | "projectTitle">): string | null {
  return tab.location.kind === "calendar" || !tab.location.projectId ? null : tab.projectTitle;
}

/** Project name an item tab adds after its item name in the strip, or null when the label already is the project. */
export function workspaceTabProjectHint(tab: Pick<WorkspaceTab, "location" | "title" | "projectTitle">): string | null {
  return tab.title === null ? null : workspaceTabProject(tab);
}

/** Longest project name a tab strip shows before shortening it, in characters. */
export const WORKSPACE_TAB_PROJECT_MAX_CHARS = 18;

/** Shorten a project name for the tab strip, keeping whole characters and marking the cut with an ellipsis. */
export function shortWorkspaceTabProject(name: string, maxChars = WORKSPACE_TAB_PROJECT_MAX_CHARS): string {
  const characters = Array.from(name.trim());
  if (characters.length <= maxChars) return characters.join("");
  return `${characters.slice(0, Math.max(1, maxChars - 1)).join("").trimEnd()}…`;
}

/** Full accessible name of a tab: its item or kind name followed by its project in parentheses when it has one. */
export function workspaceTabFullLabel(
  tab: Pick<WorkspaceTab, "location" | "title" | "projectTitle">,
  t: Translate,
): string {
  const label = workspaceTabPlaceLabel(tab, t);
  const project = workspaceTabProject(tab);
  return project ? t("titleBar.tab.withProject", label, project) : label;
}

/** Shortcut hint for a tab position, matching Alt+1 to Alt+8 and Alt+9 for the last tab. */
export function workspaceTabPositionShortcut(index: number, count: number): string | null {
  if (index < 8) return `Alt+${index + 1}`;
  return index === count - 1 ? "Alt+9" : null;
}
