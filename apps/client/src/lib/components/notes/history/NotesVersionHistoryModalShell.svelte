<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import X from "@lucide/svelte/icons/x";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    title,
    closeLabel,
    backLabel,
    restoreLabel,
    restoringLabel,
    compact,
    compactPreviewOpen,
    previewBusy = false,
    versionsBusy = false,
    canRestore,
    restoring,
    error = null,
    dismissBlocked = false,
    onClose,
    onBack,
    onRestore,
    preview,
    versionList,
    footerActions,
  }: {
    title: string;
    closeLabel: string;
    backLabel: string;
    restoreLabel: string;
    restoringLabel: string;
    compact: boolean;
    compactPreviewOpen: boolean;
    previewBusy?: boolean;
    versionsBusy?: boolean;
    canRestore: boolean;
    restoring: boolean;
    error?: string | null;
    dismissBlocked?: boolean;
    onClose: () => void;
    onBack: () => void;
    onRestore: () => void;
    preview: Snippet;
    versionList: Snippet;
    footerActions?: Snippet;
  } = $props();

  let dialogElement = $state<HTMLDivElement | null>(null);

  function handleBackdropClick(event: MouseEvent): void {
    if (event.target !== event.currentTarget || dismissBlocked) return;
    onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && !dismissBlocked && dialogElement) {
      const focusable = [...dialogElement.querySelectorAll<HTMLElement>(
        "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex='-1'])",
      )].filter((element) => element.offsetParent !== null);
      if (focusable.length > 0) {
        const first = focusable[0];
        const last = focusable.at(-1) ?? first;
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
      return;
    }
    if (event.key !== "Escape" || dismissBlocked) return;
    event.preventDefault();
    event.stopPropagation();
    onClose();
  }

  onMount(() => {
    const returnFocusElement = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    void tick().then(() => dialogElement?.focus());
    window.addEventListener("keydown", handleKeydown, true);
    return () => {
      window.removeEventListener("keydown", handleKeydown, true);
      queueMicrotask(() => returnFocusElement?.focus());
    };
  });
</script>

<div
  class="surface-backdrop fixed inset-0 z-90 flex p-0 sm:p-5"
  role="presentation"
  onclick={handleBackdropClick}
>
  <div
    bind:this={dialogElement}
    class="surface-dialog relative flex min-h-0 w-full flex-1 flex-col overflow-hidden text-foreground outline-none max-sm:rounded-none max-sm:border-0 max-sm:shadow-none sm:mx-auto sm:max-w-7xl"
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    data-app-shortcuts="ignore"
  >
    <div class="grid min-h-0 flex-1 grid-cols-1 md:grid-cols-[minmax(0,1fr)_20rem]">
      {#if !compact || compactPreviewOpen}
        <main class="relative min-h-0 overflow-hidden" style="background-color: var(--cal-bg);">
          {#if compactPreviewOpen}
            <div
              class="absolute inset-x-0 top-0 z-20 flex h-11 items-center justify-between px-3 md:hidden"
              style="background-color: var(--cal-bg);"
            >
              <button
                type="button"
                class="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground"
                aria-label={backLabel}
                onclick={onBack}
              >
                <ArrowLeft class="size-4" />
              </button>
              <button
                type="button"
                class="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground"
                aria-label={closeLabel}
                onclick={onClose}
              >
                <X class="size-4" />
              </button>
            </div>
          {/if}
          <div
            class:pt-11={compactPreviewOpen}
            class="h-full min-h-0"
            aria-busy={previewBusy}
          >
            {@render preview()}
          </div>
        </main>
      {/if}

      {#if !compact || !compactPreviewOpen}
        <aside class="flex min-h-0 flex-col border-l-0 border-border bg-card md:border-l">
          <div class="sticky top-0 z-10 shrink-0 bg-card px-4 py-3">
            <div class="flex items-center justify-between gap-3">
              <h2 class="text-base font-semibold">{title}</h2>
              <button
                type="button"
                class="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground"
                aria-label={closeLabel}
                onclick={onClose}
              >
                <X class="size-4" />
              </button>
            </div>
            {#if error}
              <Notice class="mt-2 text-panel-detail" message={error} />
            {/if}
          </div>

          <div
            class="min-h-0 flex-1 overflow-auto px-2 pb-2"
            use:scrollEdgeFadeAction
            aria-busy={versionsBusy}
          >
            {@render versionList()}
          </div>

          <div class="sticky bottom-0 flex shrink-0 items-center gap-2 border-t border-border bg-card p-3">
            {#if footerActions}
              {@render footerActions()}
            {/if}
            <button
              type="button"
              class="flex h-9 min-w-0 flex-1 items-center justify-center rounded-md bg-primary px-4 text-[0.866667rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
              disabled={!canRestore || restoring}
              onclick={onRestore}
            >
              {restoring ? restoringLabel : restoreLabel}
            </button>
          </div>
        </aside>
      {/if}
    </div>

    {#if compactPreviewOpen}
      <div class="absolute inset-x-0 bottom-0 flex items-center gap-2 border-t border-border bg-card p-3 md:hidden">
        {#if footerActions}
          {@render footerActions()}
        {/if}
        <button
          type="button"
          class="flex h-9 min-w-0 flex-1 items-center justify-center rounded-md bg-primary px-4 text-[0.866667rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={!canRestore || restoring}
          onclick={onRestore}
        >
          {restoring ? restoringLabel : restoreLabel}
        </button>
      </div>
    {/if}
  </div>
</div>
