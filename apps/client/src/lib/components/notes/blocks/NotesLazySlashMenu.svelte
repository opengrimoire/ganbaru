<script lang="ts">
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import type { NotesSlashCommand } from "$lib/notes/editor/slash-commands";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    loadNotesTextControl,
    retryNotesTextControl,
    type LoadedNotesTextControl,
  } from "$lib/components/notes/editor-component-registry";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    onSelect,
  }: {
    onSelect: (command: NotesSlashCommand) => void;
  } = $props();
  const { t } = getLocalization();

  let loadState = $state<LazyComponentLoadState<"slash-menu", LoadedNotesTextControl> | null>(null);

  /** Loads the slash menu on first use. A retry rejects when the load fails again. */
  function request(retry = false): Promise<void> {
    if (!retry && loadState) return Promise.resolve();
    const loadingState = beginLazyComponentLoad(loadState, "slash-menu");
    loadState = loadingState;
    const result = retry
      ? retryNotesTextControl("slash-menu")
      : loadNotesTextControl("slash-menu");
    return result.then((component) => {
      if (!loadState) return;
      loadState = resolveLazyComponentLoad(
        loadState,
        "slash-menu",
        loadingState.requestId,
        component,
      );
    }).catch((error: unknown) => {
      if (!loadState) return;
      loadState = rejectLazyComponentLoad(
        loadState,
        "slash-menu",
        loadingState.requestId,
        error,
      );
      console.error("load Notes slash menu failed", error);
      if (retry) throw error;
    });
  }

  $effect(() => {
    void request();
  });
</script>

{#if loadState?.status === "ready" && loadState.component.kind === "slash-menu"}
  {@const NotesSlashMenu = loadState.component.component}
  <NotesSlashMenu canSetColor={false} {onSelect} />
{:else if loadState?.status === "failed"}
  <div class="absolute top-full left-[calc(var(--notes-depth)*1.25rem+1.75rem)] z-50 mt-1 surface-floating surface-floating-body w-floating-lg">
    <Notice message={t("common.viewLoadFailed", t("notes.slashMenu"))} copy={false} onRetry={() => request(true)} />
  </div>
{:else}
  <div class="min-h-8" aria-busy="true"></div>
{/if}
