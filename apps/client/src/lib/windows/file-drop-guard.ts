/**
 * Cancels file drags that no drop target handled, so a file dropped outside a target does not replace the page.
 * Drop targets cancel the events they accept themselves; this runs after them, in the bubbling phase.
 *
 * @returns A function that removes the guard.
 */
export function installFileDropGuard(target: Window = window): () => void {
  const guard = (event: DragEvent): void => {
    if (event.defaultPrevented || !event.dataTransfer?.types.includes("Files")) return;
    event.preventDefault();
  };
  target.addEventListener("dragover", guard);
  target.addEventListener("drop", guard);
  return () => {
    target.removeEventListener("dragover", guard);
    target.removeEventListener("drop", guard);
  };
}
