// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setLanguagePreference } from "$lib/i18n/translator.svelte";
import NotesSlashMenu from "./NotesSlashMenu.svelte";

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  await setLanguagePreference("en", { persist: false });
});

/** Mount a menu in a clipped block inside a floating dialog boundary. */
function host() {
  const dialog = document.createElement("div");
  dialog.dataset.floatingRoot = "";
  const row = document.createElement("div");
  row.style.overflow = "hidden";
  const anchor = document.createElement("div");
  anchor.contentEditable = "true";
  anchor.tabIndex = 0;
  anchor.textContent = "/";
  row.append(anchor);
  dialog.append(row);
  document.body.append(dialog);
  return { dialog, row, anchor };
}

describe("Notes slash menu", () => {
  it("fades only edges with hidden commands and updates when filtering removes overflow", async () => {
    const { row } = host();
    component = mount(NotesSlashMenu, { target: row, props: { onSelect: vi.fn() } });
    await tick();
    const scroll = row.querySelector<HTMLDivElement>(".slash-scroll")!;
    let contentHeight = 600;
    Object.defineProperties(scroll, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, get: () => contentHeight },
    });
    scroll.dispatchEvent(new Event("scroll"));
    expect(scroll.dataset.scrollFade).toBe("bottom");
    scroll.scrollTop = 100;
    scroll.dispatchEvent(new Event("scroll"));
    expect(scroll.dataset.scrollFade).toBe("both");
    scroll.scrollTop = 400;
    scroll.dispatchEvent(new Event("scroll"));
    expect(scroll.dataset.scrollFade).toBe("top");
    const input = row.querySelector<HTMLInputElement>("input")!;
    input.value = "missing-command";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    contentHeight = 150;
    scroll.scrollTop = 0;
    scroll.dispatchEvent(new Event("scroll"));
    expect(scroll.dataset.scrollFade).toBeUndefined();
  });

  it("escapes the clipped row, stays inside its dialog, and preserves editor focus on selection", async () => {
    const { dialog, row, anchor } = host();
    anchor.focus();
    const onSelect = vi.fn();
    component = mount(NotesSlashMenu, { target: row, props: { anchor, onSelect, onClose: vi.fn(), canSetColor: false, query: "h2" } });
    await tick();
    const menu = dialog.querySelector<HTMLElement>('[role="menu"]')!;
    expect(menu.parentElement).toBe(dialog);
    expect(row.querySelector('[role="menu"]')).toBeNull();
    await vi.waitFor(() => expect(menu.style.position).toBe("fixed"));
    const option = menu.querySelector<HTMLButtonElement>('[role="menuitem"]')!;
    const down = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    option.dispatchEvent(down);
    expect(down.defaultPrevented).toBe(true);
    option.click();
    expect(onSelect).toHaveBeenCalledWith({ kind: "block", blockType: "heading_2" });
    expect(document.activeElement).toBe(anchor);
  });

  it("searches localized labels, handles no results, and selects from the keyboard", async () => {
    await setLanguagePreference("es", { persist: false });
    const { row } = host();
    const onSelect = vi.fn();
    component = mount(NotesSlashMenu, { target: row, props: { onSelect, canSetColor: false } });
    await tick();
    const input = row.querySelector<HTMLInputElement>("input")!;
    input.value = "xyzmissing";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(row.querySelectorAll('[role="menuitem"]')).toHaveLength(0);
    expect(row.querySelector('[role="status"]')).not.toBeNull();
    input.value = "parrafo";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(row.querySelectorAll('[role="menuitem"]')).toHaveLength(1);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(onSelect).toHaveBeenCalledWith({ kind: "block", blockType: "paragraph" });
  });

  it("positions above a bottom-edge caret and follows viewport changes", async () => {
    const { row, anchor, dialog } = host();
    let top = window.innerHeight - 25;
    vi.spyOn(anchor, "getBoundingClientRect").mockImplementation(() => new DOMRect(window.innerWidth - 30, top, 20, 20));
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(400);
    const onClose = vi.fn();
    component = mount(NotesSlashMenu, { target: row, props: { anchor, onSelect: vi.fn(), onClose } });
    await tick();
    const menu = dialog.querySelector<HTMLElement>('[role="menu"]')!;
    await vi.waitFor(() => expect(Number.parseFloat(menu.style.top)).toBeLessThan(top));
    expect(Number.parseFloat(menu.style.left) + Number.parseFloat(menu.style.width)).toBeLessThanOrEqual(window.innerWidth);
    top = 10;
    window.dispatchEvent(new Event("resize"));
    expect(Number.parseFloat(menu.style.top)).toBeGreaterThan(top);
    document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    expect(onClose).toHaveBeenCalledOnce();
    await unmount(component);
    component = undefined;
    expect(dialog.querySelector('[role="menu"]')).toBeNull();
    document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("lets keyboard activation reach the close button and delegates Escape to an insertion menu owner", async () => {
    const { row } = host();
    const onSelect = vi.fn();
    const onClose = vi.fn();
    component = mount(NotesSlashMenu, { target: row, props: { onSelect, onClose } });
    await tick();
    const close = [...row.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.includes("Close menu"))!;
    close.focus();
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    close.dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(false);
    close.click();
    expect(onClose).toHaveBeenCalledOnce();
    expect(onSelect).not.toHaveBeenCalled();
    await unmount(component);
    component = mount(NotesSlashMenu, { target: row, props: { onSelect } });
    await tick();
    const dismissOwner = vi.fn();
    row.addEventListener("keydown", dismissOwner);
    row.querySelector("input")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(dismissOwner).toHaveBeenCalledOnce();
  });

  it("keeps the active command visible while navigating and consumes Escape", async () => {
    const { row } = host();
    const onClose = vi.fn();
    component = mount(NotesSlashMenu, { target: row, props: { onSelect: vi.fn(), onClose, canSetColor: false } });
    await tick();
    const menu = row.querySelector<HTMLElement>('[role="menu"]')!;
    const option = menu.querySelector<HTMLElement>('[role="menuitem"]')!;
    const scroller = menu.querySelector<HTMLDivElement>(".slash-scroll")!;
    vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 300, 100));
    const second = menu.querySelectorAll<HTMLElement>('[role="menuitem"]')[1];
    vi.spyOn(second, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 110, 300, 36));
    menu.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    await tick(); await tick();
    expect(second.dataset.active).toBe("true");
    await vi.waitFor(() => expect(scroller.scrollTop).toBeGreaterThan(0));
    const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    menu.dispatchEvent(escape);
    expect(escape.defaultPrevented).toBe(true);
    expect(onClose).toHaveBeenCalledOnce();
  });
});
