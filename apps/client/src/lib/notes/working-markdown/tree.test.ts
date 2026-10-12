import { describe, expect, it } from "vitest";
import {
  buildWorkingMarkdownTreeItems,
  workingMarkdownNodeKey,
} from "./tree";
import type { NotesWorkingMarkdownRoot } from "$lib/notes/types";

const roots: NotesWorkingMarkdownRoot[] = [{
  workingFolderId: "folder-1",
  displayName: "Repository",
  sourceKind: "external",
  displayPath: "/work/repository",
  truncated: false,
  nodes: [{
    kind: "directory",
    name: "docs",
    relativePath: "docs",
    children: [{
      kind: "file",
      name: "guide.md",
      relativePath: "docs/guide.md",
      children: [],
    }],
  }, {
    kind: "file",
    name: "README.md",
    relativePath: "README.md",
    children: [],
  }],
}];

describe("buildWorkingMarkdownTreeItems", () => {
  it("uses source-aware identities and respects collapsed directories", () => {
    const collapsed = new Set([
      workingMarkdownNodeKey("folder-1", "directory", "docs"),
    ]);
    const rows = buildWorkingMarkdownTreeItems(roots, collapsed, "");
    expect(rows.map((row) => row.key)).toEqual([
      "working-folder:folder-1",
      "working-folder:folder-1:directory:docs",
      "working-folder:folder-1:file:README.md",
    ]);
  });

  it("reveals matching files and their directory ancestors during filtering", () => {
    const rows = buildWorkingMarkdownTreeItems(roots, new Set(), "guide");
    expect(rows.map((row) => [row.kind, row.name])).toEqual([
      ["root", "Repository"],
      ["directory", "docs"],
      ["file", "guide.md"],
    ]);
  });

  it("allows duplicate names in different working folders", () => {
    const duplicate = { ...roots[0], workingFolderId: "folder-2" };
    const rows = buildWorkingMarkdownTreeItems([roots[0]!, duplicate], new Set(), "README");
    expect(new Set(rows.map((row) => row.key)).size).toBe(rows.length);
  });
});
