<script lang="ts">
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import { NOTES_SHARE_ROLES, type NotesShareRole } from "$lib/components/contacts/model";
  import NotesLocationPath from "$lib/components/notes/NotesLocationPath.svelte";
  import { notesCaretMenuPosition } from "$lib/components/notes/blocks/caret-menu-position";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesMentionOptionDomId } from "$lib/notes/editor/accessibility";
  import { notesDateMentionDetail, type NotesMentionTarget } from "$lib/notes/rich-text/core";
  import {
    notesMentionRowKey,
    type NotesMentionGroup,
    type NotesMentionGroupKind,
    type NotesMentionRow,
  } from "$lib/notes/rich-text/mention-groups";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { tick } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import AtSign from "@lucide/svelte/icons/at-sign";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import UserRoundPlus from "@lucide/svelte/icons/user-round-plus";
  import NotesMentionTargetIcon from "./NotesMentionTargetIcon.svelte";

  /**
   * The editor mention menu: grouped suggestions anchored at the typed at sign, and an invitation panel that keeps
   * the final layout while sending invitations is not available yet.
   */
  let {
    menuId,
    blockId,
    anchor,
    anchorOffset,
    query,
    groups,
    activeIndex,
    localUserId = null,
    inviting = false,
    onSelect,
    onHighlight,
    onClose,
    onInviteBack,
    onInviteDismiss,
  }: {
    menuId: string;
    blockId: string;
    anchor: HTMLElement | null;
    /** Plain-text offset of the at sign. */
    anchorOffset: number;
    query: string;
    groups: readonly NotesMentionGroup[];
    activeIndex: number;
    /** Person shown with the device profile picture instead of a generic glyph. */
    localUserId?: string | null;
    /** Shows the invitation panel in place of the suggestions. */
    inviting?: boolean;
    onSelect: (row: NotesMentionRow) => void;
    onHighlight: (index: number) => void;
    onClose: () => void;
    /** Returns from the invitation panel to the suggestions. */
    onInviteBack: () => void;
    /** Closes the invitation panel without returning to the editor. */
    onInviteDismiss: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  let scrollArea: HTMLDivElement | null = $state(null);
  let inviteQuery = $state("");
  let inviteMessage = $state("");
  let inviteRole = $state<NotesShareRole>("full");

  const indexedGroups = $derived.by(() => {
    let offset = 0;
    return groups.map((group) => {
      const start = offset;
      offset += group.rows.length;
      return { ...group, start };
    });
  });
  const roleOptions = $derived([...NOTES_SHARE_ROLES].reverse().map((role) => ({
    value: role,
    label: roleLabel(role),
    summary: roleDescription(role),
  })));

  $effect(() => {
    if (inviting) inviteQuery = query;
  });

  $effect(() => {
    void activeIndex;
    void tick().then(() => {
      const active = scrollArea?.querySelector<HTMLElement>("[data-highlighted]");
      if (!active || !scrollArea) return;
      const item = active.getBoundingClientRect();
      const viewport = scrollArea.getBoundingClientRect();
      if (item.top < viewport.top) scrollArea.scrollTop -= viewport.top - item.top;
      else if (item.bottom > viewport.bottom) scrollArea.scrollTop += item.bottom - viewport.bottom;
    });
  });

  function groupLabel(kind: NotesMentionGroupKind): string {
    switch (kind) {
      case "date": return t("notes.commentMentionGroupDate");
      case "people": return t("notes.commentMentionGroupPeople");
      case "pages": return t("notes.commentMentionGroupPages");
      case "projects": return t("notes.mentionGroupProjects");
      case "calendar": return t("notes.mentionGroupCalendar");
      case "focus": return t("notes.mentionGroupFocus");
      case "music": return t("notes.mentionGroupMusic");
    }
  }

  function roleLabel(role: NotesShareRole): string {
    switch (role) {
      case "view": return t("notes.shareRoleView");
      case "comment": return t("notes.shareRoleComment");
      case "edit": return t("notes.shareRoleEdit");
      case "full": return t("notes.shareRoleFull");
    }
  }

  function roleDescription(role: NotesShareRole): string {
    switch (role) {
      case "view": return t("notes.shareRoleViewDescription");
      case "comment": return t("notes.shareRoleCommentDescription");
      case "edit": return t("notes.shareRoleEditDescription");
      case "full": return t("notes.shareRoleFullDescription");
    }
  }

  function isShareRole(value: string): value is NotesShareRole {
    return NOTES_SHARE_ROLES.some((role) => role === value);
  }

  /** Route segments under a page or database title, with untitled ancestors labeled. */
  function targetLocation(target: NotesMentionTarget): string[] {
    if (target.kind !== "page" && target.kind !== "database") return [];
    return (target.location ?? []).map((title) => title || t("notes.untitled"));
  }

  /** Short context after a suggestion when its section heading does not already say it. */
  function targetDetail(target: NotesMentionTarget): string | null {
    switch (target.kind) {
      case "date": return notesDateMentionDetail(target, localization.locale);
      case "user": return target.id === localUserId ? t("contacts.you") : null;
      case "project_task":
      case "pomodoro_run": return target.subtitle ?? null;
      default: return null;
    }
  }

  function focusOnMount(node: HTMLElement) {
    void tick().then(() => node.focus({ preventScroll: true }));
  }

  function handleInviteKey(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    onInviteBack();
  }
</script>

{#snippet rowContent(row: NotesMentionRow)}
  {#if row.kind === "target"}
    {@const target = row.target}
    {@const location = targetLocation(target)}
    {@const detail = targetDetail(target)}
    {#if target.kind === "user" && target.id === localUserId}
      <LocalPersonAvatar size={20} />
    {:else}
      <NotesMentionTargetIcon {target} />
    {/if}
    {#if location.length > 0}
      <span class="grid min-w-0 flex-1">
        <span class="truncate">{target.title}</span>
        <NotesLocationPath segments={location} />
      </span>
    {:else}
      <span class="min-w-0 flex-1 truncate">{target.title}</span>
    {/if}
    {#if detail}
      <span class="max-w-[45%] shrink-0 truncate text-panel-detail text-muted-foreground">{detail}</span>
    {/if}
  {:else if row.kind === "invite"}
    <UserRoundPlus class="size-4 shrink-0" aria-hidden="true" />
    <span class="min-w-0 flex-1 truncate">{t("notes.mentionInvite")}</span>
  {:else}
    <Ellipsis class="size-4 shrink-0" aria-hidden="true" />
    <span class="min-w-0 flex-1 truncate">{t("notes.databaseSourceShowMore", row.hiddenCount)}</span>
  {/if}
{/snippet}

<div
  use:notesCaretMenuPosition={{
    editor: anchor,
    offset: anchorOffset,
    scrollArea: () => scrollArea,
    onOutside: inviting ? onInviteDismiss : onClose,
    keepOpenFor: inviting ? (target) => target.closest("[data-app-floating-surface]") !== null : undefined,
  }}
  id={menuId}
  class="surface-floating z-50 flex max-h-[min(22rem,70vh)] w-floating-lg flex-col overflow-hidden"
  role={inviting ? "dialog" : "listbox"}
  aria-label={inviting ? t("notes.mentionInviteTitle") : t("notes.mentionMenu")}
  data-app-floating-surface
  tabindex="-1"
  onkeydown={inviting ? handleInviteKey : undefined}
  onmousedown={(event) => { if (!inviting) event.preventDefault(); }}
>
  {#if inviting}
    <div class="flex shrink-0 items-center gap-1 border-b border-border px-1.5 py-1">
      <button
        type="button"
        class="flex size-7 shrink-0 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label={t("notes.mentionInviteBack")}
        onclick={onInviteBack}
      >
        <ArrowLeft class="size-4" aria-hidden="true" />
      </button>
      <span class="min-w-0 truncate font-medium">{t("notes.mentionInviteTitle")}</span>
    </div>
    <div bind:this={scrollArea} class="min-h-0 overflow-y-auto overscroll-contain" use:scrollEdgeFadeAction>
      <div class="surface-floating-body flex flex-col gap-2">
        <div class="flex min-w-0 items-center gap-1.5">
          <input
            use:focusOnMount
            class="field min-w-0 flex-1"
            aria-label={t("contacts.picker.searchContacts")}
            placeholder={t("contacts.picker.searchContacts")}
            bind:value={inviteQuery}
          />
          <Select
            value={inviteRole}
            options={roleOptions}
            ariaLabel={t("notes.mentionInviteAccess")}
            appearance="quiet"
            showSelectedSummary={false}
            onChange={(value) => { if (isShareRole(value)) inviteRole = value; }}
          />
        </div>
        <textarea
          class="field min-h-16 resize-none"
          aria-label={t("contacts.invite.message")}
          placeholder={t("contacts.invite.messagePlaceholder")}
          bind:value={inviteMessage}
        ></textarea>
        <div class="flex justify-end">
          <button type="button" class="invite-button control-unavailable" aria-disabled="true">{t("notes.mentionInviteSend")}</button>
        </div>
      </div>
    </div>
  {:else}
    <div class="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2 text-muted-foreground">
      <AtSign class="size-4 shrink-0" aria-hidden="true" />
      <span class={["truncate", query && "text-foreground"]}>{query || t("notes.slashSearch")}</span>
    </div>
    <div bind:this={scrollArea} class="min-h-0 overflow-y-auto overscroll-contain" use:scrollEdgeFadeAction>
      <div class="surface-floating-body">
        {#if groups.length === 0}
          <div class="px-2 py-1.5 text-muted-foreground" role="status">{t("notes.noMentionResults")}</div>
        {:else}
          {#each indexedGroups as group (group.kind)}
            <div role="group" aria-labelledby={`${menuId}-${group.kind}`}>
              <div id={`${menuId}-${group.kind}`} class="menu-label">{groupLabel(group.kind)}</div>
              {#each group.rows as row, offset (notesMentionRowKey(row))}
                {@const index = group.start + offset}
                <button
                  id={notesMentionOptionDomId(blockId, index)}
                  type="button"
                  tabindex="-1"
                  class={["menu-item min-w-0", row.kind !== "target" && "text-muted-foreground"]}
                  data-highlighted={index === activeIndex ? "" : undefined}
                  role="option"
                  aria-selected={index === activeIndex}
                  onpointermove={() => { if (index !== activeIndex) onHighlight(index); }}
                  onclick={() => onSelect(row)}
                >
                  {@render rowContent(row)}
                </button>
              {/each}
            </div>
          {/each}
        {/if}
      </div>
    </div>
    <button class="flex min-h-(--panel-row-height) w-full shrink-0 items-center justify-between border-t border-border px-3 py-2 text-left hover:bg-accent" type="button" tabindex="-1" onclick={onClose}>
      {t("notes.slashClose")}<kbd class="text-panel-detail text-muted-foreground">Esc</kbd>
    </button>
  {/if}
</div>

<style>
  .invite-button {
    display: inline-flex;
    min-height: 2rem;
    align-items: center;
    justify-content: center;
    border-radius: var(--floating-item-radius);
    padding-inline: 0.75rem;
    background: var(--primary);
    color: var(--primary-foreground);
    font-weight: 600;
  }
</style>
