<script lang="ts">
  import CalendarClock from "@lucide/svelte/icons/calendar-clock";
  import { dismissChatScheduleProposal, readChatScheduleProposalStates } from "$lib/api/chat";
  import type {
    ChatConversationItemId,
    ChatRecurrenceSpec,
    ChatScheduleProposal,
    ChatScheduleProposalState,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { chatRecurrenceSummary } from "$lib/chat/composer/recurrence-presentation";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { requestConfirmation } from "$lib/components/ui/confirmation.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { localTimezone } from "$lib/stores/calendar/event-payloads";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";

  /**
   * A teammate's schedule proposal: when it would first send, how it repeats, and why. Nothing is
   * scheduled until a person confirms it in the schedule menu; dismissing it hides it on this device.
   */
  let {
    proposalItemId,
    proposal,
  }: {
    proposalItemId: ChatConversationItemId;
    proposal: ChatScheduleProposal;
  } = $props();

  type ScheduleMenuComponent = typeof import("$lib/components/chat/composer/ChatMessageScheduleMenu.svelte").default;

  const localization = getLocalization();
  const { t } = localization;
  const chat = useChat();
  const preferences = getPreferences();
  let proposalState = $state<ChatScheduleProposalState | null>(null);
  let pending = $state(false);
  let error = $state<string | null>(null);
  let ScheduleMenu = $state<ScheduleMenuComponent | null>(null);
  let menuOpen = $state(false);

  const firstAtLabel = $derived(formatDateTime(
    localization.locale,
    Date.parse(proposal.firstAt),
    { dateStyle: "medium", timeStyle: "short" },
  ));
  const ruleLabel = $derived(proposal.recurrence
    ? chatRecurrenceSummary(proposal.recurrence, {
      t,
      locale: localization.locale,
      timeFormat: preferences.calendarTimeFormat,
      localTimeZone: localTimezone(),
    })
    : t("chat.organization.proposal.once"));
  /** A one-time proposal whose time has passed can no longer be confirmed as proposed. */
  const expired = $derived(proposal.recurrence === null && Date.parse(proposal.firstAt) <= Date.now());

  $effect(() => {
    const _version = chat.scheduledMessagesVersion;
    void loadState(proposalItemId);
  });

  async function loadState(itemId: ChatConversationItemId): Promise<void> {
    try {
      const [read] = await readChatScheduleProposalStates([itemId]);
      if (itemId === proposalItemId) proposalState = read?.state ?? "pending";
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.proposalAction"));
    }
  }

  async function openConfirm(): Promise<void> {
    if (pending) return;
    error = null;
    try {
      ScheduleMenu ??= (await import("$lib/components/chat/composer/ChatMessageScheduleMenu.svelte")).default;
      menuOpen = true;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.proposalAction"));
    }
  }

  async function confirm(scheduledFor: string, recurrence: ChatRecurrenceSpec | null): Promise<void> {
    pending = true;
    error = null;
    try {
      await chat.confirmScheduleProposal({ proposalItemId, scheduledFor, recurrence, normalizedMarkdown: null });
      proposalState = "confirmed";
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.proposalAction"));
    } finally {
      pending = false;
    }
  }

  async function dismiss(): Promise<void> {
    if (pending) return;
    const confirmed = await requestConfirmation({
      message: t("chat.organization.proposal.dismissConfirm"),
      confirmLabel: t("chat.organization.proposal.dismiss"),
    });
    if (!confirmed || pending) return;
    pending = true;
    error = null;
    try {
      await dismissChatScheduleProposal(proposalItemId);
      proposalState = "dismissed";
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.proposalAction"));
    } finally {
      pending = false;
    }
  }
</script>

{#if proposalState !== "dismissed"}
  <div class="schedule-proposal" data-schedule-proposal>
    <div class="proposal-heading">
      <CalendarClock size={14} />
      <strong>{t("chat.organization.proposal.heading")}</strong>
    </div>
    <dl>
      <dt>{t("chat.organization.proposal.firstAt")}</dt>
      <dd><time datetime={proposal.firstAt}>{firstAtLabel}</time></dd>
      <dt>{t("chat.organization.proposal.repeats")}</dt>
      <dd>{ruleLabel}</dd>
      {#if proposal.reason}
        <dt>{t("chat.organization.proposal.reason")}</dt>
        <dd>{proposal.reason}</dd>
      {/if}
    </dl>
    {#if proposalState === "confirmed"}
      <p class="proposal-status" role="status">{t("chat.organization.proposal.confirmed")}</p>
    {:else if proposalState === "pending"}
      {#if expired}<p class="proposal-status">{t("chat.organization.proposal.expired")}</p>{/if}
      <div class="proposal-actions">
        <button type="button" class="proposal-confirm" disabled={pending} onclick={() => void openConfirm()}>
          {expired ? t("chat.organization.proposal.chooseTime") : t("chat.organization.proposal.confirm")}
        </button>
        <button type="button" class="proposal-dismiss" disabled={pending} onclick={() => void dismiss()}>{t("chat.organization.proposal.dismiss")}</button>
        {#if menuOpen && ScheduleMenu}
          <ScheduleMenu
            mode="choose"
            startCustom
            allowClear={false}
            selectedScheduledFor={expired ? null : proposal.firstAt}
            selectedRecurrence={proposal.recurrence}
            disabled={pending}
            onSelect={(scheduledFor, recurrence) => void confirm(scheduledFor, recurrence)}
            onClose={() => { menuOpen = false; }}
          />
        {/if}
      </div>
    {/if}
    {#if error}<Notice class="proposal-error" message={error} />{/if}
  </div>
{/if}

<style>
  .schedule-proposal { display:grid; gap:0.4rem; max-width:32rem; margin-top:0.4rem; border:1px solid var(--border); border-radius:0.45rem; padding:0.55rem 0.65rem; font-size:calc(0.78rem * var(--type-scale)); }
  .proposal-heading { display:flex; align-items:center; gap:0.4rem; color:var(--foreground); }
  .proposal-heading strong { font-weight:600; }
  dl { display:grid; grid-template-columns:auto minmax(0,1fr); gap:0.2rem 0.6rem; margin:0; }
  dt { color:var(--muted-foreground); }
  dd { min-width:0; margin:0; overflow-wrap:anywhere; color:var(--foreground); }
  .proposal-status { color:var(--muted-foreground); }
  .proposal-actions { position:relative; display:flex; flex-wrap:wrap; gap:0.35rem; }
  .proposal-confirm,.proposal-dismiss { min-height:1.9rem; border-radius:var(--floating-item-radius); padding:0.2rem 0.7rem; font-weight:600; }
  .proposal-confirm { background:var(--primary); color:var(--primary-foreground); }
  .proposal-dismiss { border:1px solid var(--border); color:var(--foreground); }
  .proposal-dismiss:hover:not(:disabled) { background:var(--accent); }
  .proposal-confirm:disabled,.proposal-dismiss:disabled { cursor:not-allowed; opacity:0.5; }
  @media (pointer:coarse) { .proposal-confirm,.proposal-dismiss { min-height:2.75rem; } }
</style>
