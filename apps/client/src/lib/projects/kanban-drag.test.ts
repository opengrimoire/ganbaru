import { describe, expect, it } from "vitest";
import { projectKanbanDropPlacement } from "./kanban-drag";

function tasks(...ids: string[]): { id: string }[] {
  return ids.map((id) => ({ id }));
}

describe("projectKanbanDropPlacement", () => {
  it("places a card after its visual predecessor in ascending order", () => {
    expect(projectKanbanDropPlacement({
      orderedTasks: tasks("a", "b", "c"),
      draggedTaskId: "dragged",
      overTaskId: "b",
      position: "after",
      sortDirection: "asc",
    })).toEqual({ kind: "after", id: "b" });
  });

  it("places a card before the first visual card in ascending order", () => {
    expect(projectKanbanDropPlacement({
      orderedTasks: tasks("a", "b"),
      draggedTaskId: "dragged",
      overTaskId: "a",
      position: "before",
      sortDirection: "asc",
    })).toEqual({ kind: "before", id: "a" });
  });

  it("appends a card in ascending order when no target card is supplied", () => {
    expect(projectKanbanDropPlacement({
      orderedTasks: tasks("a", "b"),
      draggedTaskId: "dragged",
      sortDirection: "asc",
    })).toEqual({ kind: "after", id: "b" });
  });

  it("appends to an empty column", () => {
    expect(projectKanbanDropPlacement({ orderedTasks: [], draggedTaskId: "dragged", sortDirection: "asc" })).toEqual({ kind: "end" });
  });

  it("swaps native before and after in descending order", () => {
    expect(projectKanbanDropPlacement({
      orderedTasks: tasks("c", "b", "a"),
      draggedTaskId: "dragged",
      overTaskId: "b",
      position: "after",
      sortDirection: "desc",
    })).toEqual({ kind: "before", id: "b" });
    expect(projectKanbanDropPlacement({
      orderedTasks: tasks("b", "a"),
      draggedTaskId: "dragged",
      overTaskId: "b",
      position: "before",
      sortDirection: "desc",
    })).toEqual({ kind: "after", id: "b" });
  });

  it("ignores the dragged card when calculating a same-column insertion", () => {
    expect(projectKanbanDropPlacement({
      orderedTasks: tasks("a", "dragged", "b"),
      draggedTaskId: "dragged",
      overTaskId: "b",
      position: "after",
      sortDirection: "asc",
    })).toEqual({ kind: "after", id: "b" });
  });

  it("returns null when a card is dropped back into its own slot", () => {
    expect(projectKanbanDropPlacement({
      orderedTasks: tasks("a", "dragged", "b"),
      draggedTaskId: "dragged",
      overTaskId: "a",
      position: "after",
      sortDirection: "asc",
    })).toBeNull();
  });
});
