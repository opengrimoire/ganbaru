<!--
  @component
  The replies addressed to a thread's work that wait for the run, failed to reach it, or changed
  after it received them. A waiting reply can be edited in the composer or cancelled; while one is
  edited, the list offers Save and Discard for it. A reply that started its own work is only
  listed while that work can stop: its work already holds its text, so cancelling it stops that work.
-->
<script lang="ts">
  import type { ChatAssignmentInputId, ChatAssignmentInputRoutingKind } from "$lib/chat/contracts";
  import { pendingInputStartsWork, type PendingInputRow, type PendingInputStatus } from "$lib/chat/composer/pending-inputs";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    rows,
    editingInputId,
    busyInputId,
    canEdit,
    onEdit,
    onCancel,
    onSave,
    onDiscard,
  }: {
    rows: readonly PendingInputRow[];
    /** The waiting reply the composer edits, if any. */
    editingInputId: ChatAssignmentInputId | null;
    /** The reply an edit or cancellation is in flight for, if any. */
    busyInputId: ChatAssignmentInputId | null;
    /** Whether the full text of a reply is loaded, so the composer can edit it. */
    canEdit: (row: PendingInputRow) => boolean;
    onEdit: (row: PendingInputRow) => void;
    onCancel: (row: PendingInputRow) => void;
    onSave: () => void;
    onDiscard: () => void;
  } = $props();

  const { t } = getLocalization();

  function routingLabel(kind: ChatAssignmentInputRoutingKind): string {
    switch (kind) {
      case "steer": return t("chat.composer.pendingInputs.steer");
      case "queued_continuation": return t("chat.composer.pendingInputs.queued");
      case "follow_up": return t("chat.composer.pendingInputs.followUp");
    }
  }

  function statusLabel(row: PendingInputRow): string {
    const status: PendingInputStatus = row.status;
    switch (status) {
      case "pending": return routingLabel(row.input.routingKind);
      case "failed": return t("chat.composer.pendingInputs.failed");
      case "editedAfterDelivery": return t("chat.composer.pendingInputs.editedAfterDelivery");
    }
  }

  function previewText(row: PendingInputRow): string {
    return row.input.previewTruncated ? `${row.input.preview}…` : row.input.preview;
  }
</script>

<section class="pending-inputs" aria-label={t("chat.composer.pendingInputs.title")} data-chat-pending-inputs>
  <strong class="pending-title">{t("chat.composer.pendingInputs.title")}</strong>
  <ul>
    {#each rows as row (row.input.inputId)}
      {@const editing = editingInputId === row.input.inputId}
      {@const busy = busyInputId === row.input.inputId}
      {@const startsWork = pendingInputStartsWork(row.input)}
      <li class:editing data-input-status={row.status}>
        <span class="pending-copy">
          <small>{editing ? t("chat.composer.pendingInputs.editing") : statusLabel(row)}</small>
          <span class="pending-preview">{previewText(row)}</span>
          {#if row.status === "pending" && !startsWork && !editing && !canEdit(row)}<small>{t("chat.composer.pendingInputs.fullTextUnavailable")}</small>{/if}
        </span>
        {#if row.status === "pending" && startsWork}
          <span class="pending-actions">
            <button type="button" data-pending-action="cancel" disabled={busy || editingInputId !== null} aria-label={t("chat.composer.pendingInputs.cancelWork", row.input.preview)} onclick={() => onCancel(row)}>{t("chat.composer.pendingInputs.cancel")}</button>
          </span>
        {:else if row.status === "pending"}
          <span class="pending-actions">
            {#if editing}
              <button type="button" data-pending-action="save" disabled={busy} onclick={onSave}>{t("chat.composer.pendingInputs.save")}</button>
              <button type="button" data-pending-action="discard" disabled={busy} onclick={onDiscard}>{t("chat.composer.pendingInputs.discard")}</button>
            {:else}
              <button type="button" data-pending-action="edit" disabled={busy || editingInputId !== null || !canEdit(row)} aria-label={t("chat.composer.pendingInputs.editReply", row.input.preview)} onclick={() => onEdit(row)}>{t("chat.composer.pendingInputs.edit")}</button>
              <button type="button" data-pending-action="cancel" disabled={busy || editingInputId !== null} aria-label={t("chat.composer.pendingInputs.cancelReply", row.input.preview)} onclick={() => onCancel(row)}>{t("chat.composer.pendingInputs.cancel")}</button>
            {/if}
          </span>
        {/if}
      </li>
    {/each}
  </ul>
</section>

<style>
  .pending-inputs { display: grid; gap: 0.25rem; margin: 0 0.35rem 0.35rem; border-radius: 0.55rem; background: color-mix(in srgb, var(--accent) 58%, transparent); padding: 0.35rem 0.55rem; color: var(--muted-foreground); }
  .pending-title { font-size: calc(0.66rem * var(--type-scale)); font-weight: 600; }
  ul { display: grid; gap: 0.2rem; }
  li { display: flex; min-width: 0; align-items: center; gap: 0.5rem; }
  li.editing .pending-preview { color: var(--foreground); }
  .pending-copy { display: grid; min-width: 0; flex: 1 1 0%; }
  .pending-copy small { font-size: calc(0.6rem * var(--type-scale)); }
  li[data-input-status="failed"] .pending-copy small:first-child { color: var(--destructive); }
  .pending-preview { overflow: hidden; color: var(--foreground); text-overflow: ellipsis; white-space: nowrap; font-size: calc(0.7rem * var(--type-scale)); }
  .pending-actions { display: flex; flex: 0 0 auto; gap: 0.15rem; }
  .pending-actions button { min-height: 1.75rem; border-radius: 0.4rem; padding: 0.2rem 0.45rem; font-size: calc(0.66rem * var(--type-scale)); font-weight: 500; }
  .pending-actions button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .pending-actions button:disabled { opacity: 0.45; }
  @media (pointer: coarse) { .pending-actions button { min-height: 2.75rem; } }
</style>
