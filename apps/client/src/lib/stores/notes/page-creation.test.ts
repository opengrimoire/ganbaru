import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NotesLoadedPage, NotesPageCreate } from "$lib/notes/types";

const backend = vi.hoisted(() => ({
  createNotesPage: vi.fn<(request: NotesPageCreate) => Promise<NotesLoadedPage>>(),
}));

vi.mock("$lib/api/notes", () => ({
  createNotesPage: backend.createNotesPage,
}));

const request: NotesPageCreate = {
  id: "11111111-1111-4111-8111-111111111111",
  title: "",
  parent: { type: "workspace", workspace: true },
  folder_id: null,
  first_block_id: "22222222-2222-4222-8222-222222222222",
};

const loaded = { page: { id: request.id } } as NotesLoadedPage;

describe("Notes page creation controller", () => {
  beforeEach(() => backend.createNotesPage.mockReset());

  it("gates mutations until creation reconciles", async () => {
    let resolveCreate: (value: NotesLoadedPage) => void = () => {};
    backend.createNotesPage.mockReturnValue(new Promise((resolve) => {
      resolveCreate = resolve;
    }));
    const reconcile = vi.fn<() => Promise<void>>().mockResolvedValue();
    const { createNotesPageCreationController } = await import("./page-creation.svelte");
    const controller = createNotesPageCreationController({ reconcile });

    controller.begin(request, loaded);
    const ready = controller.awaitReady(request.id);
    expect(controller.isPending(request.id)).toBe(true);
    expect(reconcile).not.toHaveBeenCalled();

    resolveCreate(loaded);
    await ready;

    expect(reconcile).toHaveBeenCalledWith(loaded);
    expect(controller.isPending(request.id)).toBe(false);
  });

  it("releases buffered mutations after persistence completes", async () => {
    backend.createNotesPage.mockResolvedValue(loaded);
    const afterPersisted = vi.fn<() => Promise<void>>().mockResolvedValue();
    const { createNotesPageCreationController } = await import("./page-creation.svelte");
    const controller = createNotesPageCreationController({
      reconcile: () => Promise.resolve(),
      afterPersisted,
    });

    controller.begin(request, loaded);
    await vi.waitFor(() => expect(afterPersisted).toHaveBeenCalledWith(request.id));
  });

  it("keeps a failed draft and retries the same request", async () => {
    backend.createNotesPage
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValueOnce(loaded);
    const reconcile = vi.fn<() => Promise<void>>().mockResolvedValue();
    const { createNotesPageCreationController } = await import("./page-creation.svelte");
    const controller = createNotesPageCreationController({ reconcile });

    controller.begin(request, loaded);
    let mutationReleased = false;
    const bufferedMutation = controller.awaitReady(request.id).then(() => {
      mutationReleased = true;
    });
    await vi.waitFor(() => expect(controller.errorFor(request.id)).toBe("disk full"));
    expect(controller.isPending(request.id)).toBe(true);
    expect(mutationReleased).toBe(false);

    controller.retry(request.id);
    await vi.waitFor(() => expect(controller.isPending(request.id)).toBe(false));
    await bufferedMutation;

    expect(backend.createNotesPage).toHaveBeenNthCalledWith(2, request);
    expect(reconcile).toHaveBeenCalledWith(loaded);
    expect(mutationReleased).toBe(true);
  });

  it("settles a failed creation attempt and releases blocked edits when the draft is discarded", async () => {
    backend.createNotesPage.mockRejectedValueOnce(new Error("disk full"));
    const { createNotesPageCreationController } = await import("./page-creation.svelte");
    const controller = createNotesPageCreationController({ reconcile: () => Promise.resolve() });

    controller.begin(request, loaded);
    const blockedEdit = controller.awaitReady(request.id);
    expect(await controller.awaitAttempt(request.id)).toBe("failed");
    expect(controller.errorFor(request.id)).toBe("disk full");

    controller.discardFailed(request.id);
    await expect(blockedEdit).rejects.toThrow("notes page was discarded");
    expect(controller.isPending(request.id)).toBe(false);
    expect(controller.errorFor(request.id)).toBeNull();
  });

  it("reuses a clean new page once and invalidates the preview after editing", async () => {
    backend.createNotesPage.mockResolvedValue(loaded);
    const { createNotesPageCreationController } = await import("./page-creation.svelte");
    const first = createNotesPageCreationController({ reconcile: () => Promise.resolve() });
    first.begin(request, loaded);
    expect(first.previewForSelection(request.id)).toEqual({ loaded, pending: true });
    expect(await first.awaitAttempt(request.id)).toBe("ready");
    expect(first.previewForSelection(request.id)).toEqual({ loaded, pending: false });
    expect(first.previewForSelection(request.id)).toBeNull();

    const edited = createNotesPageCreationController({ reconcile: () => Promise.resolve() });
    edited.begin(request, loaded);
    edited.markChanged(request.id);
    expect(edited.previewForSelection(request.id)).toEqual({ loaded, pending: true });
    expect(await edited.awaitAttempt(request.id)).toBe("ready");
    expect(edited.previewForSelection(request.id)).toBeNull();
  });
});
