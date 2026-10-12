// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatChangedFileRead } from "$lib/chat/contracts";
import ChatChangedFileTree from "./ChatChangedFileTree.svelte";

describe("ChatChangedFileTree", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("groups paths, exposes provider and Git sources, and keeps selection explicit", async () => {
    const onSelect = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatChangedFileTree, {
      target,
      props: {
        files: [changed("src/new.ts", true, true, "src/old.ts"), changed("README.md", false, true)],
        selectedFile: "src/new.ts",
        onSelect,
      },
    });
    await tick();

    expect([...target.querySelectorAll("button")].map((button) => button.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining("README.md"), expect.stringContaining("src"), expect.stringContaining("new.ts")]),
    );
    const selected = target.querySelector<HTMLButtonElement>("button.selected");
    expect(selected?.textContent).toContain("src/old.ts");
    expect(selected?.title).toContain("Provider reported");
    expect(selected?.title).toContain("Git observed");
    selected?.click();
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ relativePath: "src/new.ts" }));
  });

  it("collapses and restores a directory without changing the selected file", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatChangedFileTree, {
      target,
      props: { files: [changed("src/a.ts", true, true)], selectedFile: "src/a.ts", onSelect: vi.fn() },
    });
    await tick();
    target.querySelector<HTMLButtonElement>("button")?.click();
    await tick();
    expect(target.textContent).not.toContain("a.ts");
    target.querySelector<HTMLButtonElement>("button")?.click();
    await tick();
    expect(target.querySelector("button.selected")?.textContent).toContain("a.ts");
  });
});

function changed(
  relativePath: string,
  providerReported: boolean,
  gitObserved: boolean,
  previousRelativePath: string | null = null,
): ChatChangedFileRead {
  return {
    relativePath,
    previousRelativePath,
    status: previousRelativePath ? "renamed" : "modified",
    additions: 2,
    deletions: 1,
    binary: false,
    providerReported,
    gitObserved,
  };
}
