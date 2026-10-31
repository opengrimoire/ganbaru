import type { SyncDomainPresentation, SyncTablePresentation } from "$lib/sync/presentation-registry";
import { notifyProjectsSyncChanged, PROJECTS_SYNC_TABLES } from "./sync-events";
import { publishProjectsSyncChange } from "./window-sync";

/** A name edited on two devices is chosen in the project's settings. */
const PROJECTS_PRESENTATION: SyncTablePresentation = {
  conflicts: (t, count) => t("projects.sync.projectConflicts", count),
  recovery: {
    untitled: (t) => t("projects.sync.projectUntitled"),
    note: (t) => t("projects.sync.projectRecoveryNote"),
    restored: (t) => t("projects.sync.projectRestored"),
    restoreFailed: (t) => t("projects.sync.projectRestoreFailed"),
  },
};

/** Titles and descriptions edited on two devices are chosen in the task details. */
const TASKS_PRESENTATION: SyncTablePresentation = {
  conflicts: (t, count) => t("projects.sync.taskConflicts", count),
  recovery: {
    untitled: (t) => t("projects.sync.taskUntitled"),
    note: (t) => t("projects.sync.taskRecoveryNote"),
    restored: (t) => t("projects.sync.taskRestored"),
    restoreFailed: (t) => t("projects.sync.taskRestoreFailed"),
  },
};

const CHECKLIST_PRESENTATION: SyncTablePresentation = {
  conflicts: (t, count) => t("projects.sync.checklistConflicts", count),
};

const DEPENDENCIES_PRESENTATION: SyncTablePresentation = {
  conflicts: (t, count) => t("projects.sync.dependencyConflicts", count),
};

const GROUPS_PRESENTATION: SyncTablePresentation = {
  recovery: {
    untitled: (t) => t("projects.sync.groupUntitled"),
    note: (t) => t("projects.sync.groupRecoveryNote"),
    restored: (t) => t("projects.sync.groupRestored"),
    restoreFailed: (t) => t("projects.sync.groupRestoreFailed"),
  },
};

const SECTIONS_PRESENTATION: SyncTablePresentation = {
  recovery: {
    untitled: (t) => t("projects.sync.sectionUntitled"),
    note: (t) => t("projects.sync.sectionRecoveryNote"),
    restored: (t) => t("projects.sync.sectionRestored"),
    restoreFailed: (t) => t("projects.sync.sectionRestoreFailed"),
  },
};

const STATUSES_PRESENTATION: SyncTablePresentation = {
  recovery: {
    untitled: (t) => t("projects.sync.statusUntitled"),
    note: (t) => t("projects.sync.statusRecoveryNote"),
    restored: (t) => t("projects.sync.statusRestored"),
    restoreFailed: (t) => t("projects.sync.statusRestoreFailed"),
  },
};

const TABLE_PRESENTATIONS: Readonly<Record<string, SyncTablePresentation>> = {
  project_groups: GROUPS_PRESENTATION,
  projects: PROJECTS_PRESENTATION,
  project_sections: SECTIONS_PRESENTATION,
  project_statuses: STATUSES_PRESENTATION,
  project_tasks: TASKS_PRESENTATION,
  project_checklist_items: CHECKLIST_PRESENTATION,
  project_task_dependencies: DEPENDENCIES_PRESENTATION,
};

/** Tells this window and the others that recovery changed Projects rows, so they reload. */
function projectsChanged(): void {
  notifyProjectsSyncChanged();
  publishProjectsSyncChange();
}

/** How the sync settings present Projects. Tables without their own copy use neutral sync copy. */
export const PROJECTS_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("titleBar.tab.projects"),
  tables: Object.fromEntries([...PROJECTS_SYNC_TABLES].map((table) => [table, TABLE_PRESENTATIONS[table] ?? {}])),
  changed: projectsChanged,
};
