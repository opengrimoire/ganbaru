// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createCalendarOutsideCloseAction } from "./panel-dom";

interface Harness {
  root: HTMLElement;
  zone: HTMLElement;
  onClose: ReturnType<typeof vi.fn>;
  reachedGrid: ReturnType<typeof vi.fn>;
  destroy: () => void;
}

function mount(keepsOpenFor?: (target: Element) => boolean): Harness {
  const root = document.createElement("div");
  const zone = document.createElement("div");
  zone.setAttribute("data-calendar-edit-close-zone", "");
  zone.setAttribute("data-day-column-shell", "");
  root.append(zone);
  document.body.append(root);
  const onClose = vi.fn();
  const reachedGrid = vi.fn();
  zone.addEventListener("pointerdown", reachedGrid);
  const action = createCalendarOutsideCloseAction({
    closeGuardMs: 0,
    getConfirmOpen: () => false,
    getLastDragEndTime: () => 0,
    isPanelCommitHidden: () => false,
    isSessionClosed: () => false,
    keepsOpenFor,
    onClose,
  })(root);
  return { root, zone, onClose, reachedGrid, destroy: action.destroy };
}

function press(target: HTMLElement): void {
  target.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true }));
}

describe("createCalendarOutsideCloseAction", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  it("closes the session and keeps the press from the grid", () => {
    const harness = mount();
    press(harness.zone);
    expect(harness.onClose).toHaveBeenCalledOnce();
    expect(harness.reachedGrid).not.toHaveBeenCalled();
    harness.destroy();
  });

  it("keeps the session open and lets the grid handle a press the caller exempts", () => {
    const harness = mount((target) => target.closest("[data-day-column-shell]") !== null);
    press(harness.zone);
    expect(harness.onClose).not.toHaveBeenCalled();
    expect(harness.reachedGrid).toHaveBeenCalledOnce();
    harness.destroy();
  });

  it("still closes for a press the exemption does not cover", () => {
    const harness = mount((target) => target.closest("[data-other]") !== null);
    press(harness.zone);
    expect(harness.onClose).toHaveBeenCalledOnce();
    expect(harness.reachedGrid).not.toHaveBeenCalled();
    harness.destroy();
  });
});
