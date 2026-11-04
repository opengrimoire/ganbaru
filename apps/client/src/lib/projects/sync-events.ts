const PROJECTS_SYNC_CHANGED_EVENT = "ganbaru-projects-sync-changed";

/** Replicated Projects tables, as sync names them in applied notices, conflicts, and recovery offers. */
export const PROJECTS_SYNC_TABLES: ReadonlySet<string> = new Set([
  "project_groups",
  "projects",
  "project_working_folders",
  "project_primary_working_folders",
  "project_sections",
  "project_statuses",
  "project_priorities",
  "project_tags",
  "project_tasks",
  "project_checklist_items",
  "project_task_dependencies",
  "project_task_tag_links",
  "project_custom_fields",
  "project_custom_field_options",
  "project_custom_field_values",
  "project_task_change_events",
  "project_views",
  "project_view_preferences",
  "project_custom_emojis",
]);

/** Tells this window's Projects store that sync recovery changed rows outside it. */
export function notifyProjectsSyncChanged(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(PROJECTS_SYNC_CHANGED_EVENT));
}

/** Listens for {@link notifyProjectsSyncChanged}, and returns a function that stops listening. */
export function onProjectsSyncChanged(listener: () => void): () => void {
  if (typeof window === "undefined") return () => undefined;
  window.addEventListener(PROJECTS_SYNC_CHANGED_EVENT, listener);
  return () => window.removeEventListener(PROJECTS_SYNC_CHANGED_EVENT, listener);
}
