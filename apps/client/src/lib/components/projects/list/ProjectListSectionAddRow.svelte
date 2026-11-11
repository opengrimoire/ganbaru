<script lang="ts">
  import type { MaybePromise } from "$lib/utils";
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectListAddRowInputFocus } from "$lib/projects/list/add-row-focus";


  let {
    gridTemplate,
    gridMinWidth,
    leadingGridTemplate,
    label,
    draft,
    active,
    onDraftChange,
    onActiveChange,
    onSubmit,
  }: {
    gridTemplate: string;
    gridMinWidth: string;
    leadingGridTemplate: string;
    label: string;
    draft: string;
    active: boolean;
    onDraftChange: (value: string) => void;
    onActiveChange: (active: boolean) => void;
    onSubmit: () => MaybePromise;
  } = $props();

  const { t } = getLocalization();
</script>

<div
  class="project-list-sticky-row grid min-h-(--collection-table-row-height) cursor-text items-center px-1 text-collection"
  data-add-section-row="true"
  style={`grid-template-columns: ${gridTemplate}; min-width: max(100%, ${gridMinWidth});`}
  use:projectListAddRowInputFocus={{
    selector: "[data-add-section-input='true']",
    beforeFocus: () => onActiveChange(true),
  }}
>
  <div class="absolute inset-0 z-0 cursor-text" aria-hidden="true"></div>
  <form
    class="contents"
    onsubmit={(event) => { event.preventDefault(); void onSubmit(); }}
  >
    <div
      class="project-list-leading-row relative z-10 grid items-center"
      style={`grid-column: 1 / span 3; grid-template-columns: ${leadingGridTemplate};`}
    >
      <div></div>
      <div></div>
      <div class="relative flex min-w-0 items-center gap-2 px-2">
        {#if !draft.trim() && !active}
          <div
            class="pointer-events-none absolute inset-y-0 left-2 flex items-center gap-2 text-muted-foreground"
            aria-hidden="true"
          >
            <span class="flex h-5 w-5 shrink-0 items-center justify-center">
              <Plus size={15} strokeWidth={1.75} />
            </span>
            <span>{label}</span>
          </div>
        {/if}
        <input
          data-add-section-input="true"
          aria-label={label}
          value={draft}
          onfocus={() => onActiveChange(true)}
          onblur={() => onActiveChange(false)}
          oninput={(event) => onDraftChange(event.currentTarget.value)}
          class="min-h-8 min-w-0 flex-1 bg-transparent text-foreground"
        />
        {#if active && !draft.trim()}
          <span
            class="project-list-add-row-caret pointer-events-none absolute left-2 top-1/2 h-4 w-px -translate-y-1/2 bg-foreground"
            aria-hidden="true"
          ></span>
        {/if}
        {#if draft.trim()}
          <button
            type="submit"
            class="flex h-7 shrink-0 cursor-pointer items-center justify-center rounded-md border border-border bg-card px-2 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            {t("projects.list.saveWithEnter")}
          </button>
        {/if}
      </div>
    </div>
  </form>
</div>
