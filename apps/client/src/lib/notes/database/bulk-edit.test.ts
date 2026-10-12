import { describe, expect, it } from "vitest";
import type { NotesDataSourcePropertyType, NotesPage } from "$lib/notes/types";
import { notesDatabaseBulkEditableColumns, notesDatabaseBulkToggledItem, notesDatabaseBulkValue } from "./bulk-edit";
import type { NotesDatabaseTableColumn } from "./table";

function column(id: string, type: NotesDataSourcePropertyType, hidden = false): NotesDatabaseTableColumn {
  return { id, name: id, type, hidden, width: 180, options: [], relationDataSourceId: null, buttonLabel: "", buttonRequiresConfirmation: false };
}

function row(id: string, properties: NotesPage["properties"]): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-07-01T00:00:00.000Z",
    last_edited_time: "2026-07-01T00:00:00.000Z",
    parent: { type: "data_source_id", data_source_id: "11111111-1111-4111-8111-111111111111" },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties,
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

const tags = column("tags", "multi_select");
const status = column("status", "select");
const done = column("done", "checkbox");
const due = column("due", "date");
const project = column("project", "relation");

function tagged(id: string, names: string[]): NotesPage {
  return row(id, { Tags: { id: "tags", type: "multi_select", multi_select: names.map((name) => ({ id: name.toLowerCase(), name, color: "default" })) } });
}

describe("database bulk edits", () => {
  it("offers visible, settable properties in view order and never the title", () => {
    const columns = [
      column("title", "title"),
      column("estimate", "number"),
      column("hidden", "select", true),
      column("created", "created_time"),
      column("score", "formula"),
      column("files", "files"),
      column("owner", "people"),
      column("place", "place"),
      tags,
      due,
      project,
    ];

    expect(notesDatabaseBulkEditableColumns(columns).map((entry) => entry.id)).toEqual(["estimate", "tags", "due", "project"]);
  });

  it("reads the shared value of select, checkbox, and date properties", () => {
    const date = { start: "2026-07-02", end: null, time_zone: null };
    const rows = [
      row("a", { Status: { id: "status", type: "select", select: { id: "done", name: "Done", color: "green" } }, Done: { id: "done", type: "checkbox", checkbox: true }, Due: { id: "due", type: "date", date: date } }),
      row("b", { Status: { id: "status", type: "select", select: { id: "done", name: "Done", color: "green" } }, Done: { id: "done", type: "checkbox", checkbox: true }, Due: { id: "due", type: "date", date: date } }),
    ];

    expect(notesDatabaseBulkValue(rows, status)).toEqual({ value: "Done", mixed: false });
    expect(notesDatabaseBulkValue(rows, done)).toEqual({ value: true, mixed: false });
    expect(notesDatabaseBulkValue(rows, due)).toEqual({ value: date, mixed: false });
  });

  it("starts empty and reports mixed values when rows differ", () => {
    const rows = [
      row("a", { Status: { id: "status", type: "select", select: { id: "done", name: "Done", color: "green" } }, Done: { id: "done", type: "checkbox", checkbox: true } }),
      row("b", { Done: { id: "done", type: "checkbox", checkbox: false } }),
    ];

    expect(notesDatabaseBulkValue(rows, status)).toEqual({ value: "", mixed: true });
    expect(notesDatabaseBulkValue(rows, done)).toEqual({ value: false, mixed: true });
    expect(notesDatabaseBulkValue(rows, due)).toEqual({ value: null, mixed: false });
    expect(notesDatabaseBulkValue([tagged("a", ["Work"]), tagged("b", ["Home"])], tags)).toEqual({ value: [], mixed: true });
  });

  it("compares option names and relation targets regardless of order", () => {
    expect(notesDatabaseBulkValue([tagged("a", ["Work", "Urgent"]), tagged("b", ["Urgent", "Work"])], tags))
      .toEqual({ value: ["Work", "Urgent"], mixed: false });
    const related = (id: string, targets: string[]) => row(id, { Project: { id: "project", type: "relation", relation: targets.map((target) => ({ id: target, title: target })), has_more: false } });

    expect(notesDatabaseBulkValue([related("a", ["x", "y"]), related("b", ["y", "x"])], project)).toEqual({ value: ["x", "y"], mixed: false });
    expect(notesDatabaseBulkValue([related("a", ["x"]), related("b", ["x", "y"])], project)).toEqual({ value: [], mixed: true });
  });

  it("reads no value and no mix without rows", () => {
    expect(notesDatabaseBulkValue([], tags)).toEqual({ value: [], mixed: false });
  });

  it("toggles one list item while keeping the order of the others", () => {
    expect(notesDatabaseBulkToggledItem(["Work", "Urgent"], "Home")).toEqual(["Work", "Urgent", "Home"]);
    expect(notesDatabaseBulkToggledItem(["Work", "Urgent", "Home"], "Urgent")).toEqual(["Work", "Home"]);
  });
});
