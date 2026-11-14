<script lang="ts">
  import MessageCircle from "@lucide/svelte/icons/message-circle";
  import type { ChatMessageRead, ChatMessageReference, ChatParticipantRead } from "$lib/chat/contracts";
  import { organizationalMessageActionTarget } from "$lib/chat/timeline/message-action-target";
  import { chatParticipantDisplayName } from "$lib/chat/teammates/participant-display";
  import { getAllContexts, mount, unmount, untrack } from "svelte";
  import { openChatExternalUrl } from "$lib/api/chat";
  import { ORGANIZATIONAL_REFERENCE_SLOT_ATTRIBUTE, renderOrganizationalMarkdown, type RenderedOrganizationalMarkdown } from "$lib/chat/timeline/organizational-markdown";
  import { isTruncatedChatMessage } from "$lib/chat/timeline/organizational-message-model";
  import { chatAgentUpdatePresentation, chatFailureTechnicalDetail, type ChatAgentFailureCause } from "$lib/chat/timeline/agent-update";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChatPaneEvents } from "$lib/chat/pane-events";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import ChatIdentityButton from "$lib/components/chat/identity/ChatIdentityButton.svelte";
  import ChatMessageActionToolbar from "./ChatMessageActionToolbar.svelte";
  import ChatMessageReactionList from "./ChatMessageReactionList.svelte";
  import { heldMessageActionsKey, messageTouchActions } from "./message-touch-actions.svelte";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import "./chat-markdown.css";

  let {
    message,
    grouped = false,
    showReplyStrip = true,
    currentResponseSettings = false,
    onOpenThread = () => undefined,
    onActionError,
    answerChoices = [],
    onChooseAnswer,
    onRetryFailure,
  }: {
    message: ChatMessageRead;
    grouped?: boolean;
    showReplyStrip?: boolean;
    currentResponseSettings?: boolean;
    onOpenThread?: (trigger: HTMLElement) => void;
    /** Shows a failed message action, such as a Sent from lookup, in the surrounding view. */
    onActionError?: (error: unknown) => void;
    /** Suggested answers offered as chips, for a question that still waits for an answer. */
    answerChoices?: readonly string[];
    /** Puts the chosen answer in the composer. */
    onChooseAnswer?: (choice: string) => void;
    /** Retries the failed work, offered on the failure message that still stands for it. */
    onRetryFailure?: () => unknown;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = useChat();
  const paneEvents = useChatPaneEvents();
  const preferences = getPreferences();
  const touchActionsKey = $props.id();
  let actionToolbarVisible = $state(false);
  const authorDisplayName = $derived(message.authorLabelSnapshot || (message.author ? chatParticipantDisplayName(
    message.author,
    preferences.profileDisplayName,
    t("chat.timeline.you"),
  ) : ""));
  const actionTarget = $derived(organizationalMessageActionTarget(message));
  const agentUpdate = $derived(chatAgentUpdatePresentation(message.richContent));
  const failure = $derived(agentUpdate?.kind === "failure" ? agentUpdate : null);
  const contexts = getAllContexts();

  /** The localized explanation of a failure; the stored provider text is only reachable through copy. */
  function failureMessage(cause: ChatAgentFailureCause): string {
    switch (cause.kind) {
      case "error": return chatErrorMessage({ code: cause.code });
      case "provider_limit": return t("chat.composer.budget.providerLimitUntimed");
      case "budget": return cause.limit === "tokens"
        ? t("chat.composer.budget.tokenReached", authorDisplayName)
        : t("chat.composer.budget.workingTimeReached", authorDisplayName);
      case "launch":
      case "unknown": return t("chatErrors.generic");
    }
  }

  /** The lead-in naming the teammate, left out when the message already names it. */
  function failureTitle(cause: ChatAgentFailureCause): string | undefined {
    if (cause.kind === "budget") return undefined;
    return cause.kind === "launch"
      ? t("chat.runs.outcome.failedBeforeOutput", authorDisplayName)
      : t("chat.runs.outcome.failedAfterOutput", authorDisplayName);
  }
  let copy: HTMLDivElement | undefined = $state();
  let previousRender: { key: string; value: RenderedOrganizationalMarkdown } | undefined;
  /** Reuses the previous render while the text and references are unchanged, so a refresh keeps mounted references. */
  const renderedCopy = $derived.by(() => {
    const key = JSON.stringify([message.normalizedMarkdown, message.references]);
    if (previousRender?.key === key) return previousRender.value;
    const value = renderOrganizationalMarkdown(message.normalizedMarkdown, message.references);
    previousRender = { key, value };
    return value;
  });
  const truncated = $derived(isTruncatedChatMessage(message));

  function participantForReference(reference: Extract<ChatMessageReference, { kind: "participant" }>): ChatParticipantRead {
    const current = [
      ...(chat.selectedChannel?.memberships ?? []).map((membership) => membership.participant),
      ...chat.teammateIdentities.map((teammate) => teammate.participant),
      ...(message.replyThread?.participants ?? []),
      ...(message.author ? [message.author] : []),
    ].find((participant) => participant.id === reference.participantId);
    return current ?? {
      id: reference.participantId,
      kind: reference.participantKind,
      displayName: reference.metadata.labelSnapshot,
      avatar: { schemaVersion: 1, value: {} },
      revision: 0,
      archivedAt: null,
    };
  }

  function openReference(reference: ChatMessageReference): void {
    if (reference.kind === "channel") {
      void chat.selectChannel(reference.channelId);
      return;
    }
    if (reference.kind === "workingFolder") {
      chat.selectWorkingFolder(reference.workingFolderId);
      return;
    }
    if (reference.kind === "workspacePath") {
      paneEvents.dispatchEvent(new CustomEvent("ganbaru:chat-open-file", {
        detail: {
          workingFolderId: reference.workingFolderId,
          relativePath: reference.relativePath,
        },
      }));
      return;
    }
    if (reference.kind === "executionEnvironment") {
      chat.setExecutionEnvironment(reference.executionEnvironmentId);
    }
  }

  $effect(() => {
    const root = copy;
    const { slots } = renderedCopy;
    if (!root) return;
    const mounted: ReturnType<typeof mount>[] = [];
    const filled: HTMLElement[] = [];
    untrack(() => {
      for (const slot of slots) {
        const target = root.querySelector(`[${ORGANIZATIONAL_REFERENCE_SLOT_ATTRIBUTE}="${slot.index}"]`);
        if (!(target instanceof HTMLElement)) continue;
        target.replaceChildren();
        filled.push(target);
        const reference = slot.reference;
        if (reference.kind === "participant") {
          mounted.push(mount(ChatIdentityButton, {
            target,
            context: contexts,
            props: {
              get participant() { return participantForReference(reference); },
              presentation: "mention",
              triggerLabel: slot.text,
              get currentResponseSettings() { return currentResponseSettings; },
            },
          }));
        } else {
          target.append(referenceButton(reference, slot.text));
        }
      }
    });
    return () => {
      for (const component of mounted) void unmount(component);
      for (const target of filled) target.replaceChildren();
    };
  });

  $effect(() => {
    const root = copy;
    if (!root) return;
    root.addEventListener("click", openExternalLink);
    return () => root.removeEventListener("click", openExternalLink);
  });

  /** Builds the inline control for a channel, working folder, path, or environment reference. */
  function referenceButton(reference: ChatMessageReference, text: string): HTMLButtonElement {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "inline-reference";
    button.textContent = text;
    button.addEventListener("click", () => openReference(reference));
    return button;
  }

  function openExternalLink(event: MouseEvent): void {
    const link = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>("a[data-chat-external-link]") : null;
    if (!link) return;
    event.preventDefault();
    void openChatExternalUrl(link.href).catch((error: unknown) => onActionError?.(error));
  }

</script>

<article
  class="message-row"
  class:grouped
  data-message-item-id={message.itemId}
  tabindex="-1"
  aria-label={`${authorDisplayName}, ${formatDateTime(localization.locale, Date.parse(message.createdAt), { dateStyle: "medium", timeStyle: "short" })}`}
  use:messageTouchActions={touchActionsKey}
  data-message-touch-actions
  onpointerenter={(event) => { if (event.pointerType !== "touch") actionToolbarVisible = true; }}
  onpointerleave={() => { actionToolbarVisible = false; }}
  onfocusin={(event) => { if (event.target instanceof Element && event.target.matches(":focus-visible")) actionToolbarVisible = true; }}
  onfocusout={(event) => {
    if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) {
      actionToolbarVisible = false;
    }
  }}
>
  <div class="avatar-cell">
    {#if !grouped && message.author}<ChatIdentityButton participant={message.author} presentation="avatar" size={34} {currentResponseSettings} />{/if}
  </div>
  <div class="message-body">
    {#if !grouped}
      <header>
        <strong>{#if message.author}<ChatIdentityButton participant={message.author} presentation="name" triggerLabel={authorDisplayName} {currentResponseSettings} />{:else}{authorDisplayName}{/if}</strong>
        <time datetime={message.createdAt}>{formatDateTime(localization.locale, Date.parse(message.createdAt), { timeStyle: "short" })}</time>
      </header>
    {/if}
    {#if failure}
      <div class="message-failure" data-chat-failure>
        <Notice
          title={failureTitle(failure.cause)}
          message={failureMessage(failure.cause)}
          copy={chatFailureTechnicalDetail(failure.code, message.normalizedMarkdown) ?? false}
          onRetry={onRetryFailure}
          retryLabel={onRetryFailure ? t("chat.organization.retry") : undefined}
          role="status"
        />
      </div>
    {:else}
      <div bind:this={copy} class="message-copy chat-markdown" data-selectable-content>{@html renderedCopy.html}</div>
    {/if}
    {#if truncated}<p class="message-elision">{t("chat.organization.truncatedMessage")}</p>{/if}
    {#if message.attachmentIds.length > 0}
      <div class="message-context">
        {#if message.attachmentIds.length > 0}<span>{t("chat.organization.images", message.attachmentIds.length)}</span>{/if}
      </div>
    {/if}
    {#if agentUpdate?.kind === "delegation_outcome"}
      <div class="message-context" data-delegation-outcome><span>{t(`chat.delegation.states.${agentUpdate.state}`)}</span></div>
    {/if}
    {#if agentUpdate?.kind === "schedule_proposal"}
      {@const proposal = agentUpdate.proposal}
      {#await import("./ChatScheduleProposalCard.svelte") then module}
        <module.default proposalItemId={message.itemId} {proposal} />
      {/await}
    {/if}
    {#if agentUpdate?.kind === "push_request"}
      {@const push = agentUpdate.push}
      {#await import("./ChatPushRequestCard.svelte") then module}
        <module.default requestItemId={message.itemId} {push} />
      {/await}
    {/if}
    {#if agentUpdate?.kind === "note_proposal"}
      {#await import("./ChatNoteProposalCard.svelte") then module}
        <module.default proposalItemId={message.itemId} body={message.normalizedMarkdown} />
      {/await}
    {/if}
    {#if onChooseAnswer && answerChoices.length > 0}
      <div class="answer-choices" role="group" aria-label={t("chat.questions.choices")} data-answer-choices>
        {#each answerChoices as choice (choice)}
          <button type="button" class="answer-choice" aria-label={t("chat.questions.useChoice", choice)} onclick={() => onChooseAnswer(choice)}>{choice}</button>
        {/each}
      </div>
    {/if}
    {#if showReplyStrip && message.replyThread}
      <button type="button" class="reply-strip" data-reply-thread-id={message.replyThread.id} onclick={(event) => onOpenThread(event.currentTarget)}>
        <span class="reply-avatars">
          {#each message.replyThread.participants.slice(0, 3) as participant (participant.id)}<ChatParticipantAvatar {participant} size={20} shape="compact" />{/each}
        </span>
        <MessageCircle size={13} />
        <span>{t("chat.organization.replies", message.replyThread.replyCount)}</span>
        {#if message.replyThread.unread}<span class="unread-dot" aria-label={t("chat.status.unread")}></span>{/if}
        {#if message.replyThread.unseenFailure}<span class="unread-dot failure-dot" role="img" aria-label={t("chat.channels.unseenFailure")} data-chat-thread-failure></span>{/if}
      </button>
    {/if}
    <ChatMessageReactionList target={actionTarget} />
    <ChatMessageActionToolbar
      target={actionTarget}
      visible={actionToolbarVisible || heldMessageActionsKey() === touchActionsKey}
      placement={grouped ? "grouped-message" : "participant-header"}
      onError={onActionError}
    />
  </div>
</article>

<style>
  .message-row { display:grid; grid-template-columns:34px minmax(0,1fr); gap:0.65rem; padding:var(--chat-conversation-entry-space,0.45rem) var(--chat-message-row-padding-inline,1rem); outline:none; }
  .message-row:focus-visible { border-radius:0.45rem; outline:2px solid var(--ring); outline-offset:-2px; }
  .message-row.grouped { padding-top:0.08rem; }
  .avatar-cell { min-height:1px; }
  .message-body { --chat-message-action-anchor-bottom:1.3rem; --chat-message-reaction-margin-top:0.35rem; position:relative; min-width:0; }
  .message-row:not(.grouped) .message-body { transform:translateY(-4px); }
  header { display:flex; min-height:1.3rem; align-items:baseline; gap:0.4rem; }
  header strong { font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); } header time { color:var(--muted-foreground); font-size:var(--chat-organizational-time-font-size,calc(0.6875rem * var(--type-scale))); }
  .message-copy { overflow-wrap:anywhere; color:var(--foreground); font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); line-height:var(--chat-organizational-line-height,calc(1.3125rem * var(--type-scale))); }
  .message-copy :global(.inline-reference) { display:inline; border-radius:0.25rem; background:color-mix(in srgb,var(--primary) 10%,transparent); padding-inline:0.1rem; color:color-mix(in srgb,var(--primary) 76%,var(--foreground)); font:inherit; }
  .message-copy :global(.inline-reference:hover),.message-copy :global(.inline-reference:focus-visible) { text-decoration:underline; }
  .message-elision { margin-top:0.2rem; color:var(--muted-foreground); font-size:var(--chat-organizational-time-font-size,calc(0.6875rem * var(--type-scale))); font-style:italic; }
  .message-context { display:flex; flex-wrap:wrap; gap:0.3rem; margin-top:0.3rem; }
  .message-context span { border-radius:999px; background:var(--accent); padding:0.15rem 0.4rem; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .reply-strip { display:flex; width:100%; min-height:2rem; align-items:center; gap:0.4rem; margin-top:0.35rem; border-radius:0.4rem; color:color-mix(in srgb,var(--primary) 70%,var(--foreground)); font-size: calc(0.68rem * var(--type-scale)); text-align:left; }
  .reply-strip:hover,.reply-strip:focus-visible { background:color-mix(in srgb,var(--accent) 45%,transparent); }
  .reply-avatars { display:flex; align-items:center; gap:0.25rem; padding-left:0.2rem; }
  .unread-dot { width:0.42rem; height:0.42rem; flex:0 0 auto; border-radius:999px; background:var(--primary); }
  .failure-dot { background:var(--destructive); }
  .answer-choices { display:flex; flex-wrap:wrap; gap:0.3rem; margin-top:0.4rem; }
  .answer-choice { min-height:1.75rem; max-width:100%; overflow:hidden; border:1px solid var(--border); border-radius:999px; padding:0.15rem 0.6rem; color:var(--foreground); font-size: calc(0.72rem * var(--type-scale)); text-overflow:ellipsis; white-space:nowrap; }
  .answer-choice:hover,.answer-choice:focus-visible { background:var(--accent); }
  @media (pointer:coarse) { .answer-choice { min-height:2.75rem; } }
</style>
