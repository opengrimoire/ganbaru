<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import WandSparkles from "@lucide/svelte/icons/wand-sparkles";
  import * as chatApi from "$lib/api/chat";
  import { queuedFollowupDispatchReady } from "$lib/chat/composer/model";
  import type { ChatCommitDraftContextRead, ChatThreadId, ChatTurnId } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import {
    draftReplyText,
    draftTurnOutcome,
    extractDraft,
    type DraftExtraction,
    type DraftKind,
    type DraftRejection,
  } from "$lib/chat/source-control/draft-extraction";
  import { buildDraftPrompt } from "$lib/chat/source-control/draft-prompt";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";

  type ReadyDraft = Extract<DraftExtraction, { ok: true }>;

  interface Props {
    kind: DraftKind;
    /** Identifies the folder and environment; any change drops a pending request. */
    scopeKey: string;
    /** The target field's current text, compared with its text when the draft was requested. */
    fieldValue: string;
    /** Whether the panel allows a request now, for example with staged changes and no running operation. */
    allowed: boolean;
    /** Change request base branch, embedded in the prompt. */
    base?: string;
    /** Change request head branch, embedded in the prompt. */
    head?: string;
    readContext: () => Promise<ChatCommitDraftContextRead>;
    onDraft: (draft: ReadyDraft) => void;
  }

  interface PendingDraft {
    generation: number;
    threadId: ChatThreadId;
    turnId: ChatTurnId;
    fieldValue: string;
  }

  const { kind, scopeKey, fieldValue, allowed, base = "", head = "", readContext, onDraft }: Props = $props();
  const { t } = getLocalization();
  const chat = useChat();
  const LIVE_TURN_STATES = ["pending", "dispatching", "active", "waiting_for_approval", "waiting_for_user_input"];

  let generation = 0;
  let destroyed = false;
  let checking = false;
  let recheck = false;
  let requesting = $state(false);
  let pending = $state<PendingDraft | null>(null);
  let ready = $state<ReadyDraft | null>(null);
  let notice = $state<string | null>(null);

  const thread = $derived(chat.selectedThread);
  const threadIdle = $derived.by(() => {
    const interaction = chat.interaction;
    const latestTurnState = thread?.latestTurnState ?? null;
    return interaction !== null
      && queuedFollowupDispatchReady(interaction.sessionState, latestTurnState)
      && !LIVE_TURN_STATES.includes(latestTurnState ?? "");
  });
  const available = $derived(
    thread !== null
      && thread.workingFolderId !== null
      && thread.workingFolderId === chat.selectedWorkingFolderId
      && threadIdle,
  );
  const busy = $derived(requesting || pending !== null);

  $effect(() => {
    void scopeKey;
    untrack(reset);
  });

  $effect(() => {
    const current = pending;
    const selectedThreadId = thread?.id ?? null;
    void thread?.lastEventSequence;
    void thread?.latestTurnState;
    void chat.interaction?.sessionState;
    if (!current) return;
    untrack(() => {
      if (selectedThreadId !== current.threadId) {
        reset();
        return;
      }
      void check();
    });
  });

  onDestroy(() => {
    destroyed = true;
    generation += 1;
  });

  function reset(): void {
    generation += 1;
    requesting = false;
    pending = null;
    ready = null;
    notice = null;
  }

  function rejection(reason: DraftRejection): string {
    if (reason === "empty") return t("chat.sourceControl.teammateDraftEmpty");
    if (reason === "title_too_long") return t("chat.sourceControl.teammateDraftTitleTooLong");
    return t("chat.sourceControl.teammateDraftTooLong");
  }

  function current(request: number): boolean {
    return !destroyed && request === generation;
  }

  async function request(): Promise<void> {
    if (!available || !allowed || busy) return;
    reset();
    const request = generation;
    const requestedValue = fieldValue;
    requesting = true;
    try {
      const context = await readContext();
      if (!current(request)) return;
      const prompt = buildDraftPrompt(context, {
        instruction: kind === "commit"
          ? t("chat.sourceControl.teammateDraftCommitPrompt")
          : t("chat.sourceControl.teammateDraftChangeRequestPrompt", base, head),
        changedFiles: t("chat.sourceControl.teammateDraftChangedFiles"),
        omittedFiles: (count) => t("chat.sourceControl.teammateDraftOmittedFiles", count),
        subjects: kind === "commit"
          ? t("chat.sourceControl.teammateDraftRecentSubjects")
          : t("chat.sourceControl.teammateDraftRangeSubjects"),
        patch: t("chat.sourceControl.teammateDraftPatch"),
        patchTruncated: t("chat.sourceControl.teammateDraftPatchTruncated"),
        patchOmitted: t("chat.sourceControl.teammateDraftPatchOmitted"),
      });
      const sent = await chat.sendThreadTurn(prompt);
      if (!current(request)) return;
      pending = { generation: request, threadId: sent.threadId, turnId: sent.turnId, fieldValue: requestedValue };
    } catch (reason: unknown) {
      if (current(request)) notice = chatErrorMessage(reason);
    } finally {
      if (current(request)) requesting = false;
    }
  }

  async function check(): Promise<void> {
    if (checking) {
      recheck = true;
      return;
    }
    checking = true;
    try {
      do {
        recheck = false;
        const target = pending;
        if (!target || !current(target.generation)) return;
        const page = await chatApi.readChatTimelineTurn(target.threadId, target.turnId);
        if (!current(target.generation) || pending !== target) return;
        const outcome = draftTurnOutcome(page, target.turnId);
        if (outcome === null) continue;
        pending = null;
        if (outcome !== "completed") {
          notice = t("chat.sourceControl.teammateDraftUnfinished");
          return;
        }
        const draft = extractDraft(draftReplyText(page, target.turnId) ?? "", kind);
        if (!draft.ok) {
          notice = rejection(draft.reason);
          return;
        }
        if (fieldValue === target.fieldValue) onDraft(draft);
        else {
          ready = draft;
          notice = t("chat.sourceControl.teammateDraftReady");
        }
        return;
      } while (recheck);
    } catch (reason: unknown) {
      const target = pending;
      if (target && current(target.generation)) {
        pending = null;
        notice = chatErrorMessage(reason);
      }
    } finally {
      checking = false;
    }
  }

  function useDraft(): void {
    if (!ready) return;
    onDraft(ready);
    ready = null;
    notice = null;
  }
</script>

{#if chat.openReplyThreadId === null}
  <div class="teammate-draft">
    <button
      type="button"
      class="chat-secondary-button"
      disabled={!available || !allowed || busy}
      title={available ? undefined : t("chat.sourceControl.teammateDraftUnavailable")}
      aria-busy={busy ? "true" : undefined}
      onclick={() => void request()}
    ><WandSparkles size={13} />{busy ? t("chat.sourceControl.teammateDraftPending") : t("chat.sourceControl.teammateDraft")}</button>
    {#if ready}
      <button type="button" class="chat-secondary-button" onclick={useDraft}>{t("chat.sourceControl.useTeammateDraft")}</button>
    {/if}
    {#if notice}<small role="status">{notice}</small>{/if}
  </div>
{/if}

<style>
  .teammate-draft { display: flex; flex-wrap: wrap; align-items: center; gap: 0.35rem; }
  .teammate-draft button { display: inline-flex; align-items: center; gap: 0.3rem; min-height: 1.75rem; padding: 0.25rem 0.55rem; font-size: calc(0.7rem * var(--type-scale)); }
  .teammate-draft small { flex-basis: 100%; color: var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
</style>
