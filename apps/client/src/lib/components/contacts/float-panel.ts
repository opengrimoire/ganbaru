import { tick } from "svelte";
import type { Action } from "svelte/action";
import { activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
import { anchoredPanelStyle } from "$lib/utils/anchored-panel";
import { portal } from "$lib/utils/portal";

export interface FloatPanelParams {
  /** Control the panel opens from; the panel sits below or above it and returns focus to it on Escape. */
  anchor: HTMLElement;
  width: number;
  horizontalAlign?: "start" | "end";
  /** Pointer and Escape dismissal; keep it off while a nested dialog owns the keyboard layer. */
  dismissEnabled?: boolean;
  onDismiss: () => void;
  /**
   * Receives the panel's other keys, such as list navigation from a search field. The keyboard layer stops events
   * before they reach elements, so element key handlers inside the panel never run.
   */
  onKeydown?: (event: KeyboardEvent) => void;
}

/**
 * Anchors a floating surface to its trigger, keeps it inside the viewport while content or layout changes, and
 * dismisses it on outside pointer presses and Escape. The node moves into the nearest floating root (or the document
 * body) so it escapes clipped ancestors; mark it `data-floating-root` so nested Select menus stay inside it.
 *
 * The panel owns the keyboard as a modal layer while it is open, so it works inside dialogs that own a layer
 * themselves. Because the layer router stops events before they reach elements, the handler also traps Tab and clears
 * a search field on Escape, which those elements would otherwise do on their own.
 */
export const floatPanel: Action<HTMLElement, FloatPanelParams> = (node, params) => {
  let current = params;
  const portaled = portal(node, params.anchor.closest<HTMLElement>("[data-floating-root]") ?? document.body);

  function place(): void {
    if (!current.anchor.isConnected) return;
    node.style.cssText = anchoredPanelStyle({
      triggerRect: current.anchor.getBoundingClientRect(),
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
      preferredWidth: current.width,
      contentHeight: node.scrollHeight,
      horizontalAlign: current.horizontalAlign ?? "end",
    });
  }

  function handlePointerDown(event: PointerEvent): void {
    if (current.dismissEnabled === false) return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (node.contains(target) || current.anchor.contains(target)) return;
    current.onDismiss();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab") {
      trapModalTabKey(node, event);
      return;
    }
    if (event.key !== "Escape") {
      const target = event.target;
      if (target instanceof Node && node.contains(target)) current.onKeydown?.(event);
      return;
    }
    if (current.dismissEnabled === false) return;
    const target = event.target;
    if (target instanceof Node && !node.contains(target)) return;
    if (node.querySelector("[data-app-floating-surface]")) return;
    if (target instanceof HTMLInputElement && target.type === "search" && target.value) {
      event.preventDefault();
      target.value = "";
      target.dispatchEvent(new Event("input", { bubbles: true }));
      return;
    }
    event.preventDefault();
    current.onDismiss();
    current.anchor.focus({ preventScroll: true });
  }

  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
  observer?.observe(node);
  observer?.observe(current.anchor);
  const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
  window.addEventListener("pointerdown", handlePointerDown, true);
  window.addEventListener("resize", place);
  window.addEventListener("scroll", place, true);
  void tick().then(() => {
    place();
    if (!node.contains(document.activeElement)) node.focus({ preventScroll: true });
  });

  return {
    update(next) {
      if (next.anchor !== current.anchor) {
        observer?.unobserve(current.anchor);
        observer?.observe(next.anchor);
      }
      current = next;
      place();
    },
    destroy() {
      observer?.disconnect();
      deactivateKeyboard();
      window.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
      portaled.destroy();
    },
  };
};
