import { describe, expect, it } from "vitest";
import {
  flatNotesDestinationPickerTargets,
  nextNotesDestinationPickerIndex,
  notesDestinationPickerSections,
  type NotesDestinationPickerTarget,
} from "./destination-picker";

function target(
  key: string,
  title: string,
  path: readonly string[] = [],
  recent = false,
): NotesDestinationPickerTarget {
  return {
    key,
    title,
    path,
    depth: path.length,
    recent,
  };
}

describe("notes destination picker", () => {
  it("splits recent destinations from the nested page list without a query", () => {
    const sections = notesDestinationPickerSections(
      [
        target("root", "Root"),
        target("recent", "Sprint notes", ["Projects"], true),
      ],
      "",
    );

    expect(sections.recent.map((item) => item.key)).toEqual(["recent"]);
    expect(sections.pages.map((item) => item.key)).toEqual(["root"]);
  });

  it("searches titles and nested path context", () => {
    const sections = notesDestinationPickerSections(
      [
        target("root", "Root"),
        target("child", "Sprint notes", ["Projects"]),
      ],
      "projects",
    );

    expect(sections.recent).toEqual([]);
    expect(flatNotesDestinationPickerTargets(sections).map((item) => item.key)).toEqual(["child"]);
  });

  it("moves the active index with wrapping keyboard navigation", () => {
    expect(nextNotesDestinationPickerIndex(3, 0, "ArrowUp")).toBe(2);
    expect(nextNotesDestinationPickerIndex(3, 2, "ArrowDown")).toBe(0);
    expect(nextNotesDestinationPickerIndex(3, 1, "Home")).toBe(0);
    expect(nextNotesDestinationPickerIndex(3, 1, "End")).toBe(2);
    expect(nextNotesDestinationPickerIndex(0, 1, "ArrowDown")).toBe(-1);
  });
});
