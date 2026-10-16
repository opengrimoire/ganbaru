// @vitest-environment jsdom

import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import MenuItemLabel from "./MenuItemLabel.svelte";

describe("MenuItemLabel", () => {
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
    component = mount(MenuItemLabel, { target, props: { label: "Design", starred } });
    return target.firstElementChild instanceof HTMLElement ? target.firstElementChild : null;
  }

  it("places the favorite mark right after the name, inside the growing label", () => {
    const label = render(true);
    const name = label?.firstElementChild;
    expect(name?.textContent).toBe("Design");
    expect(name?.classList.contains("truncate")).toBe(true);
    expect(name?.nextElementSibling?.classList.contains("menu-item-star")).toBe(true);
    expect(name?.nextElementSibling?.getAttribute("aria-hidden")).toBe("true");
    expect(label?.classList.contains("flex-1")).toBe(true);
  });

  it("shows only the name for an item that is not a favorite", () => {
    const label = render(false);
    expect(label?.textContent?.trim()).toBe("Design");
    expect(label?.querySelector(".menu-item-star")).toBeNull();
  });
});
