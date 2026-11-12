<script lang="ts">
  import Info from "@lucide/svelte/icons/info";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { onMount, tick } from "svelte";
  import type { ChatRestorePreviewRead } from "$lib/chat/contracts";
  import { buildRestoreDialogModel, type RestoreDialogFact } from "$lib/chat/review/restore-dialog-model";
  import ChatFileIcon from "$lib/components/chat/workspace/ChatFileIcon.svelte";
  import { formatNumber, formatRelativeMinutes } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus, activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { createBackdropPress } from "$lib/utils/backdrop-press";
  import { portal } from "$lib/utils/portal";

  /** How often the expiry countdown rereads the clock. */
  const EXPIRY_TICK_MS = 15_000;

  let {
    preview,
    refreshing = false,
    onRestore,
    onCancel,
    onRefresh,
  }: {
    preview: ChatRestorePreviewRead;
    /** True while a replacement preview is loading; both actions wait for it. */
    refreshing?: boolean;
    onRestore: () => void;
    onCancel: () => void;
    /** Requests a new preview after this one expired. */
    onRefresh: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back");
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const backdropPress = createBackdropPress();
  const titleId = `chat-restore-title-${crypto.randomUUID()}`;

  let element = $state<HTMLDivElement | null>(null);
  let cancelButton = $state<HTMLButtonElement | null>(null);
  let now = $state(Date.now());
  const model = $derived(buildRestoreDialogModel(preview, now));

  function factText(fact: RestoreDialogFact): string {
    switch (fact.kind) {
      case "staging_restored": return t("chat.restoreDialog.stagingRestored");
      case "staging_unchanged": return t("chat.restoreDialog.stagingUnchanged");
      case "provider_rollback": return t("chat.restoreDialog.providerRollback");
      case "provider_fork": return t("chat.restoreDialog.providerFork");
    }
  }

  function confirmPrimary(): void {
    if (refreshing) return;
    if (model.expired) onRefresh();
    else onRestore();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && element) {
      trapModalTabKey(element, event);
      event.stopPropagation();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      confirmPrimary();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
      return;
    }
    event.stopPropagation();
  }

  onMount(() => {
    const timer = window.setInterval(() => { now = Date.now(); }, EXPIRY_TICK_MS);
    const deactivateMobileBack = androidSystemBackAvailable
      ? mobileBackStack.activate({ handle: () => onCancel() })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateModalFocus = (): void => undefined;
    void tick().then(() => {
      if (element) deactivateModalFocus = activateModalFocus(element, cancelButton);
    });
    return () => {
      window.clearInterval(timer);
      deactivateMobileBack();
      deactivateKeyboard();
      deactivateModalFocus();
    };
  });

  $effect(() => {
    void preview;
    now = Date.now();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  use:portal
  class="fixed z-90 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onpointerdown={backdropPress.start}
  onclick={(event) => { event.stopPropagation(); if (backdropPress.isClick(event)) onCancel(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={element}
    class="surface-dialog relative z-10 flex max-h-full w-full max-w-lg flex-col overflow-hidden px-5 py-5 outline-none sm:px-8"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-labelledby={titleId}
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <h2 id={titleId} class="mb-1 text-[1rem] font-semibold text-foreground">{t("chat.restoreDialog.title")}</h2>
    <p class="mb-4 text-panel text-muted-foreground">{t("chat.restoreDialog.description")}</p>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <div class="restore-files">
        <div class="restore-files-header">
          <strong>{model.files.length === 0 ? t("chat.restoreDialog.noFiles") : t("chat.restoreDialog.files", model.files.length)}</strong>
          {#if model.additions > 0}<span class="additions">+{formatNumber(localization.locale, model.additions)}</span>{/if}
          {#if model.deletions > 0}<span class="deletions">−{formatNumber(localization.locale, model.deletions)}</span>{/if}
        </div>
        {#if model.files.length > 0}
          <ul data-mobile-scroll-fade class="restore-files-list">
            {#each model.files as file (file.relativePath)}
              <li>
                <ChatFileIcon path={file.relativePath} size={13} />
                <span class="restore-file-path" title={file.previousRelativePath ? t("chat.restoreDialog.renamedFrom", file.previousRelativePath) : file.relativePath}>{file.relativePath}</span>
                {#if file.binary}
                  <small class="text-muted-foreground">{t("chat.restoreDialog.binary")}</small>
                {:else}
                  {#if file.additions}<small class="additions">+{formatNumber(localization.locale, file.additions)}</small>{/if}
                  {#if file.deletions}<small class="deletions">−{formatNumber(localization.locale, file.deletions)}</small>{/if}
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <ul class="mt-3 grid gap-1.5">
        {#each model.facts as fact (fact.kind)}
          <li class="restore-fact text-panel" data-tone={fact.tone}>
            {#if fact.tone === "warning"}<TriangleAlert size={14} aria-hidden="true" />{:else}<Info size={14} aria-hidden="true" />{/if}
            <span>{factText(fact)}</span>
          </li>
        {/each}
      </ul>

      <p class="mt-3 text-panel-detail text-muted-foreground" role="status">
        {model.expired ? t("chat.restoreDialog.expired") : t("chat.restoreDialog.expiresIn", formatRelativeMinutes(t, model.remainingMinutes))}
      </p>
    </div>

    <div class="mt-5 flex flex-wrap items-center justify-start gap-2">
      <button
        type="button"
        bind:this={cancelButton}
        onclick={onCancel}
        class="min-h-12 rounded-md border border-border bg-card px-3.5 py-2 text-[0.866667rem] font-medium text-foreground transition-colors hover:bg-accent"
      >
        {#if mobileShell}{t("common.cancel")}{:else}{`${t("common.cancel")} (${t("common.escapeKey")})`}{/if}
      </button>
      {#if model.expired}
        <button
          type="button"
          onclick={onRefresh}
          disabled={refreshing}
          class="min-h-12 rounded-md border border-border bg-primary px-3.5 py-2 text-[0.866667rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-60"
        >
          {#if mobileShell}{t("chat.restoreDialog.refresh")}{:else}{`${t("chat.restoreDialog.refresh")} (${t("common.enterKey")})`}{/if}
        </button>
      {/if}
      <button
        type="button"
        onclick={onRestore}
        disabled={model.expired || refreshing}
        class="min-h-12 rounded-md border border-transparent bg-destructive px-3.5 py-2 text-[0.866667rem] font-medium text-destructive-foreground transition-colors hover:bg-destructive/90 disabled:opacity-60"
      >
        {#if mobileShell || model.expired}{t("chat.restoreDialog.restore")}{:else}{`${t("chat.restoreDialog.restore")} (${t("common.enterKey")})`}{/if}
      </button>
    </div>
  </div>
</div>

<style>
  .restore-files { overflow: hidden; border: 1px solid var(--border); border-radius: 0.8rem; background: color-mix(in srgb, var(--muted) 24%, var(--background)); }
  .restore-files-header { display: flex; min-width: 0; align-items: center; gap: 0.45rem; padding: 0.55rem 0.65rem; }
  .restore-files-header strong { font-size: calc(0.78rem * var(--type-scale)); font-weight: 600; }
  .additions { color: var(--action-confirm); }
  .deletions { color: var(--destructive); }
  .restore-files-list { display: grid; max-height: 12rem; overflow: auto; border-top: 1px solid var(--border); padding: 0.25rem; }
  .restore-files-list li { display: flex; min-width: 0; align-items: center; gap: 0.4rem; padding: 0.3rem 0.4rem; }
  .restore-file-path { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.72rem * var(--type-scale)); }
  .restore-files-list small { font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.66rem * var(--type-scale)); }
  .restore-fact { display: flex; align-items: flex-start; gap: 0.45rem; color: var(--foreground); }
  .restore-fact :global(svg) { flex: none; margin-top: 0.2em; color: var(--muted-foreground); }
  .restore-fact[data-tone="warning"] :global(svg) { color: var(--status-tentative); }
</style>
