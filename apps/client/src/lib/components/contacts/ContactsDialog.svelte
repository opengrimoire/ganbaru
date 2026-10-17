<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus, activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";

  /**
   * Modal shell for Contacts flows that open above another dialog or a floating panel: invitation and contact dialogs.
   * It lives under the document body so it is never clipped by its opener, owns the keyboard as its own layer, and
   * closes on the backdrop, Escape, and the Android back gesture. The footer buttons share one look so every Contacts
   * dialog confirms the same way.
   */
  let {
    title,
    description = null,
    layer = "first",
    initialFocus = null,
    cancelLabel = null,
    confirmLabel = null,
    confirmDisabled = false,
    onConfirm,
    onClose,
    children,
  }: {
    title: string;
    description?: string | null;
    /** Stacking order above the opener: the first nested dialog, or a second one opened from the first. */
    layer?: "first" | "second";
    initialFocus?: HTMLElement | null;
    cancelLabel?: string | null;
    confirmLabel?: string | null;
    confirmDisabled?: boolean;
    /** Runs the confirmation; when absent the confirm button keeps its final look but stays unavailable. */
    onConfirm?: () => void;
    onClose: () => void;
    children: Snippet;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back");
  const titleId = `contacts-dialog-${crypto.randomUUID()}`;
  let dialog = $state<HTMLDivElement | null>(null);

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      return;
    }
    if (event.key !== "Escape" || dialog?.querySelector("[data-app-floating-surface]")) return;
    event.preventDefault();
    onClose();
  }

  onMount(() => {
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    const deactivateBack = androidSystemBackAvailable ? mobileBackStack.activate({ handle: onClose }) : null;
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, initialFocus);
    });
    return () => {
      deactivateKeyboard();
      deactivateBack?.();
      deactivateFocus();
    };
  });
</script>

<!-- The backdrop belongs to the same floating surface as the dialog, so hosts that close on outside presses treat a backdrop press as this dialog's own dismissal. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  use:portal
  class={cn("fixed flex items-center justify-center", layer === "first" ? "z-100" : "z-110")}
  data-app-floating-surface
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onclick={(event) => { event.stopPropagation(); onClose(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <div
    bind:this={dialog}
    class="contacts-dialog surface-dialog relative z-10 flex max-h-full w-full max-w-md flex-col outline-none"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-labelledby={titleId}
    tabindex="-1"
    data-floating-root
    data-app-floating-surface
    onclick={(event) => event.stopPropagation()}
  >
    <header class="flex items-start justify-between gap-3 px-5 pt-4 pb-1">
      <div class="min-w-0">
        <h2 id={titleId} class="truncate text-[1rem] font-semibold text-foreground">{title}</h2>
        {#if description}<p class="mt-0.5 text-panel-detail text-muted-foreground">{description}</p>{/if}
      </div>
      <button type="button" class="contacts-dialog-icon-button" aria-label={t("common.close")} onclick={onClose}><X size={16} /></button>
    </header>
    <div class="flex min-h-0 flex-col gap-4 overflow-y-auto px-5 py-3 text-panel">
      {@render children()}
    </div>
    {#if cancelLabel || confirmLabel}
      <footer class="flex justify-end gap-2 px-5 pt-2 pb-4">
        {#if cancelLabel}
          <button type="button" class="contacts-dialog-button secondary" onclick={onClose}>{cancelLabel}</button>
        {/if}
        {#if confirmLabel && onConfirm}
          <button type="button" class="contacts-dialog-button primary" disabled={confirmDisabled} onclick={onConfirm}>{confirmLabel}</button>
        {:else if confirmLabel}
          <button type="button" class="contacts-dialog-button primary control-unavailable" aria-disabled="true">{confirmLabel}</button>
        {/if}
      </footer>
    {/if}
  </div>
</div>

<style>
  .contacts-dialog { max-height: min(40rem, 100%); }
  .contacts-dialog-icon-button { display: grid; width: 1.75rem; height: 1.75rem; flex: 0 0 auto; place-items: center; border-radius: 0.375rem; color: var(--muted-foreground); }
  .contacts-dialog-icon-button:hover { background: var(--accent); color: var(--foreground); }
  .contacts-dialog-button { display: inline-flex; min-height: 2.25rem; align-items: center; justify-content: center; gap: 0.4rem; border-radius: 0.375rem; padding: 0.4rem 0.8rem; font-size: calc(0.8rem * var(--type-scale)); font-weight: 600; transition: background-color 120ms ease; }
  .contacts-dialog-button.secondary { border: 1px solid var(--border); background: var(--background); color: var(--foreground); }
  .contacts-dialog-button.secondary:hover { background: var(--accent); }
  .contacts-dialog-button.primary { background: var(--primary); color: var(--primary-foreground); }
  .contacts-dialog-button.primary:hover:not(:disabled) { background: color-mix(in srgb, var(--primary) 90%, transparent); }
  .contacts-dialog-button:disabled { cursor: not-allowed; opacity: 0.5; }
  @media (pointer: coarse) {
    .contacts-dialog-icon-button { width: 2.5rem; height: 2.5rem; }
    .contacts-dialog-button { min-height: 2.75rem; }
  }
</style>
