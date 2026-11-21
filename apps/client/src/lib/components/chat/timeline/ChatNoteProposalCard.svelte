<script lang="ts">
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import {
    acceptChatNoteProposal,
    dismissChatNoteProposal,
    readChatNoteProposalStates,
  } from "$lib/api/chat/channel-notes";
  import { channelNoteDraftProblem } from "$lib/chat/channel-notes/presentation";
  import {
    CHAT_MAX_CHANNEL_NOTE_CHARS,
    type ChatConversationItemId,
    type ChatNoteProposalState,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { requestConfirmation } from "$lib/components/ui/confirmation.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";

  /**
   * An agent's suggested channel note. Only a person saves it, as suggested or reworded, or dismisses it;
   * either outcome holds on every linked device.
   */
  let {
    proposalItemId,
    body,
  }: {
    proposalItemId: ChatConversationItemId;
    /** The suggested text, which the message above the card already shows. */
    body: string;
  } = $props();

  const { t } = getLocalization();
  const chat = useChat();
  let proposalState = $state<ChatNoteProposalState | null>(null);
  let pending = $state(false);
  let error = $state<string | null>(null);
  let editing = $state(false);
  let draft = $state("");

  const draftProblem = $derived(channelNoteDraftProblem(draft, 0, false));

  $effect(() => {
    const _version = chat.channelNotesVersion;
    void loadState(proposalItemId);
  });

  async function loadState(itemId: ChatConversationItemId): Promise<void> {
    try {
      const [read] = await readChatNoteProposalStates([itemId]);
      if (itemId === proposalItemId) proposalState = read?.state ?? "pending";
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.proposalAction"));
    }
  }

  /** Runs one settling command and tells open note views that the channel notes changed. */
  async function settle(command: () => Promise<unknown>, outcome: ChatNoteProposalState): Promise<void> {
    if (pending) return;
    pending = true;
    error = null;
    const channelId = chat.selectedChannelId;
    try {
      await command();
      proposalState = outcome;
      editing = false;
      if (channelId) chat.channelNotesChanged(channelId);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.proposalAction"));
      if (channelId) chat.channelNotesChanged(channelId);
    } finally {
      pending = false;
    }
  }

  function save(): Promise<void> {
    return settle(() => acceptChatNoteProposal({ clientCommandId: crypto.randomUUID(), itemId: proposalItemId }), "saved");
  }

  function startEdit(): void {
    draft = [...body.trim()].slice(0, CHAT_MAX_CHANNEL_NOTE_CHARS).join("");
    editing = true;
    error = null;
  }

  function saveEdited(): Promise<void> {
    if (draftProblem !== null) return Promise.resolve();
    const edited = draft;
    return settle(
      () => acceptChatNoteProposal({ clientCommandId: crypto.randomUUID(), itemId: proposalItemId, body: edited }),
      "saved",
    );
  }

  async function dismiss(): Promise<void> {
    if (pending) return;
    const confirmed = await requestConfirmation({
      message: t("chat.channelNotes.proposal.dismissConfirm"),
      confirmLabel: t("chat.channelNotes.proposal.dismiss"),
    });
    if (!confirmed) return;
    await settle(() => dismissChatNoteProposal({ clientCommandId: crypto.randomUUID(), itemId: proposalItemId }), "dismissed");
  }
</script>

<div class="note-proposal" data-note-proposal data-note-proposal-state={proposalState ?? "loading"}>
  <div class="proposal-heading">
    <StickyNote size={14} />
    <strong>{t("chat.channelNotes.proposal.title")}</strong>
  </div>
  {#if proposalState === "saved"}
    <p class="proposal-status" role="status">{t("chat.channelNotes.proposal.saved")}</p>
  {:else if proposalState === "dismissed"}
    <p class="proposal-status" role="status">{t("chat.channelNotes.proposal.dismissed")}</p>
  {:else if proposalState === "pending"}
    {#if editing}
      <textarea
        class="field min-h-16 resize-none text-[0.8rem]"
        rows="3"
        bind:value={draft}
        aria-label={t("chat.channelNotes.proposal.editAndSave")}
        aria-invalid={draftProblem === "too_long" ? "true" : undefined}
        disabled={pending}
      ></textarea>
      {#if draftProblem === "too_long"}<p class="proposal-problem" role="alert">{t("chat.channelNotes.tooLong", CHAT_MAX_CHANNEL_NOTE_CHARS)}</p>{/if}
      <div class="proposal-actions">
        <button type="button" class="proposal-primary" disabled={pending || draftProblem !== null} onclick={() => void saveEdited()}>{t("chat.channelNotes.save")}</button>
        <button type="button" class="proposal-secondary" disabled={pending} onclick={() => { editing = false; }}>{t("chat.channelNotes.cancel")}</button>
      </div>
    {:else}
      <div class="proposal-actions">
        <button type="button" class="proposal-primary" disabled={pending} onclick={() => void save()}>{t("chat.channelNotes.proposal.save")}</button>
        <button type="button" class="proposal-secondary" disabled={pending} onclick={startEdit}>{t("chat.channelNotes.proposal.editAndSave")}</button>
        <button type="button" class="proposal-secondary" disabled={pending} onclick={() => void dismiss()}>{t("chat.channelNotes.proposal.dismiss")}</button>
      </div>
    {/if}
  {/if}
  {#if error}<Notice class="proposal-error" message={error} />{/if}
</div>

<style>
  .note-proposal { display:grid; gap:0.4rem; max-width:32rem; margin-top:0.4rem; border:1px solid var(--border); border-radius:0.45rem; padding:0.55rem 0.65rem; font-size:calc(0.78rem * var(--type-scale)); }
  .proposal-heading { display:flex; align-items:center; gap:0.4rem; color:var(--foreground); }
  .proposal-heading strong { font-weight:600; }
  .proposal-status { color:var(--muted-foreground); }
  .proposal-problem { color:var(--destructive); }
  .proposal-actions { display:flex; flex-wrap:wrap; gap:0.35rem; }
  .proposal-primary,.proposal-secondary { min-height:1.9rem; border-radius:var(--floating-item-radius); padding:0.2rem 0.7rem; font-weight:600; }
  .proposal-primary { background:var(--primary); color:var(--primary-foreground); }
  .proposal-secondary { border:1px solid var(--border); color:var(--foreground); }
  .proposal-secondary:hover:not(:disabled) { background:var(--accent); }
  .proposal-primary:disabled,.proposal-secondary:disabled { cursor:not-allowed; opacity:0.5; }
  @media (pointer:coarse) { .proposal-primary,.proposal-secondary { min-height:2.75rem; } }
</style>
