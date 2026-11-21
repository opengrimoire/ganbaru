import Book from "@lucide/svelte/icons/book";
import Calendar from "@lucide/svelte/icons/calendar";
import BriefcaseBusiness from "@lucide/svelte/icons/briefcase-business";
import MessageSquare from "@lucide/svelte/icons/message-square";
import MessagesSquare from "@lucide/svelte/icons/messages-square";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { View } from "$lib/navigation";
import type { WorkspaceTab } from "./model";

export type WorkspaceTabIcon = typeof Calendar;

const KIND_ICONS: Readonly<Record<View, WorkspaceTabIcon>> = {
  calendar: Calendar,
  projects: BriefcaseBusiness,
  notes: Book,
  chat: MessageSquare,
};

/** Icon shown for a tab kind in the strip, the launcher, and the mobile bar. */
export function workspaceTabKindIcon(kind: View): WorkspaceTabIcon {
  return KIND_ICONS[kind];
}

/** Icon shown for one tab: the reply thread icon for a reply thread tab, else the icon of its kind. */
export function workspaceTabIcon(tab: Pick<WorkspaceTab, "location">): WorkspaceTabIcon {
  return tab.location.item?.type === "chat-reply-thread" ? MessagesSquare : workspaceTabKindIcon(tab.location.kind);
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
 * Label a tab shows beside its icon: its project name for any tab inside a project, item tabs included, then its
 * item name, then its kind. The icon tells the kinds apart, and the full label names the item.
 */
export function workspaceTabLabel(tab: Pick<WorkspaceTab, "location" | "title" | "projectTitle">, t: Translate): string {
  return workspaceTabProject(tab) ?? workspaceTabPlaceLabel(tab, t);
}

/** Project name a tab shows beside its label, or null for Calendar and tabs without a known project. */
export function workspaceTabProject(tab: Pick<WorkspaceTab, "location" | "projectTitle">): string | null {
  return tab.location.kind === "calendar" || !tab.location.projectId ? null : tab.projectTitle;
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
