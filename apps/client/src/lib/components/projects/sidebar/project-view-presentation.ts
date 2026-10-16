import Book from "@lucide/svelte/icons/book";
import CalendarDays from "@lucide/svelte/icons/calendar-days";
import ChartGantt from "@lucide/svelte/icons/chart-gantt";
import Columns3 from "@lucide/svelte/icons/columns-3";
import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
import List from "@lucide/svelte/icons/list";
import MessageSquare from "@lucide/svelte/icons/message-square";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { ProjectView, ProjectViewType } from "$lib/projects/types";

/** The fixed icon of each view type; users rename views but never change their icon. */
export const PROJECT_VIEW_TYPE_ICONS = {
  dashboard: LayoutDashboard,
  list: List,
  kanban: Columns3,
  calendar: CalendarDays,
  gantt: ChartGantt,
  notes: Book,
  chat: MessageSquare,
} as const satisfies Record<ProjectViewType, typeof List>;

/** The localized name of a view type, used for unnamed views and the add view menu. */
export function projectViewTypeLabel(t: Translate, viewType: ProjectViewType): string {
  switch (viewType) {
    case "dashboard": return t("projects.sidebar.overview");
    case "list": return t("projects.tabs.list");
    case "kanban": return t("projects.tabs.kanban");
    case "calendar": return t("projects.tabs.calendar");
    case "gantt": return t("projects.tabs.gantt");
    case "notes": return t("projects.sidebar.notes");
    case "chat": return t("projects.sidebar.chat");
  }
}

/** The name a view shows: the user's name when set, else its type label. */
export function projectViewLabel(t: Translate, view: Pick<ProjectView, "name" | "viewType">): string {
  return view.name ?? projectViewTypeLabel(t, view.viewType);
}
