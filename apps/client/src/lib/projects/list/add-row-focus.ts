import { tick } from "svelte";

export interface ProjectListAddRowInputFocusOptions {
  selector: string;
  beforeFocus?: () => void;
}

function projectListAddRowClickShouldFocus(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return true;
  return !target.closest("input, textarea, select, button, a, [contenteditable='true'], [role='textbox']");
}

function focusProjectListTextInput(input: HTMLInputElement): void {
  input.focus({ preventScroll: true });
  const caretPosition = input.value.length;
  input.setSelectionRange(caretPosition, caretPosition);
  requestAnimationFrame(() => {
    input.focus({ preventScroll: true });
    input.setSelectionRange(caretPosition, caretPosition);
  });
}

function focusProjectListInputFromRow(
  node: HTMLElement,
  options: ProjectListAddRowInputFocusOptions,
  event: MouseEvent,
): void {
  if (event.button !== 0) return;
  const target = event.target;
  if (!(target instanceof Element)) return;
  if (!projectListAddRowClickShouldFocus(target)) return;

  options.beforeFocus?.();
  void tick().then(() => {
    const input = node.querySelector<HTMLInputElement>(options.selector);
    if (!input) return;
    focusProjectListTextInput(input);
  });
}

export function projectListAddRowInputFocus(
  node: HTMLElement,
  options: ProjectListAddRowInputFocusOptions,
): {
  update: (nextOptions: ProjectListAddRowInputFocusOptions) => void;
  destroy: () => void;
} {
  let currentOptions = options;
  const handleClick = (event: MouseEvent): void => {
    focusProjectListInputFromRow(node, currentOptions, event);
  };

  node.addEventListener("click", handleClick);

  return {
    update(nextOptions: ProjectListAddRowInputFocusOptions) {
      currentOptions = nextOptions;
    },
    destroy() {
      node.removeEventListener("click", handleClick);
    },
  };
}
