// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import NotesPageCover from "./NotesPageCover.svelte";
import { notesPageCoverAssetUrl } from "$lib/api/notes/page-covers";

vi.mock("$lib/api/notes/page-covers", () => ({ notesPageCoverAssetUrl: vi.fn() }));
const frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;
let component: ReturnType<typeof mount> | undefined;

beforeEach(() => {
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frames.set(++nextFrame, callback); return nextFrame; });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => { frames.delete(id); });
  vi.stubGlobal("ResizeObserver", class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1000);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(200);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  frames.clear();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Render a loaded source with the actual drag handlers and a draft callback. */
async function mountCover(disabled = false) {
  const onFocalPoint = vi.fn();
  component = mount(NotesPageCover, { target: document.body, props: {
    cover: { type: "file", file: { url: "image.png" } }, previewUrl: "image.png",
    unavailableLabel: "Unavailable", focalLabel: "Drag image", onFocalPoint, positioningDisabled: disabled,
  } });
  await tick();
  const image = document.querySelector("img")!;
  Object.defineProperties(image, { naturalWidth: { value: 1000 }, naturalHeight: { value: 1000 } });
  image.dispatchEvent(new Event("load"));
  await tick();
  const button = document.querySelector("button")!;
  button.setPointerCapture = vi.fn();
  button.hasPointerCapture = vi.fn(() => true);
  button.releasePointerCapture = vi.fn();
  return { button, onFocalPoint };
}

/** Supply pointer identity in jsdom without replacing browser pointer handling. */
function dispatchPointer(button: HTMLButtonElement, type: string, y: number, id = 1): void {
  const event = new MouseEvent(type, { clientX: 100, clientY: y, button: 0, bubbles: true, cancelable: true });
  Object.defineProperty(event, "pointerId", { value: id });
  button.dispatchEvent(event);
}

describe("cover drag positioning", () => {
  it("keeps a skeleton until the image loads and exposes unavailable content only after failure", async () => {
    const onStatus = vi.fn();
    component = mount(NotesPageCover, { target: document.body, props: {
      cover: { type: "external", external: { url: "https://example.com/cover.png" } },
      previewUrl: "https://example.com/cover.png",
      unavailableLabel: "Unavailable", onStatus,
    } });
    await tick();
    expect(document.querySelector('[data-notes-skeleton="cover"]')).not.toBeNull();
    expect(document.body.textContent).not.toContain("Unavailable");
    expect(onStatus).toHaveBeenLastCalledWith("loading");
    const image = document.querySelector("img")!;
    image.dispatchEvent(new Event("load"));
    await tick();
    expect(document.querySelector('[data-notes-skeleton="cover"]')).toBeNull();
    expect(image.classList.contains("opacity-0")).toBe(false);
    expect(onStatus).toHaveBeenLastCalledWith("ready");
    image.dispatchEvent(new Event("error"));
    await tick();
    expect(document.querySelector('[data-notes-skeleton="cover"]')).toBeNull();
    expect(document.body.textContent).toContain("Unavailable");
    expect(onStatus).toHaveBeenLastCalledWith("error");
  });

  it("keeps the managed cover placeholder through asset resolution and image decoding", async () => {
    let resolveAsset!: (url: string) => void;
    vi.mocked(notesPageCoverAssetUrl).mockImplementationOnce(() => new Promise<string>((resolve) => { resolveAsset = resolve; }));
    const assetPath = `notes/page-covers/${"a".repeat(64)}.webp`;
    component = mount(NotesPageCover, { target: document.body, props: {
      cover: { type: "file", file: { url: `ganbaru-asset:${assetPath}`, ganbaru_asset_path: assetPath } },
      unavailableLabel: "Unavailable",
    } });
    await tick();
    expect(document.querySelector('[data-notes-skeleton="cover"]')).not.toBeNull();
    expect(document.body.textContent).not.toContain("Unavailable");
    expect(document.querySelector("img")).toBeNull();
    resolveAsset("data:image/webp;base64,YQ==");
    await tick(); await tick();
    expect(document.querySelector('[data-notes-skeleton="cover"]')).not.toBeNull();
    document.querySelector("img")!.dispatchEvent(new Event("load"));
    await tick();
    expect(document.querySelector('[data-notes-skeleton="cover"]')).toBeNull();
  });

  it("preserves the centered crop in a translated layer without a drag tooltip", async () => {
    const { button } = await mountCover();
    const image = button.querySelector("img")!;
    expect(button.dataset.appTooltipDisabled).toBe("true");
    expect(button.getAttribute("aria-label")).toBe("Drag image");
    expect(image.style.width).toBe("1000px");
    expect(image.style.height).toBe("1000px");
    expect(image.style.transform).toBe("translate3d(0px, -400px, 0)");
    expect(image.style.objectPosition).toBe("");
  });

  it("captures one pointer, previews movement, and stops after cancellation", async () => {
    const { button, onFocalPoint } = await mountCover();
    dispatchPointer(button, "pointerdown", 100);
    expect(button.setPointerCapture).toHaveBeenCalledWith(1);
    dispatchPointer(button, "pointermove", 150, 2);
    expect(onFocalPoint).not.toHaveBeenCalled();
    dispatchPointer(button, "pointermove", 150);
    dispatchPointer(button, "pointermove", 160);
    expect(onFocalPoint).not.toHaveBeenCalled();
    expect(frames.size).toBe(1);
    for (const [id, callback] of frames) { frames.delete(id); callback(0); }
    await tick();
    expect(button.querySelector("img")!.style.transform).toBe("translate3d(0px, -340px, 0)");
    dispatchPointer(button, "pointercancel", 160);
    expect(onFocalPoint).toHaveBeenLastCalledWith({ x: 0.5, y: 0.44 });
    expect(button.releasePointerCapture).toHaveBeenCalledWith(1);
    dispatchPointer(button, "pointermove", 200);
    expect(onFocalPoint).toHaveBeenCalledTimes(1);
  });

  it("keeps the final release position even before a preview frame runs", async () => {
    const { button, onFocalPoint } = await mountCover();
    dispatchPointer(button, "pointerdown", 100);
    dispatchPointer(button, "pointermove", 130);
    dispatchPointer(button, "pointerup", 150);
    expect(onFocalPoint).toHaveBeenCalledExactlyOnceWith({ x: 0.5, y: 0.45 });
    expect(frames.size).toBe(0);
  });

  it("supports directional keyboard movement and centering", async () => {
    const { button, onFocalPoint } = await mountCover();
    button.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(onFocalPoint).toHaveBeenLastCalledWith({ x: 0.5, y: 0.48 });
    button.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true }));
    expect(onFocalPoint).toHaveBeenLastCalledWith({ x: 0.5, y: 0.5 });
  });

  it("blocks pointer and keyboard edits while the position is saving", async () => {
    const { button, onFocalPoint } = await mountCover(true);
    dispatchPointer(button, "pointerdown", 100);
    dispatchPointer(button, "pointermove", 150);
    button.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    expect(button.setPointerCapture).not.toHaveBeenCalled();
    expect(onFocalPoint).not.toHaveBeenCalled();
  });
});
