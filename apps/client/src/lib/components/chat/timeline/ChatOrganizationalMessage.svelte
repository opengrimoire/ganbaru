<script lang="ts">
  import MessageCircle from "@lucide/svelte/icons/message-circle";
  import type { ChatMessageRead, ChatMessageReference, ChatParticipantRead } from "$lib/chat/contracts";
  import { organizationalMessageActionTarget } from "$lib/chat/timeline/message-action-target";
  import { chatParticipantDisplayName } from "$lib/chat/teammates/participant-display";
  import { chatReferenceTextSegments } from "$lib/chat/composer/message-references";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import ChatIdentityButton from "$lib/components/chat/identity/ChatIdentityButton.svelte";
  import ChatMessageActionToolbar from "./ChatMessageActionToolbar.svelte";
  import ChatMessageReactionList from "./ChatMessageReactionList.svelte";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";

  let {
    message,
    grouped = false,
    showReplyStrip = true,
    currentResponseSettings = false,
    onOpenThread = () => undefined,
  }: {
    message: ChatMessageRead;
    grouped?: boolean;
    showReplyStrip?: boolean;
    currentResponseSettings?: boolean;
    onOpenThread?: (trigger: HTMLElement) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const preferences = getPreferences();
  let actionToolbarVisible = $state(false);
  const authorDisplayName = $derived(message.authorLabelSnapshot || chatParticipantDisplayName(
    message.author,
    preferences.profileDisplayName,
    t("chat.timeline.you"),
  ));
  const actionTarget = $derived(organizationalMessageActionTarget(message));
  const messageSegments = $derived(chatReferenceTextSegments(message.normalizedMarkdown, message.references));

  function participantForReference(reference: Extract<ChatMessageReference, { kind: "participant" }>): ChatParticipantRead {
    const current = [
      ...(chat.selectedChannel?.memberships ?? []).map((membership) => membership.participant),
      ...chat.teammateIdentities.map((teammate) => teammate.participant),
      ...(message.replyThread?.participants ?? []),
      message.author,
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
      window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-open-file", {
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

</script>

<article
  class="message-row"
  class:grouped
  data-message-item-id={message.itemId}
  tabindex="-1"
  aria-label={`${authorDisplayName}, ${formatDateTime(localization.locale, Date.parse(message.createdAt), { dateStyle: "medium", timeStyle: "short" })}`}
  onpointerenter={() => { actionToolbarVisible = true; }}
  onpointerleave={() => { actionToolbarVisible = false; }}
  onfocusin={() => { actionToolbarVisible = true; }}
  onfocusout={(event) => {
    if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) {
      actionToolbarVisible = false;
    }
  }}
>
  <div class="avatar-cell">
    {#if !grouped}<ChatIdentityButton participant={message.author} presentation="avatar" size={34} {currentResponseSettings} />{/if}
  </div>
  <div class="message-body">
    {#if !grouped}
      <header>
        <strong><ChatIdentityButton participant={message.author} presentation="name" triggerLabel={authorDisplayName} {currentResponseSettings} /></strong>
        <time datetime={message.createdAt}>{formatDateTime(localization.locale, Date.parse(message.createdAt), { timeStyle: "short" })}</time>
      </header>
    {/if}
    <div class="message-copy" data-selectable-content>{#each messageSegments as segment, index (`${segment.kind}:${index}`)}{#if segment.kind === "reference"}{#if segment.reference.kind === "participant"}<ChatIdentityButton participant={participantForReference(segment.reference)} presentation="mention" triggerLabel={segment.text} {currentResponseSettings} />{:else}<button type="button" class="inline-reference" onclick={() => openReference(segment.reference)}>{segment.text}</button>{/if}{:else}{segment.text}{/if}{/each}</div>
    {#if message.attachmentIds.length > 0}
      <div class="message-context">
        {#if message.attachmentIds.length > 0}<span>{t("chat.organization.images", message.attachmentIds.length)}</span>{/if}
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
      </button>
    {/if}
    <ChatMessageReactionList target={actionTarget} />
    <ChatMessageActionToolbar
      target={actionTarget}
      visible={actionToolbarVisible}
      placement={grouped ? "grouped-message" : "participant-header"}
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
  .message-copy { white-space:pre-wrap; overflow-wrap:anywhere; color:var(--foreground); font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); line-height:var(--chat-organizational-line-height,calc(1.3125rem * var(--type-scale))); }
  .inline-reference { display:inline; border-radius:0.25rem; background:color-mix(in srgb,var(--primary) 10%,transparent); padding-inline:0.1rem; color:color-mix(in srgb,var(--primary) 76%,var(--foreground)); font:inherit; }
  .inline-reference:hover,.inline-reference:focus-visible { text-decoration:underline; }
  .message-context { display:flex; flex-wrap:wrap; gap:0.3rem; margin-top:0.3rem; }
  .message-context span { border-radius:999px; background:var(--accent); padding:0.15rem 0.4rem; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .reply-strip { display:flex; width:100%; min-height:2rem; align-items:center; gap:0.4rem; margin-top:0.35rem; border-radius:0.4rem; color:color-mix(in srgb,var(--primary) 70%,var(--foreground)); font-size: calc(0.68rem * var(--type-scale)); text-align:left; }
  .reply-strip:hover,.reply-strip:focus-visible { background:color-mix(in srgb,var(--accent) 45%,transparent); }
  .reply-avatars { display:flex; align-items:center; gap:0.25rem; padding-left:0.2rem; }
  .unread-dot { width:0.42rem; height:0.42rem; flex:0 0 auto; border-radius:999px; background:var(--primary); }
</style>
