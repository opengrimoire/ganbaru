import { describe, expect, it } from "vitest";
import type { View } from "$lib/navigation";
import type { WorkspacePaneScope } from "./model";
import { workspaceShownPanes, type WorkspaceShownPaneSources } from "./shown-panes";

function sources(
  secondaryTabId: string | null,
  hosted: Partial<Record<WorkspacePaneScope, View>>,
): WorkspaceShownPaneSources {
  return {
    scopeOf: (tabId) => (tabId === secondaryTabId ? "secondary" : "primary"),
    hostedViewIn: (scope) => hosted[scope] ?? null,
  };
}

describe("workspaceShownPanes", () => {
  it("shows a lone tab through the primary stores, hosting the workspace its Projects store opened", () => {
    expect(workspaceShownPanes({ id: "a", kind: "projects" }, null, false, sources(null, { primary: "notes" }))).toEqual([
      { tabId: "a", view: "notes", scope: "primary", hosted: true },
    ]);
    expect(workspaceShownPanes({ id: "a", kind: "projects" }, null, false, sources(null, {}))).toEqual([
      { tabId: "a", view: "projects", scope: "primary", hosted: false },
    ]);
  });

  it("shows each of two Projects tabs with the workspace its own pane hosts", () => {
    const panes = workspaceShownPanes(
      { id: "a", kind: "projects" },
      { id: "b", kind: "projects" },
      false,
      sources("b", { primary: "notes", secondary: "chat" }),
    );
    expect(panes).toEqual([
      { tabId: "a", view: "notes", scope: "primary", hosted: true },
      { tabId: "b", view: "chat", scope: "secondary", hosted: true },
    ]);
  });

  it("keeps a Projects pane on a task view while the pane beside it hosts a workspace", () => {
    const panes = workspaceShownPanes(
      { id: "b", kind: "projects" },
      { id: "a", kind: "projects" },
      true,
      sources("b", { primary: "chat" }),
    );
    expect(panes).toEqual([
      { tabId: "a", view: "chat", scope: "primary", hosted: true },
      { tabId: "b", view: "projects", scope: "secondary", hosted: false },
    ]);
  });

  it("shows a dedicated tab without the project sidebar beside a Projects tab hosting the same view", () => {
    const panes = workspaceShownPanes(
      { id: "n", kind: "notes" },
      { id: "p", kind: "projects" },
      false,
      sources("p", { primary: "notes", secondary: "notes" }),
    );
    expect(panes).toEqual([
      { tabId: "n", view: "notes", scope: "primary", hosted: false },
      { tabId: "p", view: "notes", scope: "secondary", hosted: true },
    ]);
  });

  it("shows one pane when both tabs would show the same view through the same stores", () => {
    const panes = workspaceShownPanes(
      { id: "c", kind: "calendar" },
      { id: "p", kind: "projects" },
      false,
      sources(null, { primary: "calendar" }),
    );
    expect(panes).toEqual([{ tabId: "c", view: "calendar", scope: "primary", hosted: false }]);
  });
});
