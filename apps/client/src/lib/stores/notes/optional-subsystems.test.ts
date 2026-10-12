import { describe, expect, it, vi } from "vitest";
import { createNotesOptionalSubsystemController } from "./optional-subsystems";

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve = () => {};
  const promise = new Promise<void>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe("Notes optional subsystem controller", () => {
  it("single-flights a page subsystem and does not cache a stale page result", async () => {
    let generation = 1;
    let pageId: string | null = "page-a";
    const first = deferred();
    const second = deferred();
    const load = vi.fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const controller = createNotesOptionalSubsystemController({
      readPageGeneration: () => generation,
      readSelectedPageId: () => pageId,
      readSelectedProjectId: () => null,
      load,
    });

    const initial = controller.ensure("comments", pageId);
    const duplicate = controller.ensure("comments", pageId);
    expect(load).toHaveBeenCalledTimes(1);

    generation = 2;
    pageId = "page-b";
    first.resolve();
    await Promise.all([initial, duplicate]);

    const current = controller.ensure("comments", pageId);
    expect(load).toHaveBeenCalledTimes(2);
    second.resolve();
    await current;
    await controller.ensure("comments", pageId);
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("keeps project-scoped destination loads separate", async () => {
    let projectId: string | null = "project-a";
    const load = vi.fn().mockResolvedValue(undefined);
    const controller = createNotesOptionalSubsystemController({
      readPageGeneration: () => 1,
      readSelectedPageId: () => null,
      readSelectedProjectId: () => projectId,
      load,
    });

    await controller.ensure("destinations");
    projectId = "project-b";
    await controller.ensure("destinations");

    expect(load).toHaveBeenCalledTimes(2);
  });
});
