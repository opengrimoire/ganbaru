/** Show the full label in the app tooltip only while its text is clipped. */
export function overflowTooltip(node: HTMLElement, label: string): {
  update: (nextLabel: string) => void;
  destroy: () => void;
} {
  let currentLabel = label;
  const sync = (): void => {
    if (node.scrollWidth - node.clientWidth > 2) node.dataset.appTooltip = currentLabel;
    else delete node.dataset.appTooltip;
  };
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(sync);
  observer?.observe(node);
  node.addEventListener("pointerover", sync);
  queueMicrotask(sync);
  return {
    update(nextLabel: string): void {
      currentLabel = nextLabel;
      sync();
    },
    destroy(): void {
      observer?.disconnect();
      node.removeEventListener("pointerover", sync);
      delete node.dataset.appTooltip;
    },
  };
}
