import { describe, expect, it, vi } from "vitest";
import { createProjectSettingsReorderController } from "./reorder-controller.svelte";

interface Entry {
  id: string;
  fieldId: string;
}

function dragEvent(): DragEvent {
  return {
    preventDefault: vi.fn(),
    clientY: 5,
    dataTransfer: {
      effectAllowed: "uninitialized",
      dropEffect: "none",
      getData: () => "",
      setData: vi.fn(),
    },
  } as unknown as DragEvent;
}

describe("project settings reorder controller", () => {
  it("clears pending and drag state when persistence fails", async () => {
    const entries: Entry[] = [
      { id: "a", fieldId: "field-1" },
      { id: "b", fieldId: "field-1" },
    ];
    let error: string | null = null;
    const controller = createProjectSettingsReorderController({
      dataType: "application/test",
      getEntries: () => entries,
      moveEntry: async () => { throw new Error("write failed"); },
      setError: (value) => { error = value; },
      reorderFailedMessage: () => "reorder failed",
      saveFailedMessage: (message) => `save failed: ${message}`,
    });
    controller.start(dragEvent(), entries[0]);

    await controller.moveToIndex("a", 1);

    expect(controller.pending).toBe(false);
    expect(controller.draggedId).toBeNull();
    expect(error).toBe("save failed: write failed");
  });

  it("rejects option drops across fields before showing a marker", () => {
    const entries: Entry[] = [
      { id: "a", fieldId: "field-1" },
      { id: "b", fieldId: "field-2" },
    ];
    const controller = createProjectSettingsReorderController({
      dataType: "application/test",
      getEntries: () => entries,
      moveEntry: async () => undefined,
      setError: () => undefined,
      reorderFailedMessage: () => "reorder failed",
      saveFailedMessage: (message) => message,
      canDrop: (dragged, target) => dragged.fieldId === target.fieldId,
    });
    controller.start(dragEvent(), entries[0]);
    const event = dragEvent();
    const target = {
      getBoundingClientRect: () => ({ top: 0, height: 10 }),
    } as HTMLElement;

    controller.dragOver(event, entries[1], target);

    expect(event.preventDefault).not.toHaveBeenCalled();
    expect(controller.markerVisible("b", "after")).toBe(false);
  });

  it("clears drag markers when the owning settings panel switches projects", () => {
    const entries: Entry[] = [
      { id: "a", fieldId: "field-1" },
      { id: "b", fieldId: "field-1" },
    ];
    const controller = createProjectSettingsReorderController({
      dataType: "application/test",
      getEntries: () => entries,
      moveEntry: async () => undefined,
      setError: () => undefined,
      reorderFailedMessage: () => "reorder failed",
      saveFailedMessage: (message) => message,
    });
    controller.start(dragEvent(), entries[0]);
    controller.dragOver(dragEvent(), entries[1], {
      getBoundingClientRect: () => ({ top: 0, height: 10 }),
    } as HTMLElement);

    controller.clear();

    expect(controller.draggedId).toBeNull();
    expect(controller.markerVisible("b", "after")).toBe(false);
  });
});
