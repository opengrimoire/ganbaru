<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesMentionOptionDomId } from "$lib/notes/editor/accessibility";
  import {
    notesMentionTargetKey,
    type NotesMentionTarget,
  } from "$lib/notes/rich-text/core";
  import NotesMentionTargetIcon from "./NotesMentionTargetIcon.svelte";

  let {
    menuId,
    blockId,
    targets,
    activeIndex,
    onSelect,
  }: {
    menuId: string;
    blockId: string;
    targets: NotesMentionTarget[];
    activeIndex: number;
    onSelect: (target: NotesMentionTarget) => void;
  } = $props();

  const { t } = getLocalization();
</script>

<div
  id={menuId}
  class="surface-floating surface-floating-body absolute left-1 top-full z-30 mt-1 w-floating overflow-hidden"
  role="listbox"
  aria-label={t("notes.mentionMenu")}
  aria-live="polite"
>
  {#if targets.length === 0}
    <div class="px-2 py-2 text-muted-foreground" role="status">
      {t("notes.noMentionResults")}
    </div>
  {:else}
    {#each targets as target, index (notesMentionTargetKey(target))}
      <button
        id={notesMentionOptionDomId(blockId, index)}
        type="button"
        class="menu-item min-w-0"
        data-highlighted={index === activeIndex ? "" : undefined}
        role="option"
        aria-selected={index === activeIndex}
        onclick={() => onSelect(target)}
      >
        <NotesMentionTargetIcon {target} />
        <span class="min-w-0 flex-1">
          <span class="block truncate text-foreground">{target.title}</span>
          {#if target.subtitle}
            <span class="block truncate text-panel-detail text-muted-foreground">{target.subtitle}</span>
          {/if}
        </span>
      </button>
    {/each}
  {/if}
</div>
