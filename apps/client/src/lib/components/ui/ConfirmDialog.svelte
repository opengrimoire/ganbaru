<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "system.android-back",
  );
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";

  let {
    title,
    message,
    confirmLabel = t("common.yes"),
    cancelLabel = t("common.no"),
    danger = true,
    extraConfirmShortcut,
    element = $bindable(),
    onConfirm,
    onCancel,
    onDismiss = onCancel,
  }: {
    title?: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    danger?: boolean;
    extraConfirmShortcut?: (e: KeyboardEvent) => boolean;
    element?: HTMLDivElement;
    onConfirm: () => void;
    onCancel: () => void;
    onDismiss?: () => void;
  } = $props();

  const displayMessage = $derived(message.replace(/\.\s*$/u, ""));
  let cancelButtonElement = $state<HTMLButtonElement | null>(null);

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Tab" && element) {
      trapModalTabKey(element, e);
      e.stopPropagation();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      onConfirm();
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      (document.activeElement as HTMLElement)?.blur();
      onDismiss();
      return;
    }
    if (extraConfirmShortcut?.(e)) {
      e.preventDefault();
      e.stopPropagation();
      onConfirm();
      return;
    }
    // While the modal is open, prevent any other key from triggering
    // shortcuts in underlying components (e.g. the event panel's
    // Ctrl+Enter / Ctrl+D). Running in capture phase on window, this
    // stops propagation before any bubble-phase handler sees the event.
    e.stopPropagation();
  }

  onMount(() => {
    const deactivateMobileBack = androidSystemBackAvailable
      ? mobileBackStack.activate({ handle: () => onDismiss() })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateModalFocus = (): void => undefined;
    void tick().then(() => {
      if (element) deactivateModalFocus = activateModalFocus(element, cancelButtonElement);
    });
    return () => {
      deactivateMobileBack();
      deactivateKeyboard();
      deactivateModalFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed z-90 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onclick={(e) => { e.stopPropagation(); onDismiss(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={element}
    class="confirm-dialog surface-dialog relative z-10 max-h-full w-full max-w-md overflow-y-auto px-5 py-5 outline-none sm:px-8"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <div class="mb-5 text-left">
      {#if title}
        <h2 class="mb-1 text-[1rem] font-semibold text-foreground">{title}</h2>
        <p class="text-[0.866667rem] text-foreground whitespace-pre-line">{displayMessage}</p>
      {:else}
        <p class="text-[1rem] font-semibold text-foreground whitespace-pre-line">{displayMessage}</p>
      {/if}
    </div>
    <div class="flex flex-wrap items-center justify-start gap-2">
      <button
        type="button"
        bind:this={cancelButtonElement}
        onclick={onCancel}
        class="min-h-12 rounded-md border border-border bg-card px-3.5 py-2 text-[0.866667rem] font-medium text-foreground transition-colors hover:bg-accent"
      >
        {#if mobileShell}{cancelLabel}{:else}{`${cancelLabel} (${t("common.escapeKey")})`}{/if}
      </button>
      <button
        type="button"
        onclick={onConfirm}
        class="min-h-12 rounded-md border border-border bg-primary px-3.5 py-2 text-[0.866667rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90"
      >
        {#if mobileShell}{confirmLabel}{:else}{`${confirmLabel} (${t("common.enterKey")})`}{/if}
      </button>
    </div>
  </div>
</div>
