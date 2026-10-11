import { describe, expect, it } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import { NOTES_PAGE_PROJECT_ID_PROPERTY } from "$lib/notes/project-membership";
import { createTextRichText } from "$lib/notes/rich-text/core";
import type { NotesDataSourceListItem, NotesFolder, NotesPage } from "$lib/notes/types";
import {
  buildNotesMentionTargets,
  NOTES_MENTION_LOCATION_MAX_DEPTH,
  notesPageMentionLocationResolver,
} from "./mention-targets";

function page(
  id: string,
  title: string,
  options: { parent?: NotesPage["parent"]; folderId?: string; projectId?: string } = {},
): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
    parent: options.parent ?? { type: "workspace", workspace: true },
    folder_id: options.folderId ?? null,
    in_trash: false,
    icon: null,
    cover: null,
    properties: {
      title: { title: title ? [createTextRichText(title)] : [] },
      ...(options.projectId ? { [NOTES_PAGE_PROJECT_ID_PROPERTY]: options.projectId } : {}),
    },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

function childOf(parentId: string): NotesPage["parent"] {
  return { type: "page_id", page_id: parentId };
}

function folder(id: string, name: string, parentFolderId: string | null = null, projectId = "project-1"): NotesFolder {
  return {
    object: "folder",
    id,
    project_id: projectId,
    parent_folder_id: parentFolderId,
    name,
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
  };
}

function source(id: string, title: string, databaseId: string, location: string[]): NotesDataSourceListItem {
  return {
    object: "data_source", id, title, title_rich_text: [], description: [], icon: null, in_trash: false,
    parent: { type: "database_id", database_id: databaseId }, database_parent: { type: "page_id", page_id: "holder" },
    properties: {}, created_time: "2026-01-01T00:00:00Z", last_edited_time: "2026-01-01T00:00:00Z",
    source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null, location,
  };
}

const translate = ((key: string, ...args: unknown[]) => (
  args.length > 0 ? `${key}:${String(args[0])}` : key
)) as Translate;

const projects = [{ id: "project-1", name: "Productivity", status: "active", groupId: "group-1" }];
const projectGroups = [{ id: "group-1", name: "Personal" }];

describe("Notes block mention targets", () => {
  it("keeps target family order and deduplicates row pages already in the page list", () => {
    const firstPage = page("page-1", "First");
    const targets = buildNotesMentionTargets({
      pages: [firstPage],
      localUser: {
        object: "user",
        id: "user-1",
        display_name: "Alice",
        created_time: "2026-01-01T00:00:00Z",
        last_edited_time: "2026-01-01T00:00:00Z",
      },
      dataSources: [],
      dataSourceRowPages: [firstPage, page("page-2", "Second")],
      projects: [{ id: "project-1", name: "Project", status: "active" }],
      tasks: [],
      calendarEvents: [],
      translate,
    });

    expect(targets.map((target) => `${target.kind}:${target.id}`)).toEqual([
      "user:user-1",
      "page:page-1",
      "page:page-2",
      "project:project-1",
    ]);
  });

  it("gives pages and databases their routes", () => {
    const parent = page("parent", "Plans");
    const child = page("child", "Week", { parent: childOf("parent") });
    const targets = buildNotesMentionTargets({
      pages: [parent, child],
      localUser: null,
      dataSources: [source("source-1", "Tasks", "database-1", ["Personal", "Plans"])],
      dataSourceRowPages: [],
      projects: [],
      tasks: [],
      calendarEvents: [],
      translate,
    });

    expect(targets.map((target) => ("location" in target ? target.location : undefined))).toEqual([
      [],
      ["Plans"],
      ["Personal", "Plans"],
    ]);
  });

  it("offers the scoped tasks and events with their project and when they happen", () => {
    const event = {
      id: "event-1", title: "", start_time: "2026-10-12T09:00:00Z", end_time: "2026-10-12T10:00:00Z",
      all_day: false, recurring: false, calendar_id: "calendar", project_id: "project-1",
    };
    const targets = buildNotesMentionTargets({
      pages: [],
      localUser: null,
      dataSources: [],
      dataSourceRowPages: [],
      projects,
      tasks: [
        { id: "task-1", project_id: "project-1", title: "Draft", completed: false },
        { id: "task-2", project_id: "project-unloaded", title: "", completed: true },
      ],
      calendarEvents: [event, { ...event, id: "event-2", title: "Sync" }],
      calendarEventWhen: (candidate) => (candidate.id === "event-1" ? "Mon 09:00" : null),
      translate,
    });

    expect(targets.filter((target) => target.kind !== "project")).toEqual([
      { kind: "project_task", id: "task-1", title: "Draft", subtitle: "notes.mentionTargetProjectTaskIn:Productivity" },
      { kind: "project_task", id: "task-2", title: "notes.untitled", subtitle: "notes.mentionTargetProjectTask" },
      { kind: "calendar_event", id: "event-1", title: "notes.untitled", subtitle: "Mon 09:00" },
      { kind: "calendar_event", id: "event-2", title: "Sync", subtitle: "notes.mentionTargetCalendarEvent" },
    ]);
  });
});

describe("Notes page mention routes", () => {
  it("leads with the project group, project, and folders above the outermost page", () => {
    const folders = [folder("outer", "Areas"), folder("inner", "Health", "outer")];
    const top = page("top", "", { folderId: "inner", projectId: "project-1" });
    const leaf = page("leaf", "Sleep", { parent: childOf("top") });
    const location = notesPageMentionLocationResolver({ pages: [top, leaf], dataSources: [], folders, projectGroups, projects });

    expect(location(leaf)).toEqual(["Personal", "Productivity", "Areas", "Health", ""]);
    expect(location(top)).toEqual(["Personal", "Productivity", "Areas", "Health"]);
  });

  it("finds the project through the outermost folder when pages carry no project", () => {
    const folders = [folder("outer", "Areas")];
    const top = page("top", "Plans", { folderId: "outer" });
    const location = notesPageMentionLocationResolver({ pages: [top], dataSources: [], folders, projectGroups, projects });

    expect(location(top)).toEqual(["Personal", "Productivity", "Areas"]);
  });

  it("omits the project prefix unless both the project and its group are known", () => {
    const top = page("top", "Plans", { projectId: "project-1" });
    const ungrouped = [{ id: "project-1", name: "Productivity", status: "active" }];

    expect(notesPageMentionLocationResolver({ pages: [top], dataSources: [], projects: ungrouped })(top)).toEqual([]);
    expect(notesPageMentionLocationResolver({ pages: [top], dataSources: [], projects })(top)).toEqual([]);
  });

  it("continues a row page's route from its data source location and title", () => {
    const row = page("row", "Task", { parent: { type: "data_source_id", data_source_id: "source-1" } });
    const nested = page("nested", "Notes", { parent: childOf("row") });
    const location = notesPageMentionLocationResolver({
      pages: [nested],
      dataSources: [source("source-1", "Tasks", "database-1", ["Personal", "Plans"])],
      projects: [],
    });

    expect(location(row)).toEqual(["Personal", "Plans", "Tasks"]);
    expect(location(nested)).toEqual([]);
    expect(notesPageMentionLocationResolver({
      pages: [row, nested],
      dataSources: [source("source-1", "Tasks", "database-1", ["Personal", "Plans"])],
      projects: [],
    })(nested)).toEqual(["Personal", "Plans", "Tasks", "Task"]);
  });

  it("stops at parent cycles and the depth bound", () => {
    const first = page("first", "First", { parent: childOf("second") });
    const second = page("second", "Second", { parent: childOf("first") });
    expect(notesPageMentionLocationResolver({ pages: [first, second], dataSources: [], projects: [] })(first))
      .toEqual(["Second"]);

    const chain = Array.from({ length: NOTES_MENTION_LOCATION_MAX_DEPTH + 10 }, (_, index) =>
      page(`page-${index}`, `Page ${index}`, index === 0 ? {} : { parent: childOf(`page-${index - 1}`) }));
    const leaf = chain.at(-1);
    if (!leaf) throw new Error("expected a page chain");
    const location = notesPageMentionLocationResolver({ pages: chain, dataSources: [], projects: [] })(leaf);
    expect(location).toHaveLength(NOTES_MENTION_LOCATION_MAX_DEPTH);
    expect(location.at(-1)).toBe(`Page ${chain.length - 2}`);
  });
});
