<script lang="ts">
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    taskSearch,
    matchingTaskCount,
    totalTaskCount,
    focusRequestId,
    onTaskSearchChange,
    onClose,
    onClearAndClose,
  }: {
    taskSearch: string;
    matchingTaskCount: number;
    totalTaskCount: number;
    focusRequestId: number;
    onTaskSearchChange: (value: string) => void;
    onClose: () => void;
    onClearAndClose: () => void;
  } = $props();

  const { t } = getLocalization();
  let taskFinderInputElement = $state<HTMLInputElement | null>(null);

  function focusInput(): void {
    requestAnimationFrame(() => {
      taskFinderInputElement?.focus();
      taskFinderInputElement?.select();
    });
  }

  $effect(() => {
    focusRequestId;
    focusInput();
  });

  function handleTaskFinderKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    if (taskSearch.trim()) {
      onClearAndClose();
      return;
    }
    onClose();
  }
</script>

<div class="pointer-events-none absolute inset-x-0 bottom-4 z-40 flex justify-center px-3">
  <div class="pointer-events-auto surface-floating flex min-h-10 w-[min(32rem,100%)] items-center gap-2 px-2">
    <Search size={15} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
    <input
      bind:this={taskFinderInputElement}
      value={taskSearch}
      aria-label={t("projects.finder.label")}
      placeholder={t("projects.header.searchPlaceholder")}
      class="field-bare"
      oninput={(event) => {
        onTaskSearchChange(event.currentTarget.value);
      }}
      onkeydown={handleTaskFinderKeydown}
    />
    <span class="hidden shrink-0 rounded-md bg-muted/70 px-2 py-1 text-panel-detail text-muted-foreground min-[520px]:inline">
      {t("projects.filters.matchingTasks", matchingTaskCount, totalTaskCount)}
    </span>
    <span class="hidden shrink-0 rounded-md border border-border px-2 py-1 text-panel-detail text-muted-foreground min-[420px]:inline">
      {t("projects.finder.shortcut")}
    </span>
    <button
      type="button"
      class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-label={taskSearch.trim() ? t("projects.finder.clear") : t("common.close")}
      onclick={onClearAndClose}
    >
      <X size={14} strokeWidth={1.75} />
    </button>
  </div>
</div>
