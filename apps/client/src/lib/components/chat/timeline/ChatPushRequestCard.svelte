<script lang="ts">
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import { chatPushRequestsAvailable, readChatPushRequestStates } from "$lib/api/chat";
  import {
    CHAT_PUSH_REQUEST_REFUSALS,
    type ChatConversationItemId,
    type ChatPushRequest,
    type ChatPushRequestState,
  } from "$lib/chat/contracts";
  import { chatErrorMessage, chatErrorReason } from "$lib/chat/error-presentation";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";

  /**
   * A teammate's request to push a commit. The push runs when a person confirms it here on a desktop device,
   * or after the turn ends when the person's standing approval or a full access turn lets it; either way Rust
   * rechecks the teammate's publish grant and the folder before pushing.
   */
  let {
    requestItemId,
    push,
  }: {
    requestItemId: ChatConversationItemId;
    push: ChatPushRequest;
  } = $props();

  /** Characters of the commit id the card and the confirming reply show. */
  const SHORT_COMMIT_LENGTH = 12;

  const { t } = getLocalization();
  const chat = useChat();
  let requestState = $state<ChatPushRequestState | null>(null);
  let pending = $state(false);
  let error = $state<string | null>(null);

  const shortCommit = $derived(push.objectId.slice(0, SHORT_COMMIT_LENGTH));
  const unlistedPaths = $derived(push.commit ? push.commit.changedPathCount - push.commit.changedPaths.length : 0);
  const confirmable = $derived(chatPushRequestsAvailable && (requestState === "open" || requestState === "waiting"));

  $effect(() => {
    const _version = chat.scheduledMessagesVersion;
    void loadState(requestItemId);
  });

  async function loadState(itemId: ChatConversationItemId): Promise<void> {
    if (!chatPushRequestsAvailable) return;
    try {
      const [read] = await readChatPushRequestStates([itemId]);
      if (itemId === requestItemId) requestState = read?.state ?? "closed";
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
    }
  }

  function errorText(cause: unknown): string {
    const reason = chatErrorReason(cause);
    const refusal = CHAT_PUSH_REQUEST_REFUSALS.find((code) => code === reason);
    return refusal ? t(`chat.organization.push.refusals.${refusal}`) : chatErrorMessage(cause);
  }

  async function confirm(rememberApproval: boolean): Promise<void> {
    if (pending) return;
    pending = true;
    error = null;
    try {
      await chat.confirmPushRequest({
        requestItemId,
        normalizedMarkdown: push.commit
          ? t("chat.organization.push.replyCommitted", push.remote, push.destinationBranch)
          : t("chat.organization.push.reply", shortCommit, push.remote, push.destinationBranch),
        rememberApproval,
      }, push.workingFolderId);
      requestState = "pushed";
    } catch (cause: unknown) {
      error = errorText(cause);
      await loadState(requestItemId);
    } finally {
      pending = false;
    }
  }
</script>

<div class="push-request" data-push-request>
  <div class="push-heading">
    <GitBranch size={14} />
    <strong>{t("chat.organization.push.heading")}</strong>
  </div>
  <dl>
    <dt>{t("chat.organization.push.branch")}</dt>
    <dd><code>{push.localBranch}</code></dd>
    <dt>{t("chat.organization.push.destination")}</dt>
    <dd><code>{push.remote}/{push.destinationBranch}</code></dd>
    <dt>{t("chat.organization.push.url")}</dt>
    <dd><code>{push.pushUrl}</code></dd>
    <dt>{t(push.commit ? "chat.organization.push.parent" : "chat.organization.push.commit")}</dt>
    <dd><code title={push.objectId}>{shortCommit}</code></dd>
    {#if push.commit}
      <dt>{t("chat.organization.push.author")}</dt>
      <dd>{push.commit.author}</dd>
      <dt>{t("chat.organization.push.signing")}</dt>
      <dd>{t(push.commit.signed ? "chat.organization.push.signed" : "chat.organization.push.unsigned")}</dd>
    {/if}
  </dl>
  {#if push.commit}
    <p class="push-message">{push.commit.message}</p>
    <p class="push-status">{t("chat.organization.push.files", push.commit.changedPathCount)}</p>
    <ul class="push-files">
      {#each push.commit.changedPaths as path, index (index)}<li><code>{path}</code></li>{/each}
      {#if unlistedPaths > 0}<li class="push-status">{t("chat.organization.push.moreFiles", unlistedPaths)}</li>{/if}
    </ul>
  {/if}
  {#if !chatPushRequestsAvailable}
    <p class="push-status">{t("chat.organization.push.unavailable")}</p>
  {:else if requestState === "pushed"}
    <p class="push-status" role="status">{t("chat.organization.push.pushed")}</p>
  {:else if requestState === "closed"}
    <p class="push-status">{t("chat.organization.push.closed")}</p>
  {:else if confirmable}
    {#if requestState === "waiting"}<p class="push-status">{t("chat.organization.push.waiting")}</p>{/if}
    <div class="push-actions">
      <button type="button" class="push-confirm" disabled={pending} onclick={() => void confirm(false)}>
        {t(push.commit ? "chat.organization.push.confirmCommit" : "chat.organization.push.confirm")}
      </button>
      <button type="button" class="push-remember" disabled={pending} onclick={() => void confirm(true)}>
        {t(push.commit ? "chat.organization.push.confirmCommitRemember" : "chat.organization.push.confirmRemember")}
      </button>
    </div>
  {/if}
  {#if error}<Notice class="push-error" message={error} />{/if}
</div>

<style>
  .push-request { display:grid; gap:0.4rem; max-width:32rem; margin-top:0.4rem; border:1px solid var(--border); border-radius:0.45rem; padding:0.55rem 0.65rem; font-size:calc(0.78rem * var(--type-scale)); }
  .push-heading { display:flex; align-items:center; gap:0.4rem; color:var(--foreground); }
  .push-heading strong { font-weight:600; }
  dl { display:grid; grid-template-columns:auto minmax(0,1fr); gap:0.2rem 0.6rem; margin:0; }
  dt { color:var(--muted-foreground); }
  dd { min-width:0; margin:0; overflow-wrap:anywhere; color:var(--foreground); }
  .push-status { color:var(--muted-foreground); }
  .push-message { margin:0; white-space:pre-wrap; overflow-wrap:anywhere; color:var(--foreground); }
  .push-files { display:grid; gap:0.1rem; max-height:8rem; margin:0; overflow:auto; padding:0; list-style:none; overflow-wrap:anywhere; }
  .push-actions { display:flex; flex-wrap:wrap; gap:0.35rem; }
  .push-confirm { min-height:1.9rem; border-radius:var(--floating-item-radius); padding:0.2rem 0.7rem; font-weight:600; background:var(--primary); color:var(--primary-foreground); }
  .push-remember { min-height:1.9rem; border-radius:var(--floating-item-radius); padding:0.2rem 0.7rem; color:var(--foreground); }
  .push-remember:hover:not(:disabled) { background:var(--accent); }
  .push-remember:disabled { cursor:not-allowed; opacity:0.5; }
  .push-confirm:disabled { cursor:not-allowed; opacity:0.5; }
  @media (pointer:coarse) { .push-confirm, .push-remember { min-height:2.75rem; } }
</style>
