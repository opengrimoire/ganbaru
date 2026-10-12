import { describe, expect, it } from "vitest";
import {
  projectSettingsDraggedEntryDropTarget,
  projectSettingsDraggedEntryId,
  projectSettingsDragOverPlan,
  projectSettingsEntryDropTargetIndex,
  moveProjectSettingsEntryToIndex,
  projectSettingsDropMarkerVisible,
  projectSettingsDropPosition,
  projectSettingsDropTargetIndex,
  projectSettingsStartDrag,
  type ProjectSettingsDragDataTransfer,
} from "./reorder";

interface ReorderEntry {
  id: string;
}

function entry(id: string): ReorderEntry {
  return { id };
}

class FakeDataTransfer implements ProjectSettingsDragDataTransfer {
  effectAllowed: DataTransfer["effectAllowed"] = "uninitialized";
  dropEffect: DataTransfer["dropEffect"] = "none";
  private values = new Map<string, string>();

  getData(type: string): string {
    return this.values.get(type) ?? "";
  }

  setData(type: string, data: string): void {
    this.values.set(type, data);
  }
}

describe("projectSettingsDropPosition", () => {
  it("uses the upper half as before", () => {
    expect(projectSettingsDropPosition(14, 10, 10)).toBe("before");
  });

  it("uses the midpoint and lower half as after", () => {
    expect(projectSettingsDropPosition(15, 10, 10)).toBe("after");
    expect(projectSettingsDropPosition(18, 10, 10)).toBe("after");
  });
});

describe("projectSettingsDropTargetIndex", () => {
  it("moves an earlier item after a later target", () => {
    expect(projectSettingsDropTargetIndex({
      sourceIndex: 0,
      targetIndex: 2,
      position: "after",
      itemCount: 4,
    })).toBe(2);
  });

  it("moves a later item before an earlier target", () => {
    expect(projectSettingsDropTargetIndex({
      sourceIndex: 3,
      targetIndex: 1,
      position: "before",
      itemCount: 4,
    })).toBe(1);
  });

  it("returns null when the resolved position is unchanged", () => {
    expect(projectSettingsDropTargetIndex({
      sourceIndex: 1,
      targetIndex: 2,
      position: "before",
      itemCount: 4,
    })).toBeNull();
  });

  it("rejects missing indices", () => {
    expect(projectSettingsDropTargetIndex({
      sourceIndex: -1,
      targetIndex: 2,
      position: "after",
      itemCount: 4,
    })).toBeNull();
  });
});

describe("projectSettingsDropMarkerVisible", () => {
  it("shows only for the active target and position", () => {
    expect(projectSettingsDropMarkerVisible({
      draggedId: "a",
      targetId: "b",
      overId: "b",
      currentPosition: "after",
      markerPosition: "after",
    })).toBe(true);
  });

  it("hides for the dragged row itself", () => {
    expect(projectSettingsDropMarkerVisible({
      draggedId: "a",
      targetId: "a",
      overId: "a",
      currentPosition: "after",
      markerPosition: "after",
    })).toBe(false);
  });
});

describe("projectSettingsStartDrag", () => {
  it("stores typed and plain drag data", () => {
    const dataTransfer = new FakeDataTransfer();

    projectSettingsStartDrag({ preventDefault: () => undefined, dataTransfer }, "application/test", "row-a");

    expect(dataTransfer.effectAllowed).toBe("move");
    expect(dataTransfer.getData("application/test")).toBe("row-a");
    expect(dataTransfer.getData("text/plain")).toBe("row-a");
  });
});

describe("projectSettingsDraggedEntryId", () => {
  it("prefers the active dragged id", () => {
    const dataTransfer = new FakeDataTransfer();
    dataTransfer.setData("application/test", "row-a");

    expect(projectSettingsDraggedEntryId(
      { preventDefault: () => undefined, dataTransfer },
      "application/test",
      "active-row",
    )).toBe("active-row");
  });

  it("falls back to typed drag data", () => {
    const dataTransfer = new FakeDataTransfer();
    dataTransfer.setData("application/test", "row-a");

    expect(projectSettingsDraggedEntryId(
      { preventDefault: () => undefined, dataTransfer },
      "application/test",
      null,
    )).toBe("row-a");
  });
});

describe("projectSettingsDragOverPlan", () => {
  it("returns no plan when dragging is inactive, pending, or disallowed", () => {
    expect(projectSettingsDragOverPlan({
      draggedId: null,
      targetId: "b",
      reorderPending: false,
      position: "after",
    })).toBeNull();
    expect(projectSettingsDragOverPlan({
      draggedId: "a",
      targetId: "b",
      reorderPending: true,
      position: "after",
    })).toBeNull();
    expect(projectSettingsDragOverPlan({
      draggedId: "a",
      targetId: "b",
      reorderPending: false,
      position: "after",
      dropAllowed: false,
    })).toBeNull();
  });

  it("clears the target when hovering over the dragged entry", () => {
    expect(projectSettingsDragOverPlan({
      draggedId: "a",
      targetId: "a",
      reorderPending: false,
      position: "after",
    })).toEqual({
      preventDefault: false,
      overId: null,
      position: null,
    });
  });

  it("targets a valid row for dropping", () => {
    expect(projectSettingsDragOverPlan({
      draggedId: "a",
      targetId: "b",
      reorderPending: false,
      position: "after",
    })).toEqual({
      preventDefault: true,
      overId: "b",
      position: "after",
    });
  });
});

describe("projectSettingsEntryDropTargetIndex", () => {
  it("resolves a target index from entry ids", () => {
    expect(projectSettingsEntryDropTargetIndex({
      entries: [entry("a"), entry("b"), entry("c")],
      sourceId: "a",
      targetId: "c",
      position: "after",
    })).toBe(2);
  });

  it("rejects a drop onto the dragged entry", () => {
    expect(projectSettingsEntryDropTargetIndex({
      entries: [entry("a"), entry("b")],
      sourceId: "a",
      targetId: "a",
      position: "before",
    })).toBeNull();
  });
});

describe("projectSettingsDraggedEntryDropTarget", () => {
  it("resolves a dragged entry and target index", () => {
    const dataTransfer = new FakeDataTransfer();
    dataTransfer.setData("application/test", "a");

    expect(projectSettingsDraggedEntryDropTarget({
      event: { preventDefault: () => undefined, dataTransfer },
      dataType: "application/test",
      activeDraggedId: null,
      entries: [entry("a"), entry("b"), entry("c")],
      targetId: "c",
      position: "after",
    })).toEqual({ entryId: "a", targetIndex: 2 });
  });

  it("returns null when the drop target is unchanged", () => {
    expect(projectSettingsDraggedEntryDropTarget({
      event: { preventDefault: () => undefined },
      dataType: "application/test",
      activeDraggedId: "a",
      entries: [entry("a"), entry("b")],
      targetId: "a",
      position: "before",
    })).toBeNull();
  });
});

describe("moveProjectSettingsEntryToIndex", () => {
  it("moves entries by repeatedly calling the adjacent move operation", async () => {
    let entries = [entry("a"), entry("b"), entry("c"), entry("d")];
    const moved: string[] = [];

    const result = await moveProjectSettingsEntryToIndex({
      entryId: "a",
      targetIndex: 2,
      getEntries: () => entries,
      moveEntry: async (currentEntry, direction) => {
        moved.push(`${currentEntry.id}:${direction}`);
        const index = entries.findIndex((candidate) => candidate.id === currentEntry.id);
        const target = entries[index + direction];
        if (!target) return;
        const nextEntries = [...entries];
        nextEntries[index] = target;
        nextEntries[index + direction] = currentEntry;
        entries = nextEntries;
      },
    });

    expect(result).toBe(true);
    expect(entries.map((currentEntry) => currentEntry.id)).toEqual(["b", "c", "a", "d"]);
    expect(moved).toEqual(["a:1", "a:1"]);
  });
});
