<script lang="ts">
  import { onMount, tick } from "svelte";
  import MaskedCodeInput from "$lib/components/ui/MaskedCodeInput.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import { OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";
  import { lockAncestorScroll } from "$lib/mobile/scroll-lock";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { createBackdropPress } from "$lib/utils/backdrop-press";

  /**
   * A phone's modal for entering the recovery kit code that lets it join a server that refused it.
   * It hands the typed code back on confirm and keeps nothing once it closes. The page underneath stops
   * scrolling while it shows.
   */
  let {
    confirmLabel,
    onConfirm,
    onCancel,
  }: {
    /** Label of the confirm button, such as connecting now or only keeping the code. */
    confirmLabel: string;
    /** Called with the trimmed kit code. */
    onConfirm: (kit: string) => void;
    onCancel: () => void;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back");
  const backdropPress = createBackdropPress();
  let overlay = $state<HTMLDivElement | null>(null);
  let dialog = $state<HTMLDivElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);
  let kit = $state("");
  const canConfirm = $derived(kit.trim() !== "");

  function confirm(): void {
    if (canConfirm) onConfirm(kit.trim());
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      event.stopPropagation();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
      return;
    }
    if (event.key === "Enter" && event.target === input) {
      event.preventDefault();
      event.stopPropagation();
      confirm();
      return;
    }
    // Other keys stay inside the modal so the panel underneath does not act on them.
    event.stopPropagation();
  }

  onMount(() => {
    const deactivateMobileBack = androidSystemBackAvailable
      ? mobileBackStack.activate({ handle: () => onCancel() })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    const unlockScroll = overlay ? lockAncestorScroll(overlay) : () => undefined;
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      // A phone opens the keyboard only once the user taps the field.
      if (dialog) deactivateFocus = activateModalFocus(dialog, OPENING_MAY_FOCUS_TEXT ? input : dialog);
    });
    return () => {
      deactivateMobileBack();
      deactivateKeyboard();
      unlockScroll();
      deactivateFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={overlay}
  class="fixed z-90 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onpointerdown={backdropPress.start}
  onclick={(event) => { event.stopPropagation(); if (backdropPress.isClick(event)) onCancel(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={dialog}
    class="surface-dialog relative z-10 max-h-full w-full max-w-md overflow-y-auto px-5 py-5 outline-none"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-labelledby="recovery-kit-title"
    aria-describedby="recovery-kit-message"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <h2 id="recovery-kit-title" class="mb-1 text-base font-semibold text-foreground">{t("vaultHandoff.serverConnect.kitDialogTitle")}</h2>
    <p id="recovery-kit-message" class="text-[0.866667rem] leading-5 text-muted-foreground">{t("vaultHandoff.serverConnect.kitDialogMessage")}</p>
    <MaskedCodeInput
      bind:input
      bind:value={kit}
      id="recovery-kit-code"
      clearLabel={t("common.clearKit")}
      class="field mt-4 h-11 w-full px-3 font-mono text-sm"
      dir="ltr"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      aria-label={t("vaultHandoff.serverConnect.kitLabel")}
    />
    <div class="mt-5 flex flex-wrap items-center justify-end gap-2">
      <button
        type="button"
        class="min-h-11 rounded-md border border-border bg-card px-3.5 text-[0.866667rem] font-medium text-foreground transition-colors hover:bg-accent"
        onclick={onCancel}
      >
        {t("common.cancel")}
      </button>
      <button
        type="button"
        data-kit-confirm
        class="min-h-11 rounded-md bg-primary px-3.5 text-[0.866667rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-55"
        disabled={!canConfirm}
        onclick={confirm}
      >
        {confirmLabel}
      </button>
    </div>
  </div>
</div>
