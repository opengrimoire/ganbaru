<script lang="ts">
  import Pencil from "@lucide/svelte/icons/pencil";
  import Send from "@lucide/svelte/icons/send";
  import Trash2 from "@lucide/svelte/icons/trash";
  import { untrack } from "svelte";
  import type { ChatScheduledMessageRead } from "$lib/chat/contracts";
  import { chatScheduledNextDelivery } from "$lib/chat/composer/message-scheduling";
  import { chatRecurrenceSummary } from "$lib/chat/composer/recurrence-presentation";
  import { scheduledMessageActionTarget, type ChatMessageToolbarAction } from "$lib/chat/timeline/message-action-target";
  import { ORGANIZATIONAL_REFERENCE_SLOT_ATTRIBUTE, renderOrganizationalMarkdown } from "$lib/chat/timeline/organizational-markdown";
  import ChatMessageActionToolbar from "$lib/components/chat/timeline/ChatMessageActionToolbar.svelte";
  import { heldMessageActionsKey, messageTouchActions } from "$lib/components/chat/timeline/message-touch-actions.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { localTimezone } from "$lib/stores/calendar/event-payloads";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import "$lib/components/chat/timeline/chat-markdown.css";

  let {
    message,
    pending = false,
    onSendNow,
    onRetry,
    onEdit,
    onCancel,
    onActionError,
  }: {
    message: ChatScheduledMessageRead;
    pending?: boolean;
    onSendNow: () => void;
    /** Retries a failed delivery and rejects when the retry fails. */
    onRetry: () => Promise<void>;
    /** Loads the message into the composer; the edit action is left out when it is absent. */
    onEdit?: () => void;
    onCancel: () => void;
    /** Shows a failed toolbar action, such as a copy the clipboard refused, in the surrounding view. */
    onActionError?: (error: unknown) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const touchActionsKey = $props.id();
  let actionToolbarVisible = $state(false);
  let copy: HTMLDivElement | undefined = $state();
  const userDisplayName = $derived(preferences.profileDisplayName || t("chat.timeline.you"));
  const recurrence = $derived(message.recurrence);
  const busy = $derived(pending || message.state === "dispatching");
  const repeatSummary = $derived(recurrence
    ? chatRecurrenceSummary(recurrence, {
      t,
      locale: localization.locale,
      timeFormat: preferences.calendarTimeFormat,
      localTimeZone: localTimezone(),
    })
    : null);
  const renderedCopy = $derived(renderOrganizationalMarkdown(message.normalizedMarkdown, message.references));
  const actionTarget = $derived(scheduledMessageActionTarget(message));
  const deliveryParts = $derived.by(() => {
    if (message.state === "dispatching") return [t("chat.organization.sendingScheduledMessage")];
    if (!recurrence) return [t("chat.organization.scheduleLine", formatInstant(message.scheduledFor))];
    return [
      t("chat.organization.repeat.nextLine", formatInstant(chatScheduledNextDelivery(message))),
      ...(repeatSummary ? [repeatSummary] : []),
      ...(recurrence.firedCount > 0 ? [t("chat.organization.repeat.firedCount", recurrence.firedCount)] : []),
    ];
  });
  const toolbarActions = $derived.by((): ChatMessageToolbarAction[] => [
    ...(message.state === "failed" ? [] : [{
      id: "send-now",
      label: t("chat.organization.sendScheduledNow"),
      icon: Send,
      disabled: busy,
      onSelect: onSendNow,
    }]),
    "copy",
    ...(onEdit ? [{
      id: "edit",
      label: t("chat.organization.editScheduledMessage"),
      icon: Pencil,
      disabled: busy,
      onSelect: onEdit,
    }] : []),
    {
      id: "cancel",
      label: t("chat.organization.cancelScheduledMessage"),
      icon: Trash2,
      disabled: busy,
      onSelect: onCancel,
    },
  ]);

  function formatInstant(value: string): string {
    return formatDateTime(localization.locale, Date.parse(value), { dateStyle: "medium", timeStyle: "short" });
  }

  /** Shows each reference as inline text; the preview opens nothing, since it lives inside a menu. */
  $effect(() => {
    const root = copy;
    const { slots } = renderedCopy;
    if (!root) return;
    untrack(() => {
      for (const slot of slots) {
        const target = root.querySelector(`[${ORGANIZATIONAL_REFERENCE_SLOT_ATTRIBUTE}="${slot.index}"]`);
        if (!(target instanceof HTMLElement)) continue;
        const reference = document.createElement("span");
        reference.className = "inline-reference";
        reference.textContent = slot.text;
        target.replaceChildren(reference);
      }
    });
  });
</script>

<article
  class="scheduled-preview"
  data-scheduled-message-id={message.id}
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
    <ProfileAvatar displayName={userDisplayName} imagePath={preferences.profileAvatarImagePath} crop={preferences.profileImageCrop} color={preferences.profileColor} size={34} />
  </div>
  <div class="message-body">
    <header>
      <strong>{userDisplayName}</strong>
      <time datetime={message.createdAt}>{formatDateTime(localization.locale, Date.parse(message.createdAt), { timeStyle: "short" })}</time>
    </header>
    {#if message.normalizedMarkdown}<div bind:this={copy} class="message-copy chat-markdown" data-selectable-content>{@html renderedCopy.html}</div>{/if}
    {#if message.attachmentIds.length > 0 || message.alsoSendToChannel}
      <div class="message-context">
        {#if message.attachmentIds.length > 0}<span>{t("chat.organization.images", message.attachmentIds.length)}</span>{/if}
        {#if message.alsoSendToChannel}<span>{t("chat.organization.alsoSharedToChannel")}</span>{/if}
      </div>
    {/if}
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
      <p class="delivery-line" data-scheduled-delivery>
        {#each deliveryParts as part, index (index)}{#if index > 0}<span class="delivery-separator" aria-hidden="true">·</span>{/if}<span>{part}</span>{/each}
      </p>
    {/if}
    {#if recurrence && recurrence.skippedOccurrences > 0}<p class="delivery-line">{t("chat.organization.repeat.skippedCount", recurrence.skippedOccurrences)}</p>{/if}
    {#if recurrence?.deferredUntil}<Notice class="delivery-notice" role="status" message={t("chat.organization.repeat.deferredUntil", formatInstant(recurrence.deferredUntil))} />{/if}
    {#if message.lastError && message.state !== "failed"}<Notice class="delivery-notice" role="status" message={message.lastError} />{/if}
    <ChatMessageActionToolbar
      target={actionTarget}
      actions={toolbarActions}
      visible={actionToolbarVisible || heldMessageActionsKey() === touchActionsKey}
      onError={onActionError}
    />
  </div>
</article>

<style>
  .scheduled-preview { display:grid; grid-template-columns:34px minmax(0,1fr); gap:0.65rem; padding:var(--chat-conversation-entry-space,0.45rem) 0.65rem; }
  .avatar-cell { min-height:1px; }
  .avatar-cell :global(.profile-avatar) { display:grid; }
  .message-body { --chat-message-action-anchor-bottom:1.3rem; position:relative; min-width:0; transform:translateY(-4px); }
  header { display:flex; min-height:1.3rem; align-items:baseline; gap:0.4rem; }
  header strong { font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); }
  header time { color:var(--muted-foreground); font-size:var(--chat-organizational-time-font-size,calc(0.6875rem * var(--type-scale))); }
  .message-copy { overflow-wrap:anywhere; color:var(--foreground); font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); line-height:var(--chat-organizational-line-height,calc(1.3125rem * var(--type-scale))); }
  .message-copy :global(.inline-reference) { display:inline; border-radius:0.25rem; background:color-mix(in srgb,var(--primary) 10%,transparent); padding-inline:0.1rem; color:color-mix(in srgb,var(--primary) 76%,var(--foreground)); font:inherit; }
  .message-context { display:flex; flex-wrap:wrap; gap:0.3rem; margin-top:0.35rem; }
  .message-context span { border-radius:999px; background:var(--accent); padding:0.15rem 0.4rem; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .delivery-line { display:flex; flex-wrap:wrap; column-gap:0.35rem; margin-top:0.3rem; color:var(--muted-foreground); font-size:var(--chat-organizational-time-font-size,calc(0.6875rem * var(--type-scale))); }
  .scheduled-preview :global(.delivery-notice) { margin-top:0.35rem; font-size:var(--chat-organizational-time-font-size,calc(0.6875rem * var(--type-scale))); }
</style>
