// @vitest-environment jsdom

import { createRawSnippet, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import StarredIconBadge from "./StarredIconBadge.svelte";

const icon = createRawSnippet(() => ({ render: () => '<svg class="row-icon"></svg>' }));

describe("StarredIconBadge", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  function render(starred: boolean): HTMLElement | null {
    target = document.createElement("div");
    document.body.appendChild(target);
    component = mount(StarredIconBadge, { target, props: { starred, children: icon } });
    return target.firstElementChild instanceof HTMLElement ? target.firstElementChild : null;
  }

  it("overlays a decorative star on the icon's corner for a favorite", () => {
    const badge = render(true);
    expect(badge?.classList.contains("relative")).toBe(true);
    expect(badge?.firstElementChild?.classList.contains("row-icon")).toBe(true);
    const star = badge?.querySelector(".starred-icon-badge");
    expect(star?.getAttribute("aria-hidden")).toBe("true");
    expect(star?.classList.contains("absolute")).toBe(true);
  });

  it("renders only the icon for an item that is not a favorite", () => {
    const badge = render(false);
    expect(badge?.querySelector(".row-icon")).not.toBeNull();
    expect(badge?.querySelector(".starred-icon-badge")).toBeNull();
  });
});
