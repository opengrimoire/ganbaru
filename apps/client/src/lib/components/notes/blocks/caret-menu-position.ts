import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
import { notesBlockInsertMenuStyle } from "$lib/notes/blocks/insertion";
import { findEditableDomPoint } from "$lib/notes/editor/selection";
import { portal } from "$lib/utils/portal";
import { tick } from "svelte";

/** Inputs for a floating menu anchored at a text offset inside a Notes editing host. */
export interface NotesCaretMenuPosition {
  editor: HTMLElement | null;
  /** Plain-text offset of the trigger character the menu hangs from. */
  offset: number;
  /** The menu's scrolling list, used to size the menu to its content up to the preferred height. */
  scrollArea: () => HTMLElement | null;
  /** Called for a pointer press outside the menu and the editor. */
  onOutside?: () => void;
  /** Presses inside these elements stay inside the menu, for example popovers the menu opens. */
  keepOpenFor?: (target: Element) => boolean;
  preferredMaxHeightRem?: number;
}

const DEFAULT_MAX_HEIGHT_REM = 22;

/**
 * Portal a menu to the editor's floating root and keep it beside the trigger character while the editor, the
 * viewport, or the menu itself changes size or scrolls.
 *
 * @param node The menu element.
 * @param initial The editor, trigger offset, and outside-press handling.
 * @returns A Svelte action lifecycle.
 */
export function notesCaretMenuPosition(node: HTMLElement, initial: NotesCaretMenuPosition) {
  let params = initial;
  const editor = initial.editor;
  if (!editor) return {};
  const portaled = portal(node, editor.closest<HTMLElement>("[data-floating-root]") ?? document.body);
  const viewport = window.visualViewport;
  let disposed = false;

  const update = () => {
    if (disposed) return;
    const start = findEditableDomPoint(editor, params.offset);
    const range = editor.ownerDocument.createRange();
    range.setStart(start.node, start.offset);
    range.collapse(true);
    const caret = range.getBoundingClientRect?.();
    const rect = caret?.height ? caret : editor.getBoundingClientRect();
    const offsetLeft = viewport?.offsetLeft ?? 0;
    const offsetTop = viewport?.offsetTop ?? 0;
    const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
    const maxHeight = (params.preferredMaxHeightRem ?? DEFAULT_MAX_HEIGHT_REM) * rem;
    const scrollArea = params.scrollArea();
    const chromeHeight = node.offsetHeight - (scrollArea?.clientHeight ?? 0);
    node.style.cssText = notesBlockInsertMenuStyle({
      triggerRect: {
        left: rect.left - offsetLeft,
        right: rect.right - offsetLeft,
        top: rect.top - offsetTop,
        bottom: rect.bottom - offsetTop,
      },
      viewportWidth: viewport?.width ?? window.innerWidth,
      viewportHeight: viewport?.height ?? window.innerHeight,
      preferredWidth: FLOATING_WIDTH.lg,
      preferredMaxHeight: Math.min(maxHeight, (scrollArea?.scrollHeight ?? maxHeight) + chromeHeight),
    });
    node.style.left = `${Number.parseFloat(node.style.left) + offsetLeft}px`;
    node.style.top = `${Number.parseFloat(node.style.top) + offsetTop}px`;
  };
  const outside = (event: PointerEvent) => {
    const target = event.target;
    if (!(target instanceof Node) || node.contains(target) || editor.contains(target)) return;
    if (target instanceof Element && params.keepOpenFor?.(target)) return;
    params.onOutside?.();
  };
  const scroll = (event: Event) => {
    if (!(event.target instanceof Node) || !node.contains(event.target)) update();
  };
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
  observer?.observe(node);
  observer?.observe(editor);
  document.addEventListener("pointerdown", outside, true);
  document.addEventListener("scroll", scroll, true);
  window.addEventListener("resize", update);
  viewport?.addEventListener("resize", update);
  viewport?.addEventListener("scroll", update);
  void tick().then(update);

  return {
    update(next: NotesCaretMenuPosition) {
      const moved = next.offset !== params.offset;
      params = next;
      if (moved) update();
    },
    destroy() {
      disposed = true;
      observer?.disconnect();
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", update);
      viewport?.removeEventListener("resize", update);
      viewport?.removeEventListener("scroll", update);
      portaled.destroy();
    },
  };
}
