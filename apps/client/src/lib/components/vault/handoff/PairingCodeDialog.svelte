<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount, tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";

  let {
    busy,
    error,
    onSubmit,
    onEdit,
    onClose,
  }: {
    busy: boolean;
    error: string | null;
    onSubmit: (code: string) => void;
    onEdit: () => void;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  let dialog = $state<HTMLDivElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);
  let code = $state("");

  function submit(): void {
    const value = code.trim();
    if (value && !busy) onSubmit(value);
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
      onClose();
      return;
    }
    if (event.key === "Enter" && event.target === input) {
      event.preventDefault();
      submit();
    }
    event.stopPropagation();
  }

  onMount(() => {
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, input);
    });
    return () => {
      deactivateKeyboard();
      deactivateFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-90 flex items-center justify-center p-4" onclick={onClose}>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={dialog}
    class="relative z-10 w-full max-w-md surface-dialog px-5 py-5 outline-none sm:px-8"
    role="dialog"
    aria-modal="true"
    aria-labelledby="pairing-code-title"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <div>
      <h2 id="pairing-code-title" class="text-base font-semibold text-foreground">
        {t("vaultHandoff.codeDialogTitle")}
      </h2>
      <p class="mt-1 text-[0.866667rem] leading-5 text-muted-foreground">
        {t("vaultHandoff.codeInstructions")}
      </p>
    </div>

    <input
      bind:this={input}
      bind:value={code}
      type="text"
      autocomplete="off"
      spellcheck="false"
      class="field mt-5 h-10 w-full px-3 text-sm"
      placeholder={t("vaultHandoff.codePlaceholder")}
      aria-label={t("vaultHandoff.codePlaceholder")}
      aria-invalid={error !== null}
      aria-describedby={error ? "pairing-code-error" : undefined}
      disabled={busy}
      oninput={onEdit}
    />
    {#if error}
      <p id="pairing-code-error" role="alert" class="mt-2 text-[0.8rem] leading-5 text-destructive">{error}</p>
    {/if}
    <div class="mt-5 flex justify-end gap-2">
      <button
        type="button"
        class="inline-flex min-h-9 items-center justify-center rounded-md border border-border px-3 text-[0.8rem] font-medium hover:bg-accent"
        disabled={busy}
        onclick={onClose}
      >
        {t("common.close")}
      </button>
      <button
        type="button"
        class="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-55"
        disabled={busy || code.trim().length === 0}
        onclick={submit}
      >
        {#if busy}<LoaderCircle size={14} class="animate-spin" />{/if}
        {t("vaultHandoff.linkDevice")}
      </button>
    </div>
  </div>
</div>
