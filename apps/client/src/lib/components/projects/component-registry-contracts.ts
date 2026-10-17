export type ProjectOptionalComponentKind =
  | "toolbar"
  | "bulk-actions"
  | "task-finder";

export type LoadedProjectOptionalComponent =
  | { kind: "toolbar"; component: typeof import("./ProjectToolbarPanels.svelte").default }
  | { kind: "bulk-actions"; component: typeof import("./ProjectBulkActionController.svelte").default }
  | { kind: "task-finder"; component: typeof import("$lib/components/projects/pickers/ProjectTaskFinder.svelte").default };
