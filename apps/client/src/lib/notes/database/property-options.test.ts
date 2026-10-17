import { describe, expect, it } from "vitest";
import type { NotesDataSourceSchemaOptionDraft, NotesDataSourceSchemaPropertyDraft } from "./data-source-schema";
import {
  NOTES_DATA_SOURCE_MAX_OPTIONS,
  NOTES_DATA_SOURCE_OPTION_NAME_MAX_CHARACTERS,
  notesDataSourceAddOption,
  notesDataSourceDeleteOption,
  notesDataSourceMoveOption,
  notesDataSourceRecolorOption,
  notesDataSourceRenameOption,
  notesDataSourceSetDefaultOption,
  notesDataSourceStepOption,
  notesDataSourceVisibleOptions,
  type NotesDataSourceOptionEdit,
} from "./property-options";

type OptionProperty = Pick<NotesDataSourceSchemaPropertyDraft, "type" | "options" | "statusDefaultOptionId">;

function option(id: string, group: NotesDataSourceSchemaOptionDraft["group"] = "To-do", name = id.toUpperCase()): NotesDataSourceSchemaOptionDraft {
  return { id, name, color: "default", group };
}

function select(...ids: string[]): OptionProperty {
  return { type: "select", options: ids.map((id) => option(id)), statusDefaultOptionId: "" };
}

/** Status options saved out of group order, as people can create them, so tests prove the visible order is used. */
function status(): OptionProperty {
  return {
    type: "status",
    options: [option("done", "Complete"), option("todo"), option("doing", "In progress"), option("later")],
    statusDefaultOptionId: "todo",
  };
}

function ids(edit: NotesDataSourceOptionEdit): string[] {
  if (edit.status !== "changed") throw new Error(`Expected a change, received ${edit.status}`);
  return edit.patch.options.map((current) => `${current.id}:${current.group}`);
}

function selectIds(edit: NotesDataSourceOptionEdit): string[] {
  if (edit.status !== "changed") throw new Error(`Expected a change, received ${edit.status}`);
  return edit.patch.options.map((current) => current.id);
}

describe("notes data source option editing", () => {
  it("lists status options by group while other types keep their saved order", () => {
    expect(notesDataSourceVisibleOptions(status()).map((current) => current.id)).toEqual(["todo", "later", "doing", "done"]);
    expect(notesDataSourceVisibleOptions(select("b", "a")).map((current) => current.id)).toEqual(["b", "a"]);
  });

  it("adds trimmed options with rotating colors and rejects blanks, duplicates, and full lists", () => {
    const added = notesDataSourceAddOption(select(), "  Urgent  ", undefined, "new");
    expect(added).toEqual({
      status: "changed",
      patch: { options: [{ id: "new", name: "Urgent", color: "blue", group: "To-do" }], statusDefaultOptionId: "" },
    });
    const second = notesDataSourceAddOption(select("a"), "Later", undefined, "new");
    expect(second.status === "changed" && second.patch.options[1]?.color).toBe("green");
    expect(notesDataSourceAddOption(select("a"), "   ")).toEqual({ status: "unchanged" });
    expect(notesDataSourceAddOption(select("a"), " a ")).toEqual({ status: "duplicate" });
    const full = select(...Array.from({ length: NOTES_DATA_SOURCE_MAX_OPTIONS }, (_, index) => `o${index}`));
    expect(notesDataSourceAddOption(full, "One more")).toEqual({ status: "limit" });
  });

  it("caps option names by characters rather than UTF-16 units", () => {
    const name = "🙂".repeat(NOTES_DATA_SOURCE_OPTION_NAME_MAX_CHARACTERS + 5);
    const added = notesDataSourceAddOption(select(), name, undefined, "new");
    expect(added.status === "changed" && Array.from(added.patch.options[0]?.name ?? "")).toHaveLength(NOTES_DATA_SOURCE_OPTION_NAME_MAX_CHARACTERS);
  });

  it("adds a status option at the end of its group, including an empty group between others", () => {
    expect(ids(notesDataSourceAddOption(status(), "Blocked", "To-do", "new"))).toEqual([
      "todo:To-do", "later:To-do", "new:To-do", "doing:In progress", "done:Complete",
    ]);
    const withoutProgress: OptionProperty = { ...status(), options: status().options.filter((current) => current.group !== "In progress") };
    expect(ids(notesDataSourceAddOption(withoutProgress, "Review", "In progress", "new"))).toEqual([
      "todo:To-do", "later:To-do", "new:In progress", "done:Complete",
    ]);
  });

  it("renames options without colliding with another option's name", () => {
    const renamed = notesDataSourceRenameOption(select("a", "b"), "a", " First ");
    expect(renamed.status === "changed" && renamed.patch.options[0]?.name).toBe("First");
    expect(notesDataSourceRenameOption(select("a", "b"), "a", "b")).toEqual({ status: "duplicate" });
    expect(notesDataSourceRenameOption(select("a"), "a", "  ")).toEqual({ status: "unchanged" });
    expect(notesDataSourceRenameOption(select("a"), "a", "A")).toEqual({ status: "unchanged" });
    expect(notesDataSourceRenameOption(select("a"), "missing", "Name")).toEqual({ status: "missing" });
    const recased = notesDataSourceRenameOption(select("a"), "a", "a");
    expect(recased.status === "changed" && recased.patch.options[0]?.name).toBe("a");
  });

  it("recolors one option and ignores the color it already has", () => {
    const recolored = notesDataSourceRecolorOption(select("a", "b"), "b", "red");
    expect(recolored.status === "changed" && recolored.patch.options.map((current) => current.color)).toEqual(["default", "red"]);
    expect(notesDataSourceRecolorOption(select("a"), "a", "default")).toEqual({ status: "unchanged" });
  });

  it("clears the status default only when the default option is deleted", () => {
    const deletedDefault = notesDataSourceDeleteOption(status(), "todo");
    expect(deletedDefault.status === "changed" && deletedDefault.patch.statusDefaultOptionId).toBe("");
    const deletedOther = notesDataSourceDeleteOption(status(), "done");
    expect(deletedOther.status === "changed" && deletedOther.patch.statusDefaultOptionId).toBe("todo");
    expect(notesDataSourceDeleteOption(status(), "missing")).toEqual({ status: "missing" });
  });

  it("sets the status default without reordering options", () => {
    const edit = notesDataSourceSetDefaultOption(status(), "doing");
    expect(edit.status === "changed" && edit.patch.statusDefaultOptionId).toBe("doing");
    expect(selectIds(edit)).toEqual(status().options.map((current) => current.id));
    expect(notesDataSourceSetDefaultOption(status(), "todo")).toEqual({ status: "unchanged" });
  });

  it("moves options next to a target and reports drops that change nothing", () => {
    expect(selectIds(notesDataSourceMoveOption(select("a", "b", "c"), "a", { optionId: "c", placement: "after" }))).toEqual(["b", "c", "a"]);
    expect(selectIds(notesDataSourceMoveOption(select("a", "b", "c"), "c", { optionId: "a", placement: "before" }))).toEqual(["c", "a", "b"]);
    expect(notesDataSourceMoveOption(select("a", "b"), "a", { optionId: "b", placement: "before" })).toEqual({ status: "unchanged" });
    expect(notesDataSourceMoveOption(select("a", "b"), "a", { optionId: "a", placement: "after" })).toEqual({ status: "unchanged" });
    expect(notesDataSourceMoveOption(select("a"), "a", { optionId: "missing", placement: "after" })).toEqual({ status: "missing" });
  });

  it("moves status options across groups by target or into an empty group", () => {
    expect(ids(notesDataSourceMoveOption(status(), "todo", { optionId: "done", placement: "before" }))).toEqual([
      "later:To-do", "doing:In progress", "todo:Complete", "done:Complete",
    ]);
    expect(ids(notesDataSourceMoveOption(status(), "doing", null, "Complete"))).toEqual([
      "todo:To-do", "later:To-do", "done:Complete", "doing:Complete",
    ]);
    const withoutProgress: OptionProperty = { ...status(), options: status().options.filter((current) => current.group !== "In progress") };
    expect(ids(notesDataSourceMoveOption(withoutProgress, "done", null, "In progress"))).toEqual([
      "todo:To-do", "later:To-do", "done:In progress",
    ]);
    expect(notesDataSourceMoveOption(status(), "done", null, "Complete")).toEqual({ status: "unchanged" });
  });

  it("steps options one place and stops at the ends", () => {
    expect(selectIds(notesDataSourceStepOption(select("a", "b", "c"), "b", -1))).toEqual(["b", "a", "c"]);
    expect(selectIds(notesDataSourceStepOption(select("a", "b", "c"), "b", 1))).toEqual(["a", "c", "b"]);
    expect(notesDataSourceStepOption(select("a", "b"), "a", -1)).toEqual({ status: "unchanged" });
    expect(notesDataSourceStepOption(select("a", "b"), "b", 1)).toEqual({ status: "unchanged" });
  });

  it("steps status options within a group, then into the neighboring group, then stops", () => {
    expect(ids(notesDataSourceStepOption(status(), "todo", 1))).toEqual([
      "later:To-do", "todo:To-do", "doing:In progress", "done:Complete",
    ]);
    expect(ids(notesDataSourceStepOption(status(), "later", 1))).toEqual([
      "todo:To-do", "later:In progress", "doing:In progress", "done:Complete",
    ]);
    expect(ids(notesDataSourceStepOption(status(), "doing", -1))).toEqual([
      "todo:To-do", "later:To-do", "doing:To-do", "done:Complete",
    ]);
    expect(notesDataSourceStepOption(status(), "todo", -1)).toEqual({ status: "unchanged" });
    expect(notesDataSourceStepOption(status(), "done", 1)).toEqual({ status: "unchanged" });
  });

  it("steps a status option into an empty neighboring group", () => {
    const withoutProgress: OptionProperty = { ...status(), options: status().options.filter((current) => current.group !== "In progress") };
    expect(ids(notesDataSourceStepOption(withoutProgress, "later", 1))).toEqual([
      "todo:To-do", "later:In progress", "done:Complete",
    ]);
    expect(ids(notesDataSourceStepOption(withoutProgress, "done", -1))).toEqual([
      "todo:To-do", "later:To-do", "done:In progress",
    ]);
  });
});
