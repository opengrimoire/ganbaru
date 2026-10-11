<script lang="ts">
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import NotesMentionTargetIcon from "$lib/components/notes/rich-text/NotesMentionTargetIcon.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type {
    NotesCommentMentionGroup,
    NotesCommentMentionGroupKind,
  } from "$lib/notes/collaboration/comment-mentions";
  import {
    notesDateMentionDetail,
    notesMentionTargetKey,
    type NotesMentionTarget,
  } from "$lib/notes/rich-text/core";

  /** Mention suggestions for a comment, grouped under Date, People, and Link to page headings. */
  let {
    menuId,
    optionId,
    groups,
    activeIndex,
    localUserId = null,
    placement = "below",
    onSelect,
    onHighlight,
  }: {
    menuId: string;
    optionId: (index: number) => string;
    groups: readonly NotesCommentMentionGroup[];
    activeIndex: number;
    /** Person shown with the device profile picture instead of a generic glyph. */
    localUserId?: string | null;
    placement?: "below" | "above";
    onSelect: (target: NotesMentionTarget) => void;
    onHighlight: (index: number) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;

  const indexedGroups = $derived.by(() => {
    let offset = 0;
    return groups.map((group) => {
      const start = offset;
      offset += group.targets.length;
      return { ...group, start };
    });
  });

  function groupLabel(kind: NotesCommentMentionGroupKind): string {
    if (kind === "date") return t("notes.commentMentionGroupDate");
    if (kind === "people") return t("notes.commentMentionGroupPeople");
    return t("notes.commentMentionGroupPages");
  }

  /** The secondary text after a suggestion: the spelled-out date for named days, otherwise the target's context. */
  function targetDetail(target: NotesMentionTarget): string | null {
    return target.kind === "date" ? notesDateMentionDetail(target, localization.locale) : target.subtitle ?? null;
  }
</script>

<div
  id={menuId}
  class={[
    "surface-floating absolute left-0 z-40 w-floating max-w-full overflow-hidden",
    placement === "below" ? "top-full mt-1" : "bottom-full mb-1",
  ]}
  role="listbox"
  tabindex="-1"
  aria-label={t("notes.mentionMenu")}
  onpointerdown={(event) => event.preventDefault()}
>
  <div class="surface-floating-body max-h-72 overflow-y-auto overscroll-contain">
    {#if groups.length === 0}
      <div class="px-2 py-2 text-muted-foreground" role="status">{t("notes.noMentionResults")}</div>
    {:else}
      {#each indexedGroups as group (group.kind)}
        <div role="group" aria-labelledby={`${menuId}-${group.kind}`}>
          <div id={`${menuId}-${group.kind}`} class="menu-label">{groupLabel(group.kind)}</div>
          {#each group.targets as target, offset (notesMentionTargetKey(target))}
            {@const index = group.start + offset}
            {@const detail = targetDetail(target)}
            <button
              id={optionId(index)}
              type="button"
              tabindex="-1"
              class="menu-item min-w-0"
              data-highlighted={index === activeIndex ? "" : undefined}
              role="option"
              aria-selected={index === activeIndex}
              onpointermove={() => { if (index !== activeIndex) onHighlight(index); }}
              onclick={() => onSelect(target)}
            >
              {#if target.kind === "user" && target.id === localUserId}
                <LocalPersonAvatar size={20} />
              {:else}
                <NotesMentionTargetIcon {target} />
              {/if}
              <span class="min-w-0 flex-1 truncate text-foreground">{target.title}</span>
              {#if detail}
                <span class="max-w-[45%] shrink-0 truncate text-panel-detail text-muted-foreground">{detail}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/each}
    {/if}
  </div>
</div>
