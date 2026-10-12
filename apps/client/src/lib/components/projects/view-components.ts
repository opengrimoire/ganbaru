/** Constructors for the Project views prepared by a platform shell. */
export interface ProjectViewComponents {
  list: typeof import("$lib/components/projects/list/ProjectListView.svelte").default;
  kanban: typeof import("$lib/components/projects/views/ProjectKanbanView.svelte").default;
  calendar: typeof import("$lib/components/calendar/CalendarView.svelte").default;
  gantt: typeof import("$lib/components/projects/views/ProjectGanttView.svelte").default;
  dashboard: typeof import("$lib/components/projects/views/ProjectDashboardView.svelte").default;
}
