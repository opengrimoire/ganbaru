// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
import MusicItemRepairDialog from "./MusicItemRepairDialog.svelte";

describe("MusicItemRepairDialog", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("clears an undo failure once a retried undo succeeds", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const undoItemRepair = vi.fn<() => Promise<void>>()
      .mockRejectedValueOnce(new Error("undo failed"))
      .mockResolvedValueOnce(undefined);
    const controller = {
      itemRepairApplied: { locationId: "location", rootId: "root" },
      itemRepairPreview: null,
      undoItemRepair,
    } as unknown as MusicSourcesController;
    component = mount(MusicItemRepairDialog, {
      target,
      props: { controller, itemId: "item", onClose: vi.fn(), onRepaired: vi.fn() },
    });
    await tick();

    const dialog = document.body.querySelector<HTMLElement>("[role='dialog']");
    const undoButton = [...(dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [])]
      .find((button) => button.textContent?.includes("Undo repair"));
    if (!undoButton) throw new Error("missing undo button");

    undoButton.click();
    await vi.waitFor(() => expect(dialog?.textContent).toContain("could not be repaired"));

    undoButton.click();
    await vi.waitFor(() => expect(undoItemRepair).toHaveBeenCalledTimes(2));
    await tick();
    expect(dialog?.textContent).not.toContain("could not be repaired");
  });
});
