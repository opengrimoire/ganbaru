import { describe, expect, it } from "vitest";
import {
  ancestorsOf,
  descendantsOf,
  isSpaceSelected,
  spaceSelectionState,
  summarizeSpaceSelection,
  toggleSpaceSelection,
  type InvitationSpaceNode,
} from "./space-selection";

const tree: readonly InvitationSpaceNode[] = [
  { id: "g1", kind: "group", name: "Work", parentId: null },
  { id: "p1", kind: "project", name: "Website", parentId: "g1" },
  { id: "c1", kind: "channel", name: "general", parentId: "p1" },
  { id: "c2", kind: "channel", name: "design", parentId: "p1" },
  { id: "p2", kind: "project", name: "Launch", parentId: "g1" },
  { id: "c3", kind: "channel", name: "general", parentId: "p2" },
  { id: "p3", kind: "project", name: "Solo", parentId: null },
];

describe("space selection tree", () => {
  it("walks ancestors from the nearest parent and descendants depth first", () => {
    expect(ancestorsOf(tree, "c2").map((node) => node.id)).toEqual(["p1", "g1"]);
    expect(descendantsOf(tree, "g1").map((node) => node.id)).toEqual(["p1", "c1", "c2", "p2", "c3"]);
    expect(ancestorsOf(tree, "p3")).toEqual([]);
  });

  it("treats a selected ancestor as covering every descendant", () => {
    const selected = new Set(["g1"]);
    expect(isSpaceSelected(tree, selected, "c3")).toBe(true);
    expect(isSpaceSelected(tree, selected, "p3")).toBe(false);
    expect(spaceSelectionState(tree, selected, "p1")).toBe("all");
  });

  it("reports a partial parent when only some children are selected", () => {
    const selected = new Set(["c1"]);
    expect(spaceSelectionState(tree, selected, "p1")).toBe("some");
    expect(spaceSelectionState(tree, selected, "g1")).toBe("some");
    expect(spaceSelectionState(tree, selected, "p2")).toBe("none");
  });

  it("collapses a parent to all when every child is selected", () => {
    expect(spaceSelectionState(tree, new Set(["c1", "c2"]), "p1")).toBe("all");
    expect(spaceSelectionState(tree, new Set(["p1", "p2"]), "g1")).toBe("all");
  });

  it("selecting a parent drops its already selected descendants", () => {
    expect([...toggleSpaceSelection(tree, new Set(["c1", "p2"]), "g1", true)]).toEqual(["g1"]);
  });

  it("deselecting inside a selected ancestor keeps the siblings selected", () => {
    const next = toggleSpaceSelection(tree, new Set(["g1"]), "c1", false);
    expect([...next].sort()).toEqual(["c2", "p2"]);
    expect(spaceSelectionState(tree, next, "c1")).toBe("none");
    expect(spaceSelectionState(tree, next, "p2")).toBe("all");
  });

  it("deselecting a node also clears its descendants", () => {
    expect(toggleSpaceSelection(tree, new Set(["c1", "c3"]), "p1", false)).toEqual(new Set(["c3"]));
  });

  it("summarizes selected nodes without listing their children", () => {
    const summary = summarizeSpaceSelection(tree, new Set(["p1", "c3", "p3"]));
    expect(summary.map((entry) => [entry.node.id, entry.whole, entry.childCount])).toEqual([["g1", false, 2], ["p3", true, 0]]);
    const group = summary[0];
    expect(group?.children.map((entry) => [entry.node.id, entry.whole])).toEqual([["p1", true], ["p2", false]]);
    expect(group?.children[0]?.children).toEqual([]);
    expect(group?.children[1]?.children.map((entry) => entry.node.id)).toEqual(["c3"]);
  });

  it("keeps a group partial in the summary when its projects were chosen one by one", () => {
    const summary = summarizeSpaceSelection(tree, new Set(["p1", "p2"]));
    expect(summary[0]?.whole).toBe(false);
    expect(summary[0]?.children.map((entry) => entry.node.id)).toEqual(["p1", "p2"]);
  });

  it("lists channels under a partially selected project", () => {
    const summary = summarizeSpaceSelection(tree, new Set(["c2"]));
    expect(summary[0]?.children[0]?.children.map((entry) => entry.node.id)).toEqual(["c2"]);
  });
});
