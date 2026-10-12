const PANEL_ARROW_KEYS = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"]);
const PANEL_FOCUSABLE_SELECTOR = [
  "button",
  "input",
  "textarea",
  "[contenteditable='true']",
  "[role='button']",
  "[tabindex]",
].join(",");

interface PanelNavCandidate {
  el: HTMLElement;
  rect: DOMRect;
  centerX: number;
  centerY: number;
}

function isElementVisibleForPanelNav(el: HTMLElement): boolean {
  if (el.closest("[aria-hidden='true']")) return false;
  if (el.getClientRects().length === 0) return false;
  const style = getComputedStyle(el);
  return style.display !== "none" && style.visibility !== "hidden";
}

function isEditablePanelNavTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const input = target.closest("input, textarea");
  if (input instanceof HTMLTextAreaElement) return true;
  if (!(input instanceof HTMLInputElement)) return false;
  if (input.dataset.panelArrowNav === "true") return false;
  return true;
}

function panelNavTarget(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof HTMLElement)) return null;
  return target.closest<HTMLElement>(PANEL_FOCUSABLE_SELECTOR);
}

function panelNavCandidates(root: HTMLElement): PanelNavCandidate[] {
  return Array.from(root.querySelectorAll<HTMLElement>(PANEL_FOCUSABLE_SELECTOR))
    .filter((el) => {
      if (el instanceof HTMLButtonElement || el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
        if (el.disabled) return false;
      }
      return isElementVisibleForPanelNav(el);
    })
    .map((el) => {
      const rect = el.getBoundingClientRect();
      return {
        el,
        rect,
        centerX: rect.left + rect.width / 2,
        centerY: rect.top + rect.height / 2,
      };
    });
}

function sortPanelNavCandidates(candidates: PanelNavCandidate[]): PanelNavCandidate[] {
  return [...candidates].sort((a, b) => {
    const rowDelta = a.centerY - b.centerY;
    if (Math.abs(rowDelta) > 10) return rowDelta;
    return a.centerX - b.centerX;
  });
}

function panelNavRows(candidates: PanelNavCandidate[]): PanelNavCandidate[][] {
  const rows: PanelNavCandidate[][] = [];
  for (const candidate of sortPanelNavCandidates(candidates)) {
    const previousRow = rows.at(-1);
    const rowCenter = previousRow
      ? previousRow.reduce((sum, item) => sum + item.centerY, 0) / previousRow.length
      : 0;
    const sameRowThreshold = Math.max(10, candidate.rect.height * 0.65);
    if (previousRow && Math.abs(candidate.centerY - rowCenter) <= sameRowThreshold) {
      previousRow.push(candidate);
    } else {
      rows.push([candidate]);
    }
  }
  return rows.map((row) => row.sort((a, b) => a.centerX - b.centerX));
}

function rowAndIndexFor(
  rows: PanelNavCandidate[][],
  current: HTMLElement,
): { rowIndex: number; itemIndex: number } | null {
  for (const [rowIndex, row] of rows.entries()) {
    const itemIndex = row.findIndex((candidate) => candidate.el === current);
    if (itemIndex >= 0) return { rowIndex, itemIndex };
  }
  return null;
}

function closestCandidateByX(row: PanelNavCandidate[], x: number): PanelNavCandidate | undefined {
  let best: { candidate: PanelNavCandidate; distance: number } | undefined;
  for (const candidate of row) {
    const distance = Math.abs(candidate.centerX - x);
    if (!best || distance < best.distance) best = { candidate, distance };
  }
  return best?.candidate;
}

function nextPanelArrowTarget(root: HTMLElement, current: HTMLElement, key: string): HTMLElement | null {
  const rows = panelNavRows(panelNavCandidates(root));
  const currentPosition = rowAndIndexFor(rows, current);
  if (!currentPosition) return null;

  const currentRow = rows[currentPosition.rowIndex];
  const currentCandidate = currentRow[currentPosition.itemIndex];
  if (!currentCandidate) return null;

  if (key === "ArrowRight") {
    return currentRow[currentPosition.itemIndex + 1]?.el
      ?? rows[currentPosition.rowIndex + 1]?.[0]?.el
      ?? null;
  }

  if (key === "ArrowLeft") {
    return currentRow[currentPosition.itemIndex - 1]?.el
      ?? rows[currentPosition.rowIndex - 1]?.at(-1)?.el
      ?? null;
  }

  const targetRow = key === "ArrowDown"
    ? rows[currentPosition.rowIndex + 1]
    : rows[currentPosition.rowIndex - 1];
  return targetRow ? closestCandidateByX(targetRow, currentCandidate.centerX)?.el ?? null : null;
}

export function panelArrowKeyTarget(
  event: KeyboardEvent,
  root: HTMLElement | undefined,
  disabled: boolean,
): HTMLElement | null {
  if (event.defaultPrevented || disabled) return null;
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return null;
  if (!PANEL_ARROW_KEYS.has(event.key)) return null;
  if (isEditablePanelNavTarget(event.target)) return null;
  if (!root) return null;

  const current = panelNavTarget(event.target);
  if (!current || !root.contains(current)) return null;
  return nextPanelArrowTarget(root, current, event.key);
}

export function isPanelArrowKey(key: string): boolean {
  return PANEL_ARROW_KEYS.has(key);
}
