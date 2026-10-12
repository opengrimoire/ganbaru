import { describe, expect, it } from "vitest";
import {
  buildChangedFileTree,
  CHAT_WORKSPACE_PANEL_TAB_NAME_MAX_LENGTH,
  ChatInspectorSessionState,
  closeInspectorTab,
  inspectorFocusAction,
  inspectorSessionKey,
  moveWorkspacePanelTab,
  normalizeWorkspacePanelTabName,
  openInspectorTab,
  reconcileWorkspacePanelTabOrder,
  splitPaneResizeBounds,
  splitDiffFits,
  terminalWorkspacePanelDefaultLabel,
  terminalWorkspacePanelTabKey,
  workspacePanelKinds,
  workspacePanelRenameGeometry,
  workspacePanelTabInsertionIndex,
  workspacePanelTabShift,
  workspacePanelTerminalId,
} from "./inspector-model";

describe("Chat inspector model", () => {
  it("preserves independent per-thread session state", () => {
    const state = new ChatInspectorSessionState();
    state.update("thread-a", { tab: "terminal", selectedFile: "src/a.ts" });
    state.update("thread-b", { tab: "files" });
    expect(state.read("thread-a").tab).toBe("terminal");
    expect(state.read("thread-a").selectedFile).toBe("src/a.ts");
    expect(state.read("thread-b").tab).toBe("files");
  });

  it("starts the inspector with the file browser and adds each panel once", () => {
    const state = new ChatInspectorSessionState();
    expect(state.read("thread-a").tab).toBe("files");
    expect(state.read("thread-a").openTabs).toEqual(["files"]);
    expect(state.read("thread-a").tabOrder).toEqual(["files"]);
    expect(state.read("thread-a").tabNames).toEqual({});
    expect(openInspectorTab(["files"], "review")).toEqual(["files", "review"]);
    expect(openInspectorTab(["files", "review"], "files")).toEqual(["files", "review"]);
  });

  it("supports a terminal-first bottom panel without inheriting the inspector default", () => {
    const state = new ChatInspectorSessionState("terminal");
    expect(state.read(null).tab).toBe("terminal");
    expect(state.read("thread-a").openTabs).toEqual(["terminal"]);
    expect(state.read("thread-a").tabOrder).toEqual(["terminal"]);
    expect(state.read("thread-a").fileTreeVisible).toBe(false);
  });

  it("keeps renamed tabs isolated by thread and protects stored state from mutation", () => {
    const state = new ChatInspectorSessionState();
    state.update("thread-a", { tabNames: { files: "Sources" } });
    const firstRead = state.read("thread-a");
    firstRead.tabNames.files = "Mutated";

    expect(state.read("thread-a").tabNames).toEqual({ files: "Sources" });
    expect(state.read("thread-b").tabNames).toEqual({});
  });

  it("bounds retained per-thread panel state", () => {
    const state = new ChatInspectorSessionState();
    for (let index = 0; index < 64; index += 1) {
      state.update(`thread-${index}`, { selectedFile: `src/${index}.ts` });
    }
    state.read("thread-0");
    state.update("thread-64", { selectedFile: "src/64.ts" });

    expect(state.read("thread-0").selectedFile).toBe("src/0.ts");
    expect(state.read("thread-1").selectedFile).toBeNull();
    expect(state.read("thread-64").selectedFile).toBe("src/64.ts");
  });

  it("uses the terminal path as its default tab name and bounds custom names", () => {
    expect(terminalWorkspacePanelDefaultLabel("alice@workstation: ~/Documents/ganbaru-ai"))
      .toBe("~/Documents/ganbaru-ai");
    expect(terminalWorkspacePanelDefaultLabel("alice@workstation: C:\\Code\\ganbaru-ai"))
      .toBe("C:\\Code\\ganbaru-ai");
    expect(terminalWorkspacePanelDefaultLabel("Terminal")).toBe("Terminal");
    expect(normalizeWorkspacePanelTabName("  Build logs  ")).toBe("Build logs");
    expect(normalizeWorkspacePanelTabName("   ")).toBeNull();
    expect(normalizeWorkspacePanelTabName("x".repeat(CHAT_WORKSPACE_PANEL_TAB_NAME_MAX_LENGTH + 5)))
      .toHaveLength(CHAT_WORKSPACE_PANEL_TAB_NAME_MAX_LENGTH);
  });

  it("keeps the tab rename panel inside the viewport", () => {
    expect(workspacePanelRenameGeometry(120, 80, 800, 600, 132)).toEqual({
      left: 120,
      top: 80,
      width: 240,
      maxHeight: 584,
    });
    expect(workspacePanelRenameGeometry(790, 590, 800, 600, 132)).toEqual({
      left: 552,
      top: 460,
      width: 240,
      maxHeight: 584,
    });
    expect(workspacePanelRenameGeometry(200, 120, 220, 180, 190)).toEqual({
      left: 8,
      top: 8,
      width: 204,
      maxHeight: 164,
    });
  });

  it("reconciles one physical order for terminal sessions and tool tabs", () => {
    expect(reconcileWorkspacePanelTabOrder(
      ["files", "terminal", "plan"],
      ["files", "terminal", "plan"],
      ["terminal-a", "terminal-b"],
    )).toEqual([
      "files",
      terminalWorkspacePanelTabKey("terminal-a"),
      terminalWorkspacePanelTabKey("terminal-b"),
      "plan",
    ]);

    expect(reconcileWorkspacePanelTabOrder(
      ["terminal:terminal-b", "files", "terminal:terminal-a", "review"],
      ["terminal", "files", "plan"],
      ["terminal-a", "terminal-b"],
    )).toEqual(["terminal:terminal-b", "files", "terminal:terminal-a", "plan"]);
  });

  it("moves tabs by insertion index and derives their panel families", () => {
    const order = ["terminal:one", "files", "plan"] as const;
    expect(moveWorkspacePanelTab(order, "plan", 0)).toEqual(["plan", "terminal:one", "files"]);
    expect(moveWorkspacePanelTab(order, "terminal:one", 2)).toEqual(["files", "plan", "terminal:one"]);
    expect(moveWorkspacePanelTab(order, "terminal:missing", 1)).toEqual(order);
    expect(workspacePanelKinds(["terminal:one", "files", "terminal:two", "plan"]))
      .toEqual(["terminal", "files", "plan"]);
    expect(workspacePanelTerminalId("terminal:one")).toBe("one");
    expect(workspacePanelTerminalId("terminal")).toBeNull();
  });

  it("uses symmetric drag geometry in both reorder directions", () => {
    expect(workspacePanelTabInsertionIndex(40, [50, 150, 250])).toBe(0);
    expect(workspacePanelTabInsertionIndex(175, [50, 150, 250])).toBe(2);
    expect(workspacePanelTabInsertionIndex(300, [50, 150, 250])).toBe(3);

    expect([0, 1, 2].map((index) => workspacePanelTabShift(index, 0, 2, 80)))
      .toEqual([0, -80, -80]);
    expect([0, 1, 2].map((index) => workspacePanelTabShift(index, 2, 0, 80)))
      .toEqual([80, 80, 0]);
  });

  it("preserves internal pane sizes independently per placement session", () => {
    const inspector = new ChatInspectorSessionState("files");
    const bottom = new ChatInspectorSessionState("terminal");
    inspector.update("thread-a", { fileTreeWidthPx: 280, changedFileListHeightPx: 190 });
    bottom.update("thread-a", { fileTreeWidthPx: 180, changedFileListHeightPx: 72 });
    expect(inspector.read("thread-a").fileTreeWidthPx).toBe(280);
    expect(inspector.read("thread-a").changedFileListHeightPx).toBe(190);
    expect(bottom.read("thread-a").fileTreeWidthPx).toBe(180);
    expect(bottom.read("thread-a").changedFileListHeightPx).toBe(72);
  });

  it("selects the nearest tab when a workspace panel closes", () => {
    expect(closeInspectorTab(["files", "review", "plan"], "review", "review")).toEqual({
      tabs: ["files", "plan"],
      selectedTab: "plan",
    });
    expect(closeInspectorTab(["files", "review"], "review", "files")).toEqual({
      tabs: ["files"],
      selectedTab: "files",
    });
    expect(closeInspectorTab(["files"], "files", "files")).toEqual({
      tabs: [],
      selectedTab: null,
    });
  });

  it("keeps inspector state available for a workspace draft", () => {
    expect(inspectorSessionKey(null, "workspace-a")).toBe("draft:workspace-a");
    expect(inspectorSessionKey("thread-a", "workspace-a")).toBe("thread-a");
    expect(inspectorSessionKey(null, null)).toBeNull();
  });

  it("derives a stable changed-file tree including renames", () => {
    const tree = buildChangedFileTree([
      {
        relativePath: "src/new.ts",
        previousRelativePath: "src/old.ts",
        status: "renamed",
        additions: 1,
        deletions: 1,
        binary: false,
        providerReported: true,
        gitObserved: true,
      },
      {
        relativePath: "README.md",
        previousRelativePath: null,
        status: "modified",
        additions: 2,
        deletions: 0,
        binary: false,
        providerReported: false,
        gitObserved: true,
      },
    ]);
    expect(tree.map((node) => node.name)).toEqual(["README.md", "src"]);
    expect(tree[1].children[0].file?.previousRelativePath).toBe("src/old.ts");
  });

  it("uses a fit-based split diff decision", () => {
    expect(splitDiffFits(900)).toBe(true);
    expect(splitDiffFits(700)).toBe(false);
    expect(splitDiffFits(900, 1.5)).toBe(false);
  });

  it("keeps internal split panes reachable as their container shrinks", () => {
    expect(splitPaneResizeBounds(800, 144, 120, 360)).toEqual({ minimum: 144, maximum: 360 });
    expect(splitPaneResizeBounds(240, 144, 120, 360)).toEqual({ minimum: 120, maximum: 120 });
    expect(splitPaneResizeBounds(105, 112, 64, 420)).toEqual({ minimum: 41, maximum: 41 });
    expect(splitPaneResizeBounds(40, 112, 64, 420)).toEqual({ minimum: 0, maximum: 0 });
  });

  it("enters and restores focus only across inspector visibility boundaries", () => {
    expect(inspectorFocusAction(false, true)).toBe("enter");
    expect(inspectorFocusAction(true, false)).toBe("restore");
    expect(inspectorFocusAction(true, true)).toBe("none");
  });
});
