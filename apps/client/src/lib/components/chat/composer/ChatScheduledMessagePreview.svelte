<script lang="ts">
  import Send from "@lucide/svelte/icons/send";
  import Trash2 from "@lucide/svelte/icons/trash";
  import type { ChatScheduledMessageRead } from "$lib/chat/contracts";
  import { chatReferenceTextSegments } from "$lib/chat/composer/message-references";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";

  let {
    message,
    pending = false,
    onSendNow,
    onRetry,
    onCancel,
  }: {
    message: ChatScheduledMessageRead;
    pending?: boolean;
    onSendNow: () => void;
    /** Retries a failed delivery and rejects when the retry fails. */
    onRetry: () => Promise<void>;
    onCancel: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const userDisplayName = $derived(preferences.profileDisplayName || t("chat.timeline.you"));
  const messageSegments = $derived(chatReferenceTextSegments(
    message.normalizedMarkdown,
    message.references,
  ));
</script>

<article class="scheduled-preview">
  <div class="avatar-cell">
    <ProfileAvatar displayName={userDisplayName} imagePath={preferences.profileAvatarImagePath} crop={preferences.profileImageCrop} color={preferences.profileColor} size={32} />
  </div>
  <div class="message-body">
    <header>
      <strong>{userDisplayName}</strong>
      <time datetime={message.createdAt}>{formatDateTime(localization.locale, Date.parse(message.createdAt), { timeStyle: "short" })}</time>
    </header>
    {#if message.normalizedMarkdown}<div class="message-copy" data-selectable-content>{#each messageSegments as segment, index (`${segment.kind}:${index}`)}{#if segment.kind === "reference"}<span class="inline-reference">{segment.text}</span>{:else}{segment.text}{/if}{/each}</div>{/if}
    {#if message.attachmentIds.length > 0 || message.alsoSendToChannel}
      <div class="message-context">
        {#if message.attachmentIds.length > 0}<span>{t("chat.organization.images", message.attachmentIds.length)}</span>{/if}
        {#if message.alsoSendToChannel}<span>{t("chat.organization.alsoSharedToChannel")}</span>{/if}
      </div>
    {/if}
    <div class="delivery-row">
      {#if message.state === "failed"}
        <Notice
          class="delivery-notice"
          title={message.lastError ? t("chat.organization.scheduledMessageFailed") : undefined}
          message={message.lastError ?? t("chat.organization.scheduledMessageFailed")}
          retryLabel={t("chat.organization.retryScheduledMessage")}
          {onRetry}
          copy={message.lastError ?? false}
        />
      {:else}
        <span>
          {message.state === "dispatching"
            ? t("chat.organization.sendingScheduledMessage")
            : t("chat.organization.scheduledDelivery", formatDateTime(localization.locale, Date.parse(message.scheduledFor), { dateStyle: "medium", timeStyle: "short" }))}
        </span>
      {/if}
      <div class="message-actions">
        {#if message.state !== "failed"}
          <button type="button" disabled={pending || message.state === "dispatching"} onclick={onSendNow}><Send size={13} />{t("chat.organization.sendScheduledNow")}</button>
        {/if}
        <button class="cancel-action" type="button" disabled={pending || message.state === "dispatching"} onclick={onCancel}><Trash2 size={13} />{t("chat.organization.cancelScheduledMessage")}</button>
      </div>
    </div>
    {#if message.lastError && message.state !== "failed"}<Notice class="delivery-notice delivery-notice-detail" role="status" message={message.lastError} />{/if}
  </div>
</article>

<style>
  .scheduled-preview { display:grid; grid-template-columns:2.25rem minmax(0,1fr); gap:0.65rem; padding:0.7rem 0.65rem; }
  :global(.scheduled-preview + .scheduled-preview) { border-top:1px solid color-mix(in srgb,var(--border) 72%,transparent); }
  .avatar-cell { min-height:1px; }
  .avatar-cell :global(.profile-avatar) { display:grid; }
  .message-body { min-width:0; }
  header { display:flex; min-height:1.3rem; align-items:baseline; gap:0.4rem; }
  header strong { font-size: calc(0.875rem * var(--type-scale)); }
  header time { color:var(--muted-foreground); font-size: calc(0.75rem * var(--type-scale)); }
  .message-copy { white-space:pre-wrap; overflow-wrap:anywhere; color:var(--foreground); font-size: calc(1rem * var(--type-scale)); line-height: calc(1.5rem * var(--type-scale)); }
  .inline-reference { border-radius:0.25rem; background:color-mix(in srgb,var(--primary) 10%,transparent); padding-inline:0.1rem; color:color-mix(in srgb,var(--primary) 76%,var(--foreground)); }
  .message-context { display:flex; flex-wrap:wrap; gap:0.3rem; margin-top:0.35rem; }
  .message-context span { border-radius:999px; background:var(--accent); padding:0.15rem 0.4rem; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .delivery-row { display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:0.4rem; margin-top:0.55rem; }
  .delivery-row > span { color:var(--muted-foreground); font-size: calc(0.68rem * var(--type-scale)); }
  .scheduled-preview :global(.delivery-notice) { flex:1 1 12rem; font-size: calc(0.68rem * var(--type-scale)); }
  .scheduled-preview :global(.delivery-notice-detail) { margin-top:0.35rem; }
  .message-actions { display:flex; flex-wrap:wrap; align-items:center; gap:0.2rem; }
  .message-actions button { display:flex; min-height:1.8rem; align-items:center; gap:0.3rem; border-radius:0.4rem; padding:0.25rem 0.45rem; color:var(--foreground); font-size: calc(0.68rem * var(--type-scale)); font-weight:500; }
  .message-actions button:hover:not(:disabled) { background:var(--accent); }
  .message-actions button:disabled { opacity:0.45; }
  .message-actions .cancel-action { color:var(--muted-foreground); }
  .message-actions .cancel-action:hover:not(:disabled) { color:var(--destructive); }</style>
