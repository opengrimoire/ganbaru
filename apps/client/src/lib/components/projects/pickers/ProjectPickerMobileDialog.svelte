<script lang="ts">
  import { setContext, type Snippet } from "svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { portal } from "$lib/utils/portal";

  let {
    label,
    closeLabel,
    onClose,
    children,
  }: {
    label: string;
    closeLabel: string;
    onClose: () => void;
    children: Snippet;
  } = $props();

  const mobileBackStack = getMobileBackStack();
  setContext(
    "ganbaru-mobile-nested-back",
    (handle: () => void) => mobileBackStack.activate({ handle }),
  );
  let dialog = $state<HTMLDivElement | null>(null);

  $effect(() => mobileBackStack.activate({ handle: onClose }));

  $effect(() => {
    if (!dialog) return;
    const initialFocus = dialog.querySelector<HTMLElement>(
      "[data-project-picker-initial-focus]",
    );
    return activateModalFocus(dialog, initialFocus);
  });
</script>

<div
  use:portal
  class="fixed z-80 box-border flex items-stretch justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 0.5rem) calc(var(--safe-area-right) + 0.5rem) calc(var(--safe-area-bottom) + 0.5rem) calc(var(--safe-area-left) + 0.5rem);"
>
  <button
    type="button"
    tabindex="-1"
    class="surface-backdrop absolute inset-0"
    aria-label={closeLabel}
    onclick={onClose}
  ></button>
  <div
    bind:this={dialog}
    class="relative z-10 h-full min-h-0 w-full max-w-xl"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label={label}
  >
    {@render children()}
  </div>
</div>
