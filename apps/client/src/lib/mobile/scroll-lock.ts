/**
 * Stops every scrolling ancestor of `element` from scrolling while a modal shows over it. When the soft keyboard
 * shrinks the WebView, the page under the modal then keeps its scroll position and shows no scrollbar for the
 * space the keyboard took. Each ancestor keeps its scroll offset, and the returned function restores the inline
 * overflow it had.
 */
export function lockAncestorScroll(element: Element): () => void {
  const view = element.ownerDocument.defaultView;
  if (!view) return () => undefined;
  const locked: { ancestor: HTMLElement; overflowY: string }[] = [];
  for (let ancestor = element.parentElement; ancestor; ancestor = ancestor.parentElement) {
    const { overflowY } = view.getComputedStyle(ancestor);
    if (overflowY !== "auto" && overflowY !== "scroll") continue;
    locked.push({ ancestor, overflowY: ancestor.style.overflowY });
    ancestor.style.overflowY = "hidden";
  }
  return () => {
    for (const { ancestor, overflowY } of locked) ancestor.style.overflowY = overflowY;
  };
}
