// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MusicSourceCollection } from "$lib/music/library/contracts";
import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
import MusicAddSourceDialog from "./MusicAddSourceDialog.svelte";
import MusicNetworkRefreshDialog from "./MusicNetworkRefreshDialog.svelte";
import MusicSourceRemovalDialog from "./MusicSourceRemovalDialog.svelte";

describe("MusicBuilderDialog", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("covers the app viewport with the standard backdrop", async () => {
    target = document.createElement("div");
    target.style.overflow = "hidden";
    document.body.append(target);
    component = mount(MusicNetworkRefreshDialog, {
      target,
      props: {
        onlineCount: 2,
        onLocalOnly: vi.fn(),
        onContinue: vi.fn(),
        onClose: vi.fn(),
      },
    });
    await tick();

    const dialog = document.body.querySelector<HTMLElement>("[role='dialog']");
    const layer = dialog?.parentElement;
    const backdrop = layer?.firstElementChild;

    expect(layer?.parentElement).toBe(document.body);
    expect(layer?.classList.contains("fixed")).toBe(true);
    expect(layer?.style.width).toContain("--visual-viewport-width");
    expect(backdrop?.classList.contains("surface-backdrop")).toBe(true);
    expect(layer?.classList.contains("backdrop-blur-sm")).toBe(false);
    expect(dialog?.classList.contains("shadow-2xl")).toBe(false);
    expect(dialog?.classList.contains("surface-dialog")).toBe(true);
    expect(dialog?.querySelector("[title]")).toBeNull();
  });

  it("dismisses from the backdrop without treating dialog clicks as dismissals", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const onClose = vi.fn();
    component = mount(MusicNetworkRefreshDialog, {
      target,
      props: {
        onlineCount: 1,
        onLocalOnly: vi.fn(),
        onContinue: vi.fn(),
        onClose,
      },
    });
    await tick();

    const dialog = document.body.querySelector<HTMLElement>("[role='dialog']");
    dialog?.click();
    expect(onClose).not.toHaveBeenCalled();

    dialog?.parentElement?.click();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("starts YouTube setup at the required field without redundant guidance", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const controller = {
      busy: false,
      resolving: false,
      cancelResolution: vi.fn(),
    } as unknown as MusicSourcesController;
    component = mount(MusicAddSourceDialog, {
      target,
      props: {
        controller,
        kind: "youtube",
        onClose: vi.fn(),
        onSaved: vi.fn(),
      },
    });
    await tick();

    const dialog = document.body.querySelector<HTMLElement>("[role='dialog']");
    const input = dialog?.querySelector<HTMLInputElement>(".source-input");
    const previewButton = [...(dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [])]
      .find((button) => button.textContent?.includes("Preview"));

    expect(dialog?.textContent).toContain("YouTube");
    expect(dialog?.textContent).not.toContain("What would you like to add?");
    expect(dialog?.textContent).not.toContain("contacts YouTube");
    expect(dialog?.textContent).not.toContain("Add to library");
    expect(input?.classList.contains("h-11")).toBe(true);
    expect(previewButton?.classList.contains("h-11")).toBe(true);
  });

  it("presents source removal as one concise choice with an exact action", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const controller = {
      removalImpact: {
        collectionId: "source-1",
        itemCount: 1_152,
        membershipCount: 18,
        sharedItemCount: 17,
        orphanedItemCount: 1_135,
        activeRefreshCount: 0,
      },
      forgetBinding: vi.fn(async () => undefined),
      confirmRemoval: vi.fn(async () => undefined),
    } as unknown as MusicSourcesController;
    const collection: MusicSourceCollection = {
      id: "source-1",
      kind: "local-root",
      identityKey: "local-root:source-1",
      name: "Music",
      localRootId: "root-1",
      youtubePlaylistId: null,
      refreshState: "idle",
      lastSuccessfulRefreshAtMs: null,
      previousSuccessfulRefreshAtMs: null,
      lastRefreshErrorCode: null,
      snapshotGeneration: 1,
      createdAtMs: 1,
      updatedAtMs: 1,
      version: 1,
      discoveryEnabled: true,
      removedAtMs: null,
    };
    component = mount(MusicSourceRemovalDialog, {
      target,
      props: {
        controller,
        collection,
        onClose: vi.fn(),
        onRemoved: vi.fn(),
      },
    });
    await tick();

    const dialog = document.body.querySelector<HTMLElement>("[role='alertdialog']");
    const radios = dialog?.querySelectorAll<HTMLInputElement>("input[type='radio']");
    const action = [...(dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [])].at(-1);

    expect(dialog?.textContent).not.toContain("least destructive");
    expect(dialog?.textContent).not.toContain("affected items");
    expect(action?.textContent).toBe("Disconnect this device");
    expect(action?.classList.contains("primary-action")).toBe(true);

    radios?.[1]?.click();
    await tick();

    expect(dialog?.textContent).toContain("Future scans stop");
    expect(action?.textContent).toBe("Remove source");
    expect(action?.classList.contains("destructive-action")).toBe(true);
  });
});
