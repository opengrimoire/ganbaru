<script lang="ts">
  import type { Snippet } from "svelte";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * A destination sidebar on the phone shell, opened as a floating panel over the content with the chrome of the
   * project and channel pickers: a titled header with a close button, a search field, and a touch-sized explorer list.
   * The backdrop and Android back close it. Fixed menus and dialogs a sidebar opens belong in `children`, so they stay
   * inside the panel's focus scope.
   */
  let {
    title,
    searchLabel,
    clearSearchLabel,
    query = $bindable(""),
    onClose,
    children,
  }: {
    /** Names the panel and its dialog. */
    title: string;
    searchLabel: string;
    clearSearchLabel: string;
    /** The sidebar's search text. */
    query?: string;
    onClose: () => void;
    /** The explorer list, plus any menus and dialogs it opens. */
    children: Snippet;
  } = $props();

  const { t } = getLocalization();
  const iconStrokeWidth = 1.6;
</script>

<ProjectPickerMobileDialog label={title} closeLabel={t("common.close")} {onClose}>
  <div class="surface-floating flex h-full min-h-0 flex-col overflow-hidden rounded-2xl" data-mobile-sidebar-panel>
    <div class="shrink-0 border-b border-border/70 p-2">
      <div class="flex min-h-14 items-center gap-1">
        <h2 class="min-w-0 flex-1 truncate px-2 text-base font-semibold">{title}</h2>
        <button
          type="button"
          data-project-picker-initial-focus="true"
          class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
          aria-label={t("common.close")}
          data-mobile-sidebar-panel-close
          onclick={onClose}
        >
          <X size={22} strokeWidth={iconStrokeWidth} aria-hidden="true" />
        </button>
      </div>
      <div class="field flex min-h-12 items-center gap-1.5 rounded-xl px-3">
        <Search size={18} strokeWidth={iconStrokeWidth} class="shrink-0 text-popover-foreground/60" />
        <input
          bind:value={query}
          placeholder={searchLabel}
          aria-label={searchLabel}
          class="field-bare h-12 text-base text-popover-foreground"
        />
        {#if query}
          <button
            type="button"
            class="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl text-popover-foreground/60 active:bg-accent"
            aria-label={clearSearchLabel}
            onclick={() => { query = ""; }}
          >
            <X size={18} strokeWidth={iconStrokeWidth} aria-hidden="true" />
          </button>
        {/if}
      </div>
    </div>
    {@render children()}
  </div>
</ProjectPickerMobileDialog>
