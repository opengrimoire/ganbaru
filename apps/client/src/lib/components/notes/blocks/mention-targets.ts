import {
  listNotesDataSources,
  listNotesDataSourceRowPages,
} from "$lib/api/notes";
import type { Translate } from "$lib/i18n/translator.svelte";
import { notesLocalUserDisplayName } from "$lib/notes/collaboration/local-user";
import { notesPageIconText } from "$lib/notes/pages/icon";
import { notesPageTitle } from "$lib/notes/pages/title";
import type {
  NotesNamedMentionTarget,
  NotesObjectMentionTarget,
  NotesPageMentionTarget,
} from "$lib/notes/rich-text/core";
import { notesPageProjectId } from "$lib/notes/project-membership";
import type {
  NotesDataSourceListItem,
  NotesFolder,
  NotesLocalUser,
  NotesMentionScopeEvent,
  NotesMentionScopeTask,
  NotesPage,
} from "$lib/notes/types";

/** Most ancestors a mention route follows, matching the bound the backend applies to data source locations. */
export const NOTES_MENTION_LOCATION_MAX_DEPTH = 64;

export interface NotesDatabaseMentionData {
  dataSources: NotesDataSourceListItem[];
  rowPages: NotesPage[];
}

export interface NotesMentionProject {
  id: string;
  name: string;
  status: string;
  groupId?: string;
}

export interface NotesMentionProjectGroup {
  id: string;
  name: string;
}

export interface NotesMentionTargetInput {
  pages: readonly NotesPage[];
  localUser: NotesLocalUser | null;
  dataSources: readonly NotesDataSourceListItem[];
  dataSourceRowPages: readonly NotesPage[];
  /** Folders and project groups complete page routes; without them a route starts at the outermost page. */
  folders?: readonly NotesFolder[];
  projectGroups?: readonly NotesMentionProjectGroup[];
  projects: readonly NotesMentionProject[];
  /** Tasks and events from the editing page's project group; other projects' tasks and events are not offered. */
  tasks: readonly NotesMentionScopeTask[];
  calendarEvents: readonly NotesMentionScopeEvent[];
  /** Say when an event happens for its subtitle; without it, or when it returns null, the subtitle names the kind. */
  calendarEventWhen?: (event: NotesMentionScopeEvent) => string | null;
  translate: Translate;
}

/** Load local database mention sources while isolating failures by data source. */
export async function loadNotesDatabaseMentionData(): Promise<NotesDatabaseMentionData> {
  const dataSources = await listNotesDataSources();
  const rowPageGroups = await Promise.all(
    dataSources.map(async (dataSource) => {
      try {
        return await listNotesDataSourceRowPages(dataSource.id);
      } catch (error) {
        console.warn("notes mention data source row targets failed", error);
        return [];
      }
    }),
  );
  return { dataSources, rowPages: rowPageGroups.flat() };
}

/**
 * Resolve where a page lives, outermost first, in the shape the backend uses for data source locations: project
 * group and project, folders, then the titles of the pages above it. A page inside a database continues from that
 * data source's location and title. Titles stay raw, so untitled pages appear as empty strings.
 *
 * @param input The pages, data sources, folders, and projects that routes may pass through.
 * @returns A resolver from a page to its route segments, empty for a page at the top of the vault. The page's own
 * title is not part of its route.
 */
export function notesPageMentionLocationResolver(
  input: Pick<NotesMentionTargetInput, "pages" | "dataSources" | "folders" | "projectGroups" | "projects">,
): (page: NotesPage) => string[] {
  const pagesById = new Map(input.pages.map((page) => [page.id, page]));
  const sourcesById = new Map(input.dataSources.map((source) => [source.id, source]));
  const foldersById = new Map((input.folders ?? []).map((folder) => [folder.id, folder]));
  const projectsById = new Map(input.projects.map((project) => [project.id, project]));
  const groupsById = new Map((input.projectGroups ?? []).map((group) => [group.id, group]));

  function folderPath(folderId: string | null): NotesFolder[] {
    const folders: NotesFolder[] = [];
    for (let folder = folderId ? foldersById.get(folderId) : undefined; folder; ) {
      if (folders.includes(folder) || folders.length >= NOTES_MENTION_LOCATION_MAX_DEPTH) break;
      folders.unshift(folder);
      folder = folder.parent_folder_id ? foldersById.get(folder.parent_folder_id) : undefined;
    }
    return folders;
  }

  return (page) => {
    const chain: NotesPage[] = [page];
    const titles: string[] = [];
    const visited = new Set([page.id]);
    let current = page;
    for (let depth = 0; depth < NOTES_MENTION_LOCATION_MAX_DEPTH; depth += 1) {
      const parent = current.parent;
      if (parent.type === "data_source_id") {
        const source = sourcesById.get(parent.data_source_id);
        return source ? [...source.location, source.title, ...titles] : titles;
      }
      if (parent.type !== "page_id") break;
      const next = pagesById.get(parent.page_id);
      if (!next || visited.has(next.id)) break;
      visited.add(next.id);
      titles.unshift(notesPageTitle(next));
      chain.unshift(next);
      current = next;
    }
    const folders = folderPath(chain.find((candidate) => candidate.folder_id)?.folder_id ?? null);
    const projectId = chain.map(notesPageProjectId).find((id) => id !== null) ?? folders[0]?.project_id ?? null;
    const project = projectId ? projectsById.get(projectId) : undefined;
    const group = project?.groupId ? groupsById.get(project.groupId) : undefined;
    const projectPath = project && group ? [group.name, project.name] : [];
    return [...projectPath, ...folders.map((folder) => folder.name), ...titles];
  };
}

/** Build the ordered, deduplicated mention catalog shown by Notes text editors. */
export function buildNotesMentionTargets(input: NotesMentionTargetInput): NotesNamedMentionTarget[] {
  const { translate: t } = input;
  const dataSourceById = new Map(input.dataSources.map((source) => [source.id, source]));
  const pagesById = new Map(input.pages.map((page) => [page.id, page]));
  const pageLocation = notesPageMentionLocationResolver(input);
  const pageTargets: NotesPageMentionTarget[] = [];
  const seenPages = new Set<string>();
  for (const page of [...input.pages, ...input.dataSourceRowPages]) {
    if (seenPages.has(page.id)) continue;
    seenPages.add(page.id);
    let subtitle: string;
    if (page.parent.type === "data_source_id") {
      const dataSource = dataSourceById.get(page.parent.data_source_id);
      subtitle = dataSource
        ? t("notes.mentionTargetDataSourceRowIn", dataSource.title || t("notes.untitled"))
        : t("notes.mentionTargetDataSourceRow");
    } else {
      const parent = page.parent.type === "page_id" ? pagesById.get(page.parent.page_id) : null;
      subtitle = parent ? notesPageTitle(parent, t("notes.untitled")) : t("notes.workspace");
    }
    pageTargets.push({
      kind: "page",
      id: page.id,
      title: notesPageTitle(page, t("notes.untitled")),
      subtitle,
      iconText: notesPageIconText(page.icon),
      location: pageLocation(page),
    });
  }

  const userTargets: NotesObjectMentionTarget[] = input.localUser
    ? [{
        kind: "user",
        id: input.localUser.id,
        title: notesLocalUserDisplayName(input.localUser.display_name),
        subtitle: t("notes.mentionTargetLocalUser"),
      }]
    : [];
  const seenDatabases = new Set<string>();
  const databaseTargets: NotesObjectMentionTarget[] = [];
  for (const dataSource of input.dataSources) {
    const databaseId = dataSource.parent.database_id;
    if (seenDatabases.has(databaseId)) continue;
    seenDatabases.add(databaseId);
    databaseTargets.push({
      kind: "database",
      id: databaseId,
      title: dataSource.title || t("notes.untitled"),
      subtitle: t("notes.mentionTargetDatabase"),
      iconText: notesPageIconText(dataSource.icon),
      location: dataSource.location,
    });
  }
  const projectById = new Map(input.projects.map((project) => [project.id, project]));
  const projectTargets: NotesObjectMentionTarget[] = input.projects
    .filter((project) => project.status === "active")
    .map((project) => ({
      kind: "project",
      id: project.id,
      title: project.name,
      subtitle: t("notes.mentionTargetProject"),
    }));
  const taskTargets: NotesObjectMentionTarget[] = input.tasks.map((task) => {
    const project = projectById.get(task.project_id);
    return {
      kind: "project_task",
      id: task.id,
      title: task.title || t("notes.untitled"),
      subtitle: project
        ? t("notes.mentionTargetProjectTaskIn", project.name)
        : t("notes.mentionTargetProjectTask"),
    };
  });
  const calendarTargets: NotesObjectMentionTarget[] = input.calendarEvents.map((event) => ({
    kind: "calendar_event",
    id: event.id,
    title: event.title || t("notes.untitled"),
    subtitle: input.calendarEventWhen?.(event) ?? t("notes.mentionTargetCalendarEvent"),
  }));

  return [
    ...userTargets,
    ...pageTargets,
    ...databaseTargets,
    ...projectTargets,
    ...taskTargets,
    ...calendarTargets,
  ];
}
