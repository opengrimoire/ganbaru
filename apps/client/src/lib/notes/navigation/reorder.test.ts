import { describe, expect, it } from "vitest";
import {
  notesNavigationReorderBand,
  notesNavigationReorderedKeys,
  notesNavigationSiblingGroups,
  notesNavigationSteppedKeys,
  type NotesNavigationReorderRow,
} from "./reorder";

function row(key: string, depth: number, favorite = false): NotesNavigationReorderRow {
  return { key, kind: key.startsWith("folder:") ? "folder" : "page", depth, favorite };
}

/** Root: two folders (the first open with its own notes), a favorite note, then two notes. */
const groups = notesNavigationSiblingGroups([
  row("folder:a", 0),
  row("page:a1", 1),
  row("page:a2", 1),
  row("folder:b", 0),
  row("page:starred", 0, true),
  row("page:x", 0),
  row("page:y", 0),
]);

describe("notes navigation reorder", () => {
  it("lists only rows under the same parent with the same kind and favorite state", () => {
    expect(groups.get("folder:b") ?? []).toEqual(["folder:a", "folder:b"]);
    expect(groups.get("page:x") ?? []).toEqual(["page:x", "page:y"]);
    expect(groups.get("page:starred") ?? []).toEqual(["page:starred"]);
    expect(groups.get("page:a2") ?? []).toEqual(["page:a1", "page:a2"]);
    expect(groups.get("page:missing") ?? []).toEqual([]);
  });

  it("splits note rows in half and keeps the middle of folder rows for moving into them", () => {
    expect(notesNavigationReorderBand("page", 0.49)).toBe("before");
    expect(notesNavigationReorderBand("page", 0.5)).toBe("after");
    expect(notesNavigationReorderBand("folder", 0.2)).toBe("before");
    expect(notesNavigationReorderBand("folder", 0.5)).toBe("into");
    expect(notesNavigationReorderBand("folder", 0.8)).toBe("after");
  });

  it("places a row before or after a sibling and ignores drops that change nothing", () => {
    expect(notesNavigationReorderedKeys(groups, "page:y", "page:x", "before")).toEqual(["page:y", "page:x"]);
    expect(notesNavigationReorderedKeys(groups, "folder:a", "folder:b", "after")).toEqual(["folder:b", "folder:a"]);
    expect(notesNavigationReorderedKeys(groups, "page:x", "page:y", "before")).toBeNull();
    expect(notesNavigationReorderedKeys(groups, "page:x", "page:x", "after")).toBeNull();
  });

  it("refuses rows from another parent, kind, or favorite state", () => {
    expect(notesNavigationReorderedKeys(groups, "page:a1", "page:x", "before")).toBeNull();
    expect(notesNavigationReorderedKeys(groups, "page:x", "folder:b", "after")).toBeNull();
    expect(notesNavigationReorderedKeys(groups, "page:x", "page:starred", "before")).toBeNull();
  });

  it("moves a row one step within its siblings and stops at either end", () => {
    expect(notesNavigationSteppedKeys(groups, "page:y", -1)).toEqual(["page:y", "page:x"]);
    expect(notesNavigationSteppedKeys(groups, "folder:a", 1)).toEqual(["folder:b", "folder:a"]);
    expect(notesNavigationSteppedKeys(groups, "page:x", -1)).toBeNull();
    expect(notesNavigationSteppedKeys(groups, "page:starred", 1)).toBeNull();
  });
});
