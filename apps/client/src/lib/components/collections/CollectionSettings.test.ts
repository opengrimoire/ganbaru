// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import SettingsFixture from "./CollectionSettingsHarness.test.svelte";

let component: ReturnType<typeof mount> | undefined;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Mount settings inside a clipped page preview and wait for initial focus. */
async function openSettings(onClose = vi.fn(), onCommit = vi.fn(), showHeader = true, filterDisabled = false) {
  const preview = document.createElement("div");
  preview.dataset.floatingRoot = "";
  const clipped = document.createElement("div");
  clipped.style.overflow = "hidden";
  preview.append(clipped);
  document.body.append(preview);
  component = mount(SettingsFixture, { target: clipped, props: { onClose, onCommit, showHeader, filterDisabled } });
  const anchor = clipped.querySelector<HTMLButtonElement>("button")!;
  vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(new DOMRect(800, 32, 32, 32));
  anchor.focus();
  anchor.click();
  await tick();
  await tick();
  await tick();
  const panel = preview.querySelector<HTMLDivElement>('[role="dialog"]')!;
  return { preview, clipped, anchor, panel, onClose, onCommit };
}

/** Press a key at the focused element as a browser would. */
async function key(key: string): Promise<void> {
  document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  await tick();
  await tick();
}

describe("Collection settings navigation", () => {
  it("includes fractional header height and borders when fitting content and retains scrolling in small viewports", async () => {
    vi.stubGlobal("innerHeight", 230);
    let contentHeight = 80.25;
    const borderStyle = document.createElement("div").style;
    borderStyle.borderTopWidth = "1.25px";
    borderStyle.borderBottomWidth = "1.25px";
    const getComputedStyle = window.getComputedStyle.bind(window);
    vi.spyOn(window, "getComputedStyle").mockImplementation((element) => element.classList.contains("collection-panel")
      ? borderStyle : getComputedStyle(element));
    const { panel, anchor } = await openSettings();
    vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(new DOMRect(800, 170, 32, 28));
    vi.spyOn(panel.querySelector<HTMLElement>("[data-collection-settings-header]")!, "getBoundingClientRect")
      .mockReturnValue(new DOMRect(0, 0, 320, 40.25));
    const content = panel.querySelector<HTMLElement>("[data-collection-settings-content]")!;
    vi.spyOn(content, "getBoundingClientRect").mockImplementation(() => new DOMRect(0, 0, 320, contentHeight));
    window.dispatchEvent(new Event("resize"));
    expect(panel.style.maxHeight).toBe("158px");
    expect(panel.style.top).toBe("43px");

    contentHeight = 1_000;
    vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(new DOMRect(800, 32, 32, 32));
    vi.stubGlobal("innerHeight", 160);
    window.dispatchEvent(new Event("resize"));
    expect(panel.style.maxHeight).toBe("84px");
    expect(panel.style.top).toBe("68px");
    expect(panel.querySelector('[aria-label="View name"]')).not.toBeNull();
  });

  it("shrinks to a shorter settings page without reusing the previous scroll viewport height", async () => {
    vi.stubGlobal("innerHeight", 600);
    const { panel, anchor } = await openSettings();
    vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(new DOMRect(800, 550, 32, 32));
    vi.spyOn(panel.querySelector<HTMLElement>("[data-collection-settings-header]")!, "getBoundingClientRect")
      .mockReturnValue(new DOMRect(0, 0, 320, 40.25));
    const content = panel.querySelector<HTMLElement>("[data-collection-settings-content]")!;
    vi.spyOn(content, "scrollHeight", "get").mockReturnValue(1_000);
    vi.spyOn(content, "getBoundingClientRect").mockImplementation(() => {
      const page = content.querySelector<HTMLElement>("[data-collection-settings-page]:not([hidden])");
      return new DOMRect(0, 0, 320, page?.querySelector('[aria-label="Layout value"]') ? 80.25 : 1_000);
    });
    window.dispatchEvent(new Event("resize"));
    expect(panel.style.top).toBe("66px");
    expect(panel.style.maxHeight).toBe("480px");
    expect(content.parentElement?.classList.contains("overflow-y-auto")).toBe(true);
    expect(content.classList.contains("overflow-y-auto")).toBe(false);

    panel.querySelector<HTMLButtonElement>('[aria-label="Layout"]')!.click();
    await tick();
    await tick();
    expect(panel.style.top).toBe("426px");
    expect(panel.style.maxHeight).toBe("480px");
    const shortStyle = panel.style.cssText;
    for (let index = 0; index < 5; index += 1) window.dispatchEvent(new Event("resize"));
    expect(panel.style.cssText).toBe(shortStyle);
    await key("Escape");
    expect(panel.style.top).toBe("66px");
  });

  it("anchors a nonmodal panel within its preview without a full-screen backdrop", async () => {
    const { preview, clipped, anchor, panel } = await openSettings();
    expect(panel.parentElement).toBe(preview);
    expect(clipped.querySelector('[role="dialog"]')).toBeNull();
    expect(panel.style.position).toBe("fixed");
    expect(panel.style.width).toBe("320px");
    expect(panel.style.left).toBe("512px");
    expect(panel.style.top).toBe("68px");
    expect(panel.getAttribute("aria-modal")).toBeNull();
    expect(preview.querySelectorAll("[data-app-floating-surface]")).toHaveLength(1);
    expect(document.activeElement).toBe(panel.querySelector('[aria-label="View name"]'));
    await key("Escape");
    expect(panel.isConnected).toBe(false);
    expect(document.activeElement).toBe(anchor);
  });

  it("omits the first page title and close button when the header is hidden but keeps Back on drill-in pages", async () => {
    const { panel } = await openSettings(vi.fn(), vi.fn(), false);
    expect(panel.querySelector("[data-collection-settings-header]")).toBeNull();
    expect(panel.getAttribute("aria-label")).toBe("View settings");

    panel.querySelector<HTMLButtonElement>('[aria-label="Layout"]')!.click();
    await tick();
    await tick();
    const header = panel.querySelector<HTMLElement>("[data-collection-settings-header]");
    expect(header?.textContent).toContain("Layout");
    expect(header?.querySelector('[aria-label="Back"]')).not.toBeNull();
    expect(header?.querySelector('[aria-label="Close"]')).toBeNull();

    header!.querySelector<HTMLButtonElement>('[aria-label="Back"]')!.click();
    await tick();
    await tick();
    expect(panel.querySelector("[data-collection-settings-header]")).toBeNull();
  });

  it("opens a page requested while its row is disabled once the row is enabled", async () => {
    const { panel } = await openSettings(vi.fn(), vi.fn(), true, true);
    const fixture = component as { showFilter: () => void; enableFilter: () => void };
    fixture.showFilter();
    await tick();
    await tick();
    expect(panel.getAttribute("aria-label")).toBe("View settings");
    fixture.enableFilter();
    await tick();
    await tick();
    expect(panel.getAttribute("aria-label")).toBe("Filter");
    fixture.showFilter();
    await tick();
    await tick();
    expect(panel.querySelectorAll("[data-collection-settings-page]")).toHaveLength(2);
  });

  it("shows details in the same panel, keeps root drafts mounted, and returns one level on Back or Escape", async () => {
    const { preview, panel, anchor } = await openSettings();
    const draft = panel.querySelector<HTMLInputElement>('[aria-label="View name"]')!;
    draft.value = "Weekly plan";
    draft.dispatchEvent(new Event("input", { bubbles: true }));
    const layout = panel.querySelector<HTMLButtonElement>('[aria-label="Layout"]')!;
    expect(layout.textContent).toContain("Table");
    layout.click();
    await tick();
    await tick();
    expect(preview.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(panel.getAttribute("aria-label")).toBe("Layout");
    expect(draft.isConnected).toBe(true);
    expect(draft.closest<HTMLElement>("[data-collection-settings-page]")?.hidden).toBe(true);
    expect(document.activeElement).toBe(panel.querySelector('[aria-label="Layout value"]'));

    panel.querySelector<HTMLButtonElement>('[aria-label="Advanced layout"]')!.click();
    await tick();
    await tick();
    expect(panel.getAttribute("aria-label")).toBe("Advanced layout");
    await key("Escape");
    expect(panel.getAttribute("aria-label")).toBe("Layout");
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Advanced layout");
    panel.querySelector<HTMLButtonElement>('[aria-label="Back"]')!.click();
    await tick();
    await tick();
    expect(panel.getAttribute("aria-label")).toBe("View settings");
    expect(document.activeElement).toBe(layout);
    expect(panel.querySelector('[aria-label="View name"]')).toBe(draft);
    expect(draft.value).toBe("Weekly plan");
    await key("Escape");
    expect(panel.isConnected).toBe(false);
    expect(document.activeElement).toBe(anchor);
  });

  it("returns to the remaining page when the row that opened a page leaves the panel", async () => {
    const { panel } = await openSettings();
    panel.querySelector<HTMLButtonElement>('[aria-label="Layout"]')!.click();
    await tick();
    await tick();
    panel.querySelector<HTMLButtonElement>('[aria-label="Advanced layout"]')!.click();
    await tick();
    await tick();
    expect(panel.getAttribute("aria-label")).toBe("Advanced layout");
    (component as { removeLayout: () => void }).removeLayout();
    await tick();
    await new Promise((resolve) => setTimeout(resolve));
    await tick();
    expect(panel.getAttribute("aria-label")).toBe("View settings");
    expect(panel.querySelectorAll("[data-collection-settings-page]")).toHaveLength(1);
    expect(panel.querySelector('[aria-label="Advanced value"]')).toBeNull();
  });

  it("lets nested selects own the first Escape and selection before returning from the detail page", async () => {
    const { panel } = await openSettings();
    const filter = panel.querySelector<HTMLButtonElement>('[aria-label="Filter"]')!;
    filter.click();
    await tick();
    await tick();
    const select = panel.querySelector<HTMLButtonElement>('[aria-haspopup="listbox"]')!;
    select.click();
    await vi.waitFor(() => expect(document.activeElement?.getAttribute("role")).toBe("option"));
    expect(panel.querySelector('[role="listbox"]')?.parentElement).toBe(panel);
    await key("Escape");
    expect(panel.getAttribute("aria-label")).toBe("Filter");
    expect(panel.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(select);

    select.click();
    await vi.waitFor(() => expect(document.activeElement?.getAttribute("role")).toBe("option"));
    panel.querySelector<HTMLButtonElement>('[role="option"][aria-selected="false"]')!.click();
    await tick();
    expect(select.textContent).toContain("Done");
    expect(panel.getAttribute("aria-label")).toBe("Filter");
    await key("Escape");
    expect(panel.getAttribute("aria-label")).toBe("View settings");
    expect(document.activeElement).toBe(filter);
  });

  it("dismisses when outside focus moves and preserves that focus", async () => {
    const { panel, onClose } = await openSettings();
    const outside = document.createElement("button");
    document.body.append(outside);
    outside.focus();
    await tick();
    expect(panel.isConnected).toBe(false);
    expect(document.activeElement).toBe(outside);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("dismisses on outside pointer without restoring focus over the target", async () => {
    const { panel, onClose } = await openSettings();
    const outside = document.createElement("input");
    document.body.append(outside);
    outside.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    outside.focus();
    await tick();
    expect(panel.isConnected).toBe(false);
    expect(document.activeElement).toBe(outside);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("commits a focused draft before outside background dismissal without restoring anchor focus", async () => {
    const { panel, anchor, onClose, onCommit } = await openSettings();
    const anchorFocus = vi.spyOn(anchor, "focus");
    const input = panel.querySelector<HTMLInputElement>('[aria-label="View name"]')!;
    input.value = "Weekly plan";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const background = document.createElement("div");
    document.body.append(background);
    background.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(onCommit).toHaveBeenCalledExactlyOnceWith("Weekly plan");
    expect(input.isConnected).toBe(true);
    background.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    await tick();
    expect(panel.isConnected).toBe(false);
    expect(onClose).toHaveBeenCalledOnce();
    expect(onCommit).toHaveBeenCalledOnce();
    expect(anchorFocus).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(document.body);
  });

  it("handles Escape after a focused Save control becomes disabled by its fieldset", async () => {
    const { panel, anchor } = await openSettings();
    const save = [...panel.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Save")!;
    save.focus();
    save.click();
    // Emulate Chromium's focus loss before jsdom applies disabled attributes.
    save.blur();
    await tick();
    expect(save.matches(":disabled")).toBe(true);
    expect(document.activeElement).toBe(document.body);
    await key("Escape");
    expect(panel.isConnected).toBe(false);
    expect(document.activeElement).toBe(anchor);
  });

  it("ignores Escape from an enabled blurred input and from an unrelated control", async () => {
    const { panel } = await openSettings();
    const input = panel.querySelector<HTMLInputElement>('[aria-label="View name"]')!;
    input.blur();
    const bodyEscape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    document.body.dispatchEvent(bodyEscape);
    await tick();
    expect(bodyEscape.defaultPrevented).toBe(false);
    expect(panel.isConnected).toBe(true);

    input.focus();
    input.blur();
    input.disabled = true;
    const unrelated = document.createElement("input");
    document.body.append(unrelated);
    const unrelatedEscape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    unrelated.dispatchEvent(unrelatedEscape);
    await tick();
    expect(unrelatedEscape.defaultPrevented).toBe(false);
    expect(panel.isConnected).toBe(true);
    unrelated.focus();
    await tick();
    expect(panel.isConnected).toBe(false);
    expect(document.activeElement).toBe(unrelated);
  });

  it("navigates settings rows with arrows and removes listeners when unmounted", async () => {
    const { panel, onClose } = await openSettings();
    const layout = panel.querySelector<HTMLButtonElement>('[aria-label="Layout"]')!;
    const filter = panel.querySelector<HTMLButtonElement>('[aria-label="Filter"]')!;
    layout.focus();
    await key("ArrowDown");
    expect(document.activeElement).toBe(filter);
    await key("Home");
    expect(document.activeElement).toBe(layout);
    await unmount(component!);
    component = undefined;
    expect(panel.isConnected).toBe(false);
    expect(panel.contains(document.activeElement)).toBe(false);
    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(onClose).not.toHaveBeenCalled();
  });
});
