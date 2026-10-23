import { describe, expect, it } from "vitest";
import type { LocalRootBinding } from "$lib/music/library/contracts";
import { itemBindingProblem } from "./binding-status";

const bindings: LocalRootBinding[] = [
  { rootId: "bound", folderPath: "/music", status: "available" },
  { rootId: "remote", folderPath: null, status: "unbound" },
  { rootId: "moved", folderPath: "/gone", status: "missing" },
];

describe("item binding problem", () => {
  it("reports a root that has no folder on this device as unbound", () => {
    expect(itemBindingProblem({ sourceKind: "local-file", localRootId: "remote" }, bindings)).toBe("unbound");
  });

  it("keeps a bound root whose folder disappeared distinct from an unbound one", () => {
    expect(itemBindingProblem({ sourceKind: "local-file", localRootId: "moved" }, bindings)).toBe("missing");
  });

  it("reports nothing for available roots, unread roots, YouTube items, and items without a root", () => {
    expect(itemBindingProblem({ sourceKind: "local-file", localRootId: "bound" }, bindings)).toBeNull();
    expect(itemBindingProblem({ sourceKind: "local-file", localRootId: "unread" }, bindings)).toBeNull();
    expect(itemBindingProblem({ sourceKind: "youtube-video", localRootId: "remote" }, bindings)).toBeNull();
    expect(itemBindingProblem({ sourceKind: "local-file", localRootId: null }, bindings)).toBeNull();
  });
});
