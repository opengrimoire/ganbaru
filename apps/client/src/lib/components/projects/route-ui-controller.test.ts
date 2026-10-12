// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { ProjectRouteUiController } from "./route-ui-controller.svelte";

describe("Project route UI controller", () => {
  it("defers a toolbar transition until dirty settings are discarded", () => {
    const controller = new ProjectRouteUiController({
      setActiveView: vi.fn(),
    });
    controller.toolbarPanel = "settings";
    controller.settingsDirty = true;

    controller.toggleToolbarPanel("filters");
    expect(controller.toolbarPanel).toBe("settings");
    expect(controller.discardConfirmOpen).toBe(true);

    controller.confirmDiscard();
    expect(controller.toolbarPanel).toBe("filters");
    expect(controller.settingsDirty).toBe(false);
  });

  it("opens a task draft without a task and swaps it for the created task", () => {
    const setActiveView = vi.fn();
    const controller = new ProjectRouteUiController({ setActiveView });
    controller.selectedTaskId = "previous";
    controller.toolbarPanel = "filters";

    controller.openTaskDraft("project-1");
    expect(controller.selectedTaskId).toBeNull();
    expect(controller.taskDraftProjectId).toBe("project-1");
    expect(controller.toolbarPanel).toBeNull();

    const shortcut = new KeyboardEvent("keydown", { key: "2", cancelable: true });
    controller.handleWindowKeydown(shortcut);
    expect(setActiveView).not.toHaveBeenCalled();

    controller.completeTaskDraft({ id: "created" } as Parameters<typeof controller.completeTaskDraft>[0]);
    expect(controller.taskDraftProjectId).toBeNull();
    expect(controller.selectedTaskId).toBe("created");

    controller.closeTaskDetail();
    expect(controller.selectedTaskId).toBeNull();
  });

  it("waits for dirty settings to be discarded before opening a task draft", () => {
    const controller = new ProjectRouteUiController({ setActiveView: vi.fn() });
    controller.toolbarPanel = "settings";
    controller.settingsDirty = true;

    controller.openTaskDraft("project-1");
    expect(controller.taskDraftProjectId).toBeNull();

    controller.confirmDiscard();
    expect(controller.taskDraftProjectId).toBe("project-1");
  });

  it("suppresses route shortcuts while a document selection surface is open", () => {
    const setActiveView = vi.fn();
    const controller = new ProjectRouteUiController({
      setActiveView,
    });
    controller.taskFinderOpen = true;
    const event = new KeyboardEvent("keydown", { key: "3", cancelable: true });

    expect(controller.handleViewShortcut(event)).toBe(false);
    expect(setActiveView).not.toHaveBeenCalled();
  });

  it("clears non-editable selections inside the route without touching inputs", () => {
    const controller = new ProjectRouteUiController({
      setActiveView: vi.fn(),
    });
    const root = document.createElement("div");
    const text = document.createTextNode("selected");
    root.append(text);
    document.body.append(root);
    controller.rootElement = root;
    const range = document.createRange();
    range.selectNodeContents(text);
    document.getSelection()?.addRange(range);

    controller.handleDocumentSelectionChange();
    expect(document.getSelection()?.isCollapsed).toBe(true);
    root.remove();
  });
});
