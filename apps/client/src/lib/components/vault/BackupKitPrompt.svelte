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
  import { createBackdropPress } from "$lib/utils/backdrop-press";
  import { backupRefusalMessage } from "$lib/vault/backup-restore";
  import type { BackupRefusal } from "$lib/vault/state";

  /**
   * Asks for the recovery kit code that opens a backup. The code stays in this dialog until it is
   * submitted and is cleared when the dialog closes.
   */
  let {
    refusal,
    onSubmit,
    onCancel,
  }: {
    /** Why the previous code did not open the backup, or `null` on the first request. */
    refusal: BackupRefusal | null;
    onSubmit: (code: string) => void;
    onCancel: () => void;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "system.android-back",
  );
  const backdropPress = createBackdropPress();
  let element = $state<HTMLDivElement | undefined>();
  let input = $state<HTMLTextAreaElement | null>(null);
  let code = $state("");
  const canSubmit = $derived(code.trim() !== "");

  function submit(): void {
    if (!canSubmit) return;
    const typed = code;
    code = "";
    onSubmit(typed);
  }

  function cancel(): void {
    code = "";
    onCancel();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && element) {
      trapModalTabKey(element, event);
      event.stopPropagation();
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      event.stopPropagation();
      submit();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancel();
      return;
    }
    event.stopPropagation();
  }

  onMount(() => {
    const deactivateMobileBack = androidSystemBackAvailable
      ? mobileBackStack.activate({ handle: () => cancel() })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateModalFocus = (): void => undefined;
    void tick().then(() => {
      if (element) deactivateModalFocus = activateModalFocus(element, input);
    });
    return () => {
      code = "";
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
  onpointerdown={backdropPress.start}
  onclick={(event) => { event.stopPropagation(); if (backdropPress.isClick(event)) cancel(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={element}
    class="surface-dialog relative z-10 max-h-full w-full max-w-md overflow-y-auto px-5 py-5 outline-none sm:px-8"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-labelledby="backup-kit-prompt-title"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <form class="flex flex-col gap-4" onsubmit={(event) => { event.preventDefault(); submit(); }}>
      <div class="text-left">
        <h2 id="backup-kit-prompt-title" class="mb-1 text-[1rem] font-semibold text-foreground">
          {t("settings.data.kitPromptTitle")}
        </h2>
        <p class="text-[0.866667rem] text-foreground">{t("settings.data.kitPromptMessage")}</p>
      </div>
      <label class="flex flex-col gap-1.5">
        <span class="text-[0.8rem] text-muted-foreground">{t("settings.data.kitPromptLabel")}</span>
        <textarea
          bind:this={input}
          bind:value={code}
          rows="3"
          autocomplete="off"
          autocapitalize="characters"
          spellcheck="false"
          class="field resize-none font-mono text-[0.866667rem] tracking-wider"
          dir="ltr"
        ></textarea>
      </label>
      {#if refusal}
        <p role="alert" class="text-[0.8rem] leading-5 text-destructive">
          {backupRefusalMessage(refusal, t)}
        </p>
      {/if}
      <div class="flex flex-wrap items-center justify-start gap-2">
        <button
          type="button"
          onclick={cancel}
          class="min-h-12 rounded-md border border-border bg-card px-3.5 py-2 text-[0.866667rem] font-medium text-foreground transition-colors hover:bg-accent"
        >
          {t("common.cancel")}
        </button>
        <button
          type="submit"
          disabled={!canSubmit}
          class="min-h-12 rounded-md border border-border bg-primary px-3.5 py-2 text-[0.866667rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-55"
        >
          {t("settings.data.kitPromptAction")}
        </button>
      </div>
    </form>
  </div>
</div>
