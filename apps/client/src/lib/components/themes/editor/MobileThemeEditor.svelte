<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Check from "@lucide/svelte/icons/check";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { getThemeEditor } from "$lib/stores/theme-editor.svelte";
  import ThemeEditor from "./ThemeEditor.svelte";

  const themeStore = getTheme();
  const themeEditor = getThemeEditor();
  const mobileBackStack = getMobileBackStack();
  const { t } = getLocalization();

  const editing = $derived(
    themeEditor.editingId
      ? themeStore.registry[themeEditor.editingId]
      : undefined,
  );
  const isBuiltin = $derived(
    themeEditor.editingId
      ? themeStore.isBuiltin(themeEditor.editingId)
      : false,
  );
  const canResetToSeed = $derived(
    editing?.kind === "user" && themeStore.canResetThemeToSeed(editing.id),
  );

  let panel = $state<HTMLDivElement | null>(null);
  let discardConfirmationOpen = $state(false);
  let pending = $state(false);
  let operationError = $state("");
  let sessionSettlementStarted = false;

  function requestCancel(): void {
    if (pending) return;
    if (themeEditor.hasUnsavedChanges) {
      discardConfirmationOpen = true;
      return;
    }
    void cancel();
  }

  async function cancel(): Promise<void> {
    if (pending) return;
    pending = true;
    operationError = "";
    sessionSettlementStarted = true;
    try {
      await themeEditor.cancel();
    } catch (error) {
      sessionSettlementStarted = false;
      operationError = error instanceof Error ? error.message : String(error);
      pending = false;
    }
  }

  async function commit(): Promise<void> {
    if (pending) return;
    pending = true;
    operationError = "";
    sessionSettlementStarted = true;
    try {
      await themeEditor.commit();
    } catch (error) {
      sessionSettlementStarted = false;
      operationError = error instanceof Error ? error.message : String(error);
      pending = false;
    }
  }

  function resetAllToSeed(): void {
    if (editing?.kind !== "user" || !canResetToSeed) return;
    themeStore.resetThemeToSeed(editing.id);
  }

  onMount(() => {
    const deactivateBack = mobileBackStack.activate({ handle: requestCancel });
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (panel) deactivateFocus = activateModalFocus(panel);
    });
    return () => {
      deactivateBack();
      deactivateFocus();
    };
  });

  onDestroy(() => {
    if (!sessionSettlementStarted && themeEditor.editingId) {
      void themeEditor.cancel();
    }
  });
</script>

{#if editing}
  <div
    bind:this={panel}
    class="mobile-theme-editor fixed z-85 flex flex-col overflow-hidden bg-background text-foreground outline-none"
    style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);"
    data-app-shortcuts="ignore"
    role="dialog"
    aria-modal="true"
    aria-label={t("settings.theme.editor.dialogLabel")}
    tabindex="-1"
  >
    <header class="flex min-h-14 shrink-0 items-center gap-1 border-b border-border bg-sidebar px-1">
      <button
        type="button"
        onclick={requestCancel}
        disabled={pending}
        aria-label={t("settings.theme.editor.backToThemes")}
        class="flex size-12 shrink-0 items-center justify-center rounded-xl active:bg-accent disabled:opacity-40"
      >
        <ArrowLeft size={22} aria-hidden="true" />
      </button>
      <div class="min-w-0 flex-1 px-2">
        <h2 class="truncate text-base font-semibold">{editing.displayName}</h2>
        <p class="truncate text-xs text-muted-foreground">
          {isBuiltin
            ? t("settings.theme.editor.builtInReadOnly")
            : t("settings.theme.editor.editingTheme")}
        </p>
      </div>
    </header>

    <div class="min-h-0 flex-1">
      <ThemeEditor theme={editing} />
    </div>

    {#if operationError}
      <Notice class="shrink-0 border-t border-border px-4 py-2 text-sm" title={t("settings.theme.editor.operationFailed")} message={operationError} />
    {/if}

    <footer class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-border bg-sidebar px-2 py-2">
      <div class="flex min-w-0 items-center gap-2">
        <button
          type="button"
          onclick={requestCancel}
          disabled={pending}
          class="flex h-9 items-center rounded-lg border border-border bg-card px-3 text-sm font-medium active:bg-accent disabled:opacity-40"
        >
          {t("common.cancel")}
        </button>
        {#if editing.kind === "user"}
          <button
            type="button"
            onclick={resetAllToSeed}
            disabled={!canResetToSeed || pending}
            aria-label={t("settings.theme.editor.resetAllToSeed")}
            class="flex size-9 items-center justify-center rounded-lg border border-border bg-card active:bg-accent disabled:opacity-40"
          >
            <RotateCcw size={16} aria-hidden="true" />
          </button>
        {/if}
      </div>
      <button
        type="button"
        onclick={() => void commit()}
        disabled={pending}
        class="flex h-9 items-center gap-2 rounded-lg bg-primary px-3 text-sm font-semibold text-primary-foreground active:opacity-90 disabled:opacity-40"
      >
        <Check size={16} aria-hidden="true" />
        <span>
          {isBuiltin
            ? t("settings.theme.editor.applyAndReturn")
            : t("settings.theme.editor.saveAndApply")}
        </span>
      </button>
    </footer>
  </div>

  {#if discardConfirmationOpen}
    <ConfirmDialog
      title={t("settings.theme.editor.discardTitle")}
      message={t("settings.theme.editor.discardMessage")}
      confirmLabel={t("settings.theme.editor.discardChanges")}
      cancelLabel={t("settings.theme.editor.keepEditing")}
      onConfirm={() => {
        discardConfirmationOpen = false;
        void cancel();
      }}
      onCancel={() => {
        discardConfirmationOpen = false;
      }}
    />
  {/if}
{/if}

<style>
  .mobile-theme-editor :global(.theme-editor-identity),
  .mobile-theme-editor :global(.theme-editor-nav-shell) {
    height: 44px;
  }
</style>
