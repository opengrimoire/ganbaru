<!--
  @component
  The replies addressed to a thread's work that wait for the run, failed to reach it, or changed
  after it received them, as a compact list attached to the top of the composer. A waiting reply
  can be steered into the current turn now, held back or resumed, reordered, edited in the composer,
  or removed from the queue; while one is edited, its row offers Save and Discard. A reply that
  started its own work is only listed while that work can stop: its work already holds its text, so
  removing it stops that work.
-->
<script lang="ts">
  import { tick } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CornerDownRight from "@lucide/svelte/icons/corner-down-right";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import ListEnd from "@lucide/svelte/icons/list-end";
  import Pause from "@lucide/svelte/icons/pause";
  import Pencil from "@lucide/svelte/icons/pencil";
  import PencilLine from "@lucide/svelte/icons/pencil-line";
  import Play from "@lucide/svelte/icons/play";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Trash from "@lucide/svelte/icons/trash";
  import type { ChatAssignmentInputId } from "$lib/chat/contracts";
  import {
    pendingInputCanSteerNow,
    pendingInputReorderable,
    pendingInputStartsWork,
    type PendingInputRow,
  } from "$lib/chat/composer/pending-inputs";
  import MoveStepMenuItems from "$lib/components/ui/MoveStepMenuItems.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { createSidebarOrderDrag } from "$lib/utils/sidebar-order-drag.svelte";

  let {
    rows,
    editingInputId,
    busyInputId,
    canEdit,
    onEdit,
    onCancel,
    onSave,
    onDiscard,
    onSetHeld,
    onSteer,
    onReorder,
  }: {
    rows: readonly PendingInputRow[];
    /** The waiting reply the composer edits, if any. */
    editingInputId: ChatAssignmentInputId | null;
    /** The reply an action is in flight for, if any. */
    busyInputId: ChatAssignmentInputId | null;
    /** Whether the full text of a reply is loaded, so the composer can edit it. */
    canEdit: (row: PendingInputRow) => boolean;
    onEdit: (row: PendingInputRow) => void;
    onCancel: (row: PendingInputRow) => void;
    onSave: () => void;
    onDiscard: () => void;
    /** Holds a waiting reply back from delivery, or resumes it. */
    onSetHeld: (row: PendingInputRow, held: boolean) => void;
    /** Sends a waiting reply into the current turn now. */
    onSteer: (row: PendingInputRow) => void;
    /** Saves the waiting replies in a new delivery order. */
    onReorder: (orderedInputIds: string[]) => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  let menuInputId = $state<ChatAssignmentInputId | null>(null);
  let menuAnchor = $state<HTMLButtonElement | undefined>();
  const menuRow = $derived(rows.find((row) => row.input.inputId === menuInputId) ?? null);
  const idle = $derived(editingInputId === null && busyInputId === null);

  const drag = createSidebarOrderDrag({
    getIds: () => rows.filter(pendingInputReorderable).map((row) => row.input.inputId),
    enabled: () => editingInputId === null && busyInputId === null,
    save: (orderedIds) => onReorder(orderedIds),
    description: "chat waiting replies",
  });
  $effect(() => drag.reset);

  function routingLabel(row: PendingInputRow): string {
    switch (row.input.routingKind) {
      case "steer": return t("chat.composer.pendingInputs.steer");
      case "queued_continuation": return t("chat.composer.pendingInputs.queued");
      case "follow_up": return t("chat.composer.pendingInputs.followUp");
    }
  }

  function statusLabel(row: PendingInputRow): string {
    switch (row.status) {
      case "pending": return routingLabel(row);
      case "failed": return t("chat.composer.pendingInputs.failed");
      case "editedAfterDelivery": return t("chat.composer.pendingInputs.editedAfterDelivery");
    }
  }

  function previewText(row: PendingInputRow): string {
    return row.input.previewTruncated ? `${row.input.preview}…` : row.input.preview;
  }

  async function openMenu(row: PendingInputRow, trigger: HTMLButtonElement): Promise<void> {
    if (menuInputId === row.input.inputId) {
      closeMenu();
      return;
    }
    menuAnchor = trigger;
    menuInputId = row.input.inputId;
    await tick();
    document.querySelector<HTMLElement>("[data-chat-queued-reply-menu] [role='menuitem']:not([aria-disabled='true'])")?.focus();
  }

  function closeMenu(): void {
    menuInputId = null;
  }

  /** Closes the menu, then applies `action` to the reply it was open for. */
  function runMenuAction(action: (row: PendingInputRow) => void): void {
    const row = menuRow;
    closeMenu();
    if (row) action(row);
  }
</script>

<section class="queued-replies" aria-label={t("chat.composer.pendingInputs.title")} data-chat-pending-inputs>
  <ul>
    {#each rows as row (row.input.inputId)}
      {@const inputId = row.input.inputId}
      {@const editing = editingInputId === inputId}
      {@const busy = busyInputId === inputId}
      {@const startsWork = pendingInputStartsWork(row.input)}
      {@const reorderable = pendingInputReorderable(row)}
      {@const held = row.status === "pending" && row.input.held}
      {@const status = editing ? t("chat.composer.pendingInputs.editing") : statusLabel(row)}
      {#if reorderable}
        <li class:editing data-input-status={row.status} data-input-held={held ? "" : undefined} {@attach drag.item(inputId)}>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span class="queued-copy reorderable" onpointerdown={(event) => drag.pointerDown(event, inputId)}>
            <span class="queued-icon" data-app-tooltip={status}>{#if editing}<PencilLine size={14} />{:else if row.input.routingKind === "steer"}<CornerDownRight size={14} />{:else}<ListEnd size={14} />{/if}</span>
            <span class="queued-preview">{previewText(row)}</span>
          </span>
          {#if editing}
            <span class="queued-actions">
              <button type="button" class="queued-text-action" data-pending-action="save" disabled={busy} onclick={onSave}>{t("chat.composer.pendingInputs.save")}</button>
              <button type="button" class="queued-text-action" data-pending-action="discard" disabled={busy} onclick={onDiscard}>{t("chat.composer.pendingInputs.discard")}</button>
            </span>
          {:else}
            <span class="queued-actions">
              {#if held}
                <button type="button" class="on-hold-chip" data-pending-action="resume-chip" disabled={!idle} aria-label={t("chat.composer.pendingInputs.resumeReply", row.input.preview)} data-app-tooltip={t("chat.composer.pendingInputs.resume")} onclick={() => onSetHeld(row, false)}><Pause size={12} /><span>{t("chat.composer.pendingInputs.onHold")}</span></button>
              {/if}
              {#if pendingInputCanSteerNow(row)}
                <button type="button" class="queued-text-action" data-pending-action="steer" disabled={!idle} aria-label={t("chat.composer.pendingInputs.steerReply", row.input.preview)} data-app-tooltip={t("chat.composer.pendingInputs.steerNowTooltip")} onclick={() => onSteer(row)}><CornerDownRight size={13} /><span>{t("chat.composer.pendingInputs.steerNow")}</span></button>
              {/if}
              <button type="button" class="queued-icon-action" data-pending-action="cancel" disabled={!idle} aria-label={t("chat.composer.pendingInputs.cancelReply", row.input.preview)} data-app-tooltip={t("chat.composer.pendingInputs.remove")} onclick={() => onCancel(row)}><Trash size={14} /></button>
              <button type="button" class="queued-icon-action" data-pending-action="more" disabled={!idle} aria-label={t("chat.composer.pendingInputs.moreActions", row.input.preview)} data-app-tooltip={t("chat.moreActions")} aria-haspopup="menu" aria-expanded={menuInputId === inputId} onclick={(event) => { void openMenu(row, event.currentTarget); }}><Ellipsis size={14} /></button>
            </span>
          {/if}
        </li>
      {:else}
        <li data-input-status={row.status}>
          <span class="queued-copy">
            <span class="queued-icon" data-app-tooltip={status}>{#if row.status === "failed"}<CircleAlert size={14} />{:else if row.status === "editedAfterDelivery"}<Pencil size={14} />{:else}<Sparkles size={14} />{/if}</span>
            <span class="queued-preview">{previewText(row)}</span>
          </span>
          {#if row.status === "pending" && startsWork}
            <span class="queued-actions">
              <button type="button" class="queued-icon-action" data-pending-action="cancel" disabled={!idle} aria-label={t("chat.composer.pendingInputs.cancelWork", row.input.preview)} data-app-tooltip={t("chat.composer.pendingInputs.stopWork")} onclick={() => onCancel(row)}><Trash size={14} /></button>
            </span>
          {/if}
        </li>
      {/if}
    {/each}
  </ul>
</section>

{#if menuRow}
  {@const row = menuRow}
  <div
    use:portal
    use:anchoredPopover={{ anchor: menuAnchor, onOutside: closeMenu, horizontalAlign: "end" }}
    class="surface-floating fixed z-90 flex w-floating-sm flex-col overflow-hidden"
    role="menu"
    tabindex="-1"
    aria-label={t("chat.composer.pendingInputs.moreActions", row.input.preview)}
    data-app-floating-surface
    data-chat-queued-reply-menu
    onkeydown={(event) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      closeMenu();
      menuAnchor?.focus();
    }}
  >
    <div class="surface-floating-body flex flex-col">
      <button type="button" role="menuitem" class="menu-item" data-pending-action="edit" aria-disabled={canEdit(row) ? undefined : "true"} data-app-tooltip={canEdit(row) ? undefined : t("chat.composer.pendingInputs.fullTextUnavailable")} onclick={() => { if (canEdit(row)) runMenuAction(onEdit); }}>
        <Pencil />
        <span>{t("chat.composer.pendingInputs.edit")}</span>
      </button>
      {#if row.input.held}
        <button type="button" role="menuitem" class="menu-item" data-pending-action="resume" onclick={() => runMenuAction((target) => onSetHeld(target, false))}>
          <Play />
          <span>{t("chat.composer.pendingInputs.resume")}</span>
        </button>
      {:else}
        <button type="button" role="menuitem" class="menu-item" data-pending-action="hold" onclick={() => runMenuAction((target) => onSetHeld(target, true))}>
          <Pause />
          <span>{t("chat.composer.pendingInputs.hold")}</span>
        </button>
      {/if}
      <MoveStepMenuItems
        canMoveUp={drag.canMove(row.input.inputId, -1)}
        canMoveDown={drag.canMove(row.input.inputId, 1)}
        onMove={(step) => runMenuAction((target) => drag.move(target.input.inputId, step))}
        onClose={() => undefined}
      />
    </div>
  </div>
{/if}

<style>
  .queued-replies { container-type: inline-size; margin: 0 0.75rem -0.6rem; border: 1px solid var(--border); border-bottom: 0; border-radius: 0.75rem 0.75rem 0 0; background: var(--card); padding: 0.2rem 0.3rem 0.8rem; color: var(--muted-foreground); }
  ul { display: grid; }
  li { display: flex; min-width: 0; align-items: center; gap: 0.35rem; border-radius: 0.45rem; background: var(--card); padding: 0 0.2rem; }
  li.editing .queued-preview { color: var(--primary); }
  .queued-copy { display: flex; min-width: 0; min-height: 1.85rem; flex: 1 1 0%; align-items: center; gap: 0.45rem; }
  .queued-copy.reorderable { cursor: grab; touch-action: pan-y; }
  .queued-icon { display: inline-flex; flex: 0 0 auto; color: var(--muted-foreground); }
  li[data-input-status="failed"] .queued-icon { color: var(--destructive); }
  .queued-preview { overflow: hidden; color: var(--foreground); text-overflow: ellipsis; white-space: nowrap; font-size: calc(0.75rem * var(--type-scale)); }
  li[data-input-held] .queued-preview, li[data-input-status="failed"] .queued-preview, li[data-input-status="editedAfterDelivery"] .queued-preview { color: var(--muted-foreground); }
  .queued-actions { display: flex; flex: 0 0 auto; align-items: center; gap: 0.1rem; }
  .queued-actions button { display: inline-flex; min-height: 1.6rem; align-items: center; gap: 0.25rem; border-radius: 0.4rem; font-size: calc(0.7rem * var(--type-scale)); font-weight: 500; }
  .queued-text-action { padding: 0 0.4rem; }
  .queued-icon-action { justify-content: center; min-width: 1.6rem; }
  .queued-actions button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .queued-actions button:disabled { opacity: 0.45; }
  .on-hold-chip { margin-right: 0.2rem; background: color-mix(in srgb, var(--primary) 14%, transparent); padding: 0 0.5rem; color: var(--primary); }
  .queued-actions .on-hold-chip:hover:not(:disabled) { background: color-mix(in srgb, var(--primary) 22%, transparent); color: var(--primary); }
  @container (max-width: 26rem) { .queued-text-action span { display: none; } }
  @media (pointer: coarse) { .queued-actions button { min-height: 2.75rem; } .queued-icon-action { min-width: 2.75rem; } }
</style>
