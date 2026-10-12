<script lang="ts">
  import type { Snippet } from "svelte";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Save from "@lucide/svelte/icons/save";
  import X from "@lucide/svelte/icons/x";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    presentation,
    draftReady,
    dirty,
    saving,
    busy = false,
    error,
    title,
    discardLabel,
    closeLabel,
    saveLabel,
    onDiscard,
    onClose,
    onSave,
    children,
    scrollElement = $bindable<HTMLElement | undefined>(),
  }: {
    presentation: "side" | "popover";
    draftReady: boolean;
    dirty: boolean;
    saving: boolean;
    busy?: boolean;
    error: string | null;
    title: string;
    discardLabel: string;
    closeLabel: string;
    saveLabel: string;
    onDiscard: () => void;
    onClose: () => void;
    onSave: () => void;
    children: Snippet;
    scrollElement?: HTMLElement;
  } = $props();

  const { t } = getLocalization();

  let discardConfirmOpen = $state(false);

  function confirmDiscard(): void {
    discardConfirmOpen = false;
    onDiscard();
  }
</script>

<aside
  class={cn(
    "project-settings-panel flex min-h-0 flex-col",
    presentation === "popover"
      ? "h-full w-full"
      : "w-[min(23rem,42vw)] min-w-64 shrink-0 border-l border-border bg-card max-[760px]:fixed max-[760px]:inset-2 max-[760px]:z-30 max-[760px]:w-auto max-[760px]:rounded-floating max-[760px]:border max-[760px]:shadow-floating",
  )}
>
  <header class="sticky top-0 z-10 flex shrink-0 items-center gap-2 px-3 pb-1 pt-2">
    <div class="min-w-0 flex-1">
      <div class="truncate text-[0.8125rem] font-medium">{title}</div>
    </div>
    <button
      type="button"
      class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
      aria-label={discardLabel}
      title={discardLabel}
      disabled={!draftReady || !dirty || saving}
      onclick={() => {
        if (dirty) discardConfirmOpen = true;
      }}
    >
      <RotateCcw size={14} strokeWidth={1.75} />
    </button>
    <button
      type="button"
      class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-label={closeLabel}
      title={closeLabel}
      disabled={saving}
      onclick={onClose}
    >
      <X size={15} strokeWidth={1.75} />
    </button>
  </header>

  <form class="flex min-h-0 flex-1 flex-col" onsubmit={(event) => { event.preventDefault(); }}>
    {#if draftReady}
      <div class="relative min-h-0 flex-1">
        <div
          bind:this={scrollElement}
          data-settings-content
          use:scrollEdgeFadeAction
          class="hide-scrollbar h-full min-h-0 overflow-y-auto px-3 pb-3 pt-1"
        >
          <fieldset disabled={saving} inert={saving} class="flex min-w-0 flex-col gap-3 border-0 p-0">
            {@render children()}
          </fieldset>
        </div>
        <CalendarScrollbar
          scrollContainer={scrollElement}
          stickyTop={8}
          stickyBottom={8}
          wheelPassthrough
        />
      </div>

      <footer class="flex shrink-0 items-center gap-2 px-3 pb-2 pt-1">
        {#if error}
          <Notice class="min-w-0 flex-1 text-panel-detail" message={error} />
        {:else}
          <div class="min-w-0 flex-1"></div>
        {/if}
        <button
          type="button"
          class={cn(
            "flex min-h-8 shrink-0 items-center gap-1.5 rounded-md bg-primary px-2 text-[0.8rem] font-medium text-primary-foreground disabled:cursor-not-allowed",
            dirty || saving ? "hover:bg-primary/90" : "opacity-60",
          )}
          disabled={saving || busy || !dirty}
          onclick={onSave}
        >
          <Save size={14} strokeWidth={1.75} />
          <span>{saveLabel}</span>
        </button>
      </footer>
    {/if}
  </form>
</aside>

{#if discardConfirmOpen}
  <ConfirmDialog
    title={t("calendar.view.discardUnsavedTitle")}
    message={t("calendar.view.changesLost")}
    confirmLabel={t("calendar.view.discard")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmDiscard}
    onCancel={() => {
      discardConfirmOpen = false;
    }}
  />
{/if}

<style>
  .project-settings-panel {
    --cal-scrollbar-thumb: color-mix(in srgb, var(--card-foreground) 18%, var(--card));
    --cal-scrollbar-thumb-hover: color-mix(in srgb, var(--card-foreground) 36%, var(--card));
  }
</style>
