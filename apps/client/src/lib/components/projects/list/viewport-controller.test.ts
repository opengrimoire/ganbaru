// @vitest-environment jsdom
import { tick } from "svelte";
import { describe, expect, it, vi } from "vitest";
import { moveProjectTaskListColumnResize, startProjectTaskListColumnResize, type ProjectTaskListColumnWidths } from "$lib/projects/list/view";
import { ProjectListViewportController } from "./viewport-controller.svelte";

/** Start a real resize gesture with a measured name column. */
function drag(controller: ProjectListViewportController, delta: number): void {
  controller.resizeGesture = moveProjectTaskListColumnResize(startProjectTaskListColumnResize({
    column: "name", pointerId: 1, startClientX: 100, startWidthRem: 24,
    rootFontSizePx: 16, widthsAtStart: controller.effectiveColumnWidths,
  }), 100 + delta);
}

/** Supply only the pointer identity read by the release handler. */
function release(): PointerEvent {
  const event = new MouseEvent("pointerup");
  Object.defineProperty(event, "pointerId", { value: 1 });
  return event as PointerEvent;
}

describe("Project column resize persistence", () => {
  it("keeps the released width visible until saved props arrive", async () => {
    let widths: ProjectTaskListColumnWidths = { name: 24 };
    let complete: () => void = () => {};
    const controller = new ProjectListViewportController({
      getColumnWidths: () => widths,
      getGridInput: (columnWidths) => ({ columns: [], columnWidths }),
      persistColumnWidths: (next) => new Promise<void>((resolve) => {
        complete = () => { widths = next; resolve(); };
      }),
    });
    drag(controller, 64);
    controller.finishResize(release(), true);
    await tick();
    expect(widths.name).toBe(24);
    expect(controller.effectiveColumnWidths.name).toBe(28);
    complete();
    await tick();
    await tick();
    expect(controller.effectiveColumnWidths.name).toBe(28);
    widths = { name: 30 };
    await vi.waitFor(() => expect(controller.effectiveColumnWidths.name).toBe(30));
  });

  it("restores saved widths on cancellation or failed persistence", async () => {
    const persist = vi.fn().mockRejectedValue(new Error("Width save failed"));
    const controller = new ProjectListViewportController({
      getColumnWidths: () => ({ name: 24 }),
      getGridInput: () => ({ columns: [] }),
      persistColumnWidths: persist,
    });
    drag(controller, 64);
    controller.finishResize(release(), false);
    expect(controller.effectiveColumnWidths.name).toBe(24);
    expect(persist).not.toHaveBeenCalled();
    drag(controller, 64);
    controller.finishResize(release(), true);
    expect(controller.effectiveColumnWidths.name).toBe(28);
    await tick();
    await tick();
    await vi.waitFor(() => expect(controller.effectiveColumnWidths.name).toBe(24));
    expect(controller.resizeError).toBe("Width save failed");
  });

  it("does not drop a newer preview when an earlier save finishes", async () => {
    const saves: Array<() => void> = [];
    let widths: ProjectTaskListColumnWidths = { name: 24 };
    const controller = new ProjectListViewportController({
      getColumnWidths: () => widths,
      getGridInput: () => ({ columns: [] }),
      persistColumnWidths: (next) => new Promise<void>((resolve) => {
        saves.push(() => { widths = next; resolve(); });
      }),
    });
    drag(controller, 64);
    controller.finishResize(release(), true);
    drag(controller, 128);
    controller.finishResize(release(), true);
    saves[0]();
    await tick();
    await tick();
    expect(controller.effectiveColumnWidths.name).toBe(32);
    saves[1]();
    await tick();
    await tick();
    expect(controller.effectiveColumnWidths.name).toBe(32);
  });
});
