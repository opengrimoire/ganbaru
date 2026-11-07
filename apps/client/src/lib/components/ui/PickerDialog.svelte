<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";
  import { activateIsolatingKeyboardLayer, activateModalFocus } from "$lib/modal-focus";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { createBackdropPress } from "$lib/utils/backdrop-press";
  import { portal } from "$lib/utils/portal";

  /**
   * A centered modal that holds a touch date or time picker. The backdrop, Android Back, and Escape close it.
   * Pickers opened from inside one stack above it.
   */
  let {
    label,
    onClose,
    children,
  }: {
    /** Accessible name of the dialog. */
    label: string;
    onClose: () => void;
    children: Snippet;
  } = $props();

  const mobileBackStack = getMobileBackStack();
  const backdropPress = createBackdropPress();
  let element = $state<HTMLDivElement>();

  onMount(() => {
    const deactivateBack = mobileBackStack.activate({ handle: () => onClose() });
    const deactivateKeyboard = activateIsolatingKeyboardLayer((event) => {
      if (event.key !== "Escape") return false;
      event.preventDefault();
      onClose();
      return true;
    });
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (!element) return;
      // A picker may already have focused its own control, such as the selected day; keep that focus.
      const active = document.activeElement;
      const initial = active instanceof HTMLElement && element.contains(active) ? active : element;
      deactivateFocus = activateModalFocus(element, initial);
    });
    return () => {
      deactivateBack();
      deactivateKeyboard();
      deactivateFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  use:portal
  data-app-floating-surface
  data-calendar-edit-close-ignore
  class="fixed z-100 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onpointerdown={backdropPress.start}
  onclick={(event) => { event.stopPropagation(); if (backdropPress.isClick(event)) onClose(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <div
    bind:this={element}
    class="surface-dialog relative z-10 max-h-full w-full max-w-96 overflow-y-auto p-4 outline-none"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-label={label}
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
  >
    {@render children()}
  </div>
</div>
