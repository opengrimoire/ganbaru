<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import Settings from "@lucide/svelte/icons/settings";
  import Zap from "@lucide/svelte/icons/zap";
  import * as chatApi from "$lib/api/chat";
  import type { ChatInteractionStateRead, ChatParticipantRead, ChatThreadId } from "$lib/chat/contracts";
  import { composerMeterRead } from "$lib/chat/composer/composer-meter";
  import { formatNumber } from "$lib/i18n/formatters";
  import { chatParticipantDisplayName } from "$lib/chat/agents/participant-display";
  import type { ChatModelParticipant } from "$lib/chat/agents/participant-identity";
  import { compactModelOptionLabel } from "$lib/chat/composer/model-picker-model";
  import { formatModelDisplayName } from "$lib/chat/composer/model-company";
  import { agentExecutionSummary } from "$lib/chat/agents/execution-summary";
  import { chatAgentModelParticipant } from "$lib/chat/agents/identity";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { portal } from "$lib/utils/portal";
  import ChatModelAvatar from "./ChatModelAvatar.svelte";
  import ChatParticipantAvatar from "./ChatParticipantAvatar.svelte";
  import ProviderPlanLimits from "./ProviderPlanLimits.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";

  type IdentityPresentation = "avatar" | "name" | "mention";

  let {
    participant = null,
    model = null,
    presentation,
    triggerLabel = null,
    currentResponseSettings = false,
    size = 34,
    shape = "default",
    usageThreadId = null,
  }: {
    participant?: ChatParticipantRead | null;
    model?: ChatModelParticipant | null;
    presentation: IdentityPresentation;
    triggerLabel?: string | null;
    currentResponseSettings?: boolean;
    size?: number;
    shape?: "default" | "compact";
    /** Provider thread whose context usage the card shows while open, or `null` to show no usage. */
    usageThreadId?: ChatThreadId | null;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = useChat();
  const preferences = getPreferences();
  const settings = getSettingsLauncher();
  const cardId = `chat-identity-${crypto.randomUUID()}`;
  const previewDelayMs = 180;
  const closeDelayMs = 140;
  const viewportInsetPx = 8;
  const cardGapPx = 7;
  /** Card height assumed before the card has rendered once. */
  const fallbackCardHeightPx = 260;
  const cardWidthPx = FLOATING_WIDTH.lg;
  let triggerElement = $state<HTMLButtonElement | null>(null);
  let cardElement = $state<HTMLElement | null>(null);
  let open = $state(false);
  let pinned = $state(false);
  let lastPointerType: string | null = null;
  let cardStyle = $state("visibility:hidden");
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
  let closeTimer: ReturnType<typeof setTimeout> | null = null;
  const agent = $derived(participant?.kind === "ai_agent"
    ? chat.agentIdentities.find((entry) => entry.participant.id === participant.id) ?? null
    : null);
  const agentModel = $derived(participant?.kind === "ai_agent"
    ? chatAgentModelParticipant(participant.id, chat.agentIdentities, chat.settings)
    : null);
  const agentProvider = $derived(agent?.localProviderInstanceId
    ? chat.settings?.providerInstances.find((entry) => (
      entry.configuration.instanceId === agent.localProviderInstanceId
    )) ?? null
    : null);
  /** The provider whose plan limits the card shows: the agent's, or the one serving the model. */
  const cardProvider = $derived(agentProvider ?? (model
    ? chat.settings?.providerInstances.find((entry) => entry.configuration.familyId === model.providerFamilyId) ?? null
    : null));
  const displayName = $derived(participant
    ? chatParticipantDisplayName(
      participant,
      preferences.profileDisplayName,
      t("chat.timeline.you"),
    )
    : model?.displayName ?? "");
  /** The user's full name from their profile, shown under the display name on their own card. */
  const localFullName = $derived(preferences.profileFullName.trim());
  const visibleTriggerLabel = $derived(triggerLabel
    ?? (presentation === "mention" ? `@${displayName}` : displayName));
  const cardModel = $derived(agentModel ?? model);
  const cardReasoning = $derived(agentReasoning() ?? cardModel?.defaultReasoning ?? null);
  const formattedModelName = $derived(cardModel
    ? formatModelDisplayName(cardModel.displayName)
    : null);
  const formattedReasoning = $derived(cardReasoning
    ? compactModelOptionLabel(
        cardReasoning,
        cardModel?.providerFamilyId ?? null,
        t("chat.composer.light"),
        t("chat.composer.extraHigh"),
      )
    : null);
  const fastEnabled = $derived.by(() => {
    const policy = agent?.latestPolicy;
    if (!policy) return false;
    const modelDefinition = agentProvider?.modelCatalog?.models.find((candidate) => (
      candidate.id === policy.modelId
    )) ?? null;
    return agentExecutionSummary(policy.modelOptions, modelDefinition).speed === "fast";
  });
  let usageState = $state<ChatInteractionStateRead | null>(null);
  let usageReadFailed = $state(false);
  let usageRequest = 0;
  /** Whether a hover preview is counting down, so context usage can load before the card opens. */
  let previewPending = $state(false);
  const usageWanted = $derived(open || previewPending);
  const usageMeter = $derived(usageState ? composerMeterRead(usageState.usage, usageState.rateLimitStatus).meter : null);

  $effect(() => {
    const threadId = usageWanted ? usageThreadId : null;
    const request = ++usageRequest;
    usageState = null;
    usageReadFailed = false;
    if (!threadId) return;
    chatApi.readChatInteractionState(threadId)
      .then((read) => { if (request === usageRequest) usageState = read; })
      .catch(() => {
        // Context usage is advisory, so a failed read only says it is unavailable.
        if (request === usageRequest) usageReadFailed = true;
      });
  });

  onDestroy(() => {
    cancelPreview();
    cancelClose();
  });

  $effect(() => {
    if (!open) return;
    const reposition = () => positionCard();
    const closeFromOutside = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (triggerElement?.contains(target) || cardElement?.contains(target)) return;
      closeCard();
    };
    const closeFromKeyboard = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      closeCard();
      triggerElement?.focus();
    };
    void tick().then(reposition);
    // Context usage and plan limits arrive after the card opens, so a taller card is placed again.
    const resizeObserver = cardElement && typeof ResizeObserver !== "undefined" ? new ResizeObserver(reposition) : null;
    if (cardElement) {
      resizeObserver?.observe(cardElement);
      for (const child of cardElement.children) resizeObserver?.observe(child);
    }
    window.addEventListener("pointerdown", closeFromOutside, true);
    window.addEventListener("keydown", closeFromKeyboard, true);
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener("pointerdown", closeFromOutside, true);
      window.removeEventListener("keydown", closeFromKeyboard, true);
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  });

  function cancelPreview(): void {
    if (previewTimer === null) return;
    clearTimeout(previewTimer);
    previewTimer = null;
    previewPending = false;
  }

  function cancelClose(): void {
    if (closeTimer === null) return;
    clearTimeout(closeTimer);
    closeTimer = null;
  }

  function schedulePreview(): void {
    cancelClose();
    if (open || previewTimer !== null) return;
    previewPending = true;
    previewTimer = setTimeout(() => {
      previewTimer = null;
      open = true;
      previewPending = false;
    }, previewDelayMs);
  }

  function showFromFocus(): void {
    cancelPreview();
    cancelClose();
    open = true;
  }

  function scheduleClose(): void {
    cancelPreview();
    if (pinned) return;
    cancelClose();
    closeTimer = setTimeout(() => {
      closeTimer = null;
      open = false;
    }, closeDelayMs);
  }

  /**
   * Opens the card for keyboard and touch activation, which have no hover preview; a mouse click
   * leaves the hover preview as it is.
   */
  function togglePinned(event: MouseEvent): void {
    if (event.detail > 0 && lastPointerType === "mouse") return;
    cancelPreview();
    cancelClose();
    if (open && pinned) {
      closeCard();
      return;
    }
    open = true;
    pinned = true;
    if (event.detail === 0) {
      void tick().then(() => {
        const action = cardElement?.querySelector<HTMLElement>("button");
        (action ?? cardElement)?.focus();
      });
    }
  }

  function closeCard(): void {
    cancelPreview();
    cancelClose();
    open = false;
    pinned = false;
  }

  /**
   * Opens the card below its trigger when it fits there, above when it fits there, and otherwise
   * on the roomier side with its height capped to that space. The height is the card's full
   * content, so a capped card that scrolls still measures what it needs.
   */
  function positionCard(): void {
    if (!open || !triggerElement) return;
    const triggerBounds = triggerElement.getBoundingClientRect();
    const cardWidth = cardElement?.getBoundingClientRect().width ?? cardWidthPx;
    const cardHeight = cardElement
      ? cardElement.scrollHeight + cardElement.offsetHeight - cardElement.clientHeight
      : fallbackCardHeightPx;
    const availableBelow = Math.max(0, window.innerHeight - triggerBounds.bottom - cardGapPx - viewportInsetPx);
    const availableAbove = Math.max(0, triggerBounds.top - cardGapPx - viewportInsetPx);
    const openAbove = cardHeight > availableBelow
      && (cardHeight <= availableAbove || availableAbove > availableBelow);
    const maxHeight = openAbove ? availableAbove : availableBelow;
    const top = openAbove
      ? Math.max(viewportInsetPx, triggerBounds.top - cardGapPx - Math.min(cardHeight, maxHeight))
      : triggerBounds.bottom + cardGapPx;
    const idealLeft = triggerBounds.left;
    const left = Math.min(
      Math.max(viewportInsetPx, idealLeft),
      Math.max(viewportInsetPx, window.innerWidth - cardWidth - viewportInsetPx),
    );
    cardStyle = `left:${Math.round(left)}px;top:${Math.round(top)}px;max-height:${Math.round(maxHeight)}px;visibility:visible`;
  }

  function agentStatus(): string {
    if (participant?.archivedAt) return t("chat.organization.archivedParticipant");
    if (agent?.configurationState === "healthy") return t("chat.organization.availableParticipant");
    return t("chat.organization.needsSetup");
  }

  function agentReasoning(): string | null {
    const policy = agent?.latestPolicy;
    const selectedValue = policy?.modelOptions.find((option) => /effort|reasoning/iu.test(option.key))?.value;
    const value = typeof selectedValue === "string"
      ? selectedValue
      : selectedValue?.kind === "choice" ? selectedValue.value : null;
    if (!value) return agentModel?.defaultReasoning ?? null;
    const modelDefinition = agentProvider?.modelCatalog?.models.find((candidate) => (
      candidate.id === policy?.modelId
    ));
    const optionDefinition = modelDefinition?.options.find((option) => (
      option.kind === "choice" && /effort|reasoning/iu.test(`${option.key} ${option.label}`)
    ));
    if (optionDefinition?.kind !== "choice") return value;
    return optionDefinition.options.find((option) => option.value === value)?.label ?? value;
  }

  /** One line of context usage: the share used with the token counts, or the bare count when the window is unknown. */
  function contextUsageLabel(): string {
    if (!usageMeter) return t("chat.composer.contextUnavailable");
    const used = formatNumber(localization.locale, usageMeter.usedTokens);
    if (usageMeter.maximumTokens === null) return t("chat.composer.contextUnknown", used);
    const maximum = formatNumber(localization.locale, usageMeter.maximumTokens, { notation: "compact" });
    return usageMeter.ratio === null
      ? t("chat.composer.contextUsed", used, maximum)
      : t("chat.composer.contextSummary", contextPercentage(usageMeter.ratio), used, maximum);
  }

  function contextPercentage(value: number): string {
    return formatNumber(localization.locale, value * 100, { maximumFractionDigits: 0 });
  }

  function openIdentitySettings(): void {
    closeCard();
    if (participant?.kind === "local_user") {
      settings.open("profile");
      return;
    }
    if (participant?.kind === "ai_agent") {
      settings.open("agents", { agentsSubsection: "providers" });
    }
  }
</script>

<button
  bind:this={triggerElement}
  type="button"
  class="identity-trigger"
  class:avatar-trigger={presentation === "avatar"}
  class:name-trigger={presentation === "name"}
  class:mention-trigger={presentation === "mention"}
  aria-label={presentation === "avatar" ? t("chat.organization.identityDetailsFor", visibleTriggerLabel) : undefined}
  data-app-tooltip-disabled="true"
  aria-haspopup="dialog"
  aria-expanded={open}
  aria-controls={open ? cardId : undefined}
  onpointerdown={(event) => { lastPointerType = event.pointerType; }}
  onpointerenter={schedulePreview}
  onpointerleave={scheduleClose}
  onfocus={showFromFocus}
  onblur={scheduleClose}
  onclick={togglePinned}
>
  {#if presentation === "avatar"}
    {#if participant}<ChatParticipantAvatar {participant} {size} {shape} />
    {:else if model}<ChatModelAvatar familyId={model.company.iconFamilyId} label={model.company.name} {size} />{/if}
  {:else}{visibleTriggerLabel}{/if}</button>{#if open}
  <div
    bind:this={cardElement}
    use:portal
    id={cardId}
    class="identity-card surface-floating w-floating-lg"
    style={cardStyle}
    role="dialog"
    tabindex="-1"
    aria-label={t("chat.organization.identityDetailsFor", displayName)}
    onpointerenter={() => { cancelClose(); }}
    onpointerleave={scheduleClose}
  >
    <div class="identity-header">
      {#if participant}<ChatParticipantAvatar {participant} size={40} />
      {:else if model}<ChatModelAvatar familyId={model.company.iconFamilyId} label={model.company.name} size={40} />{/if}
      <div class="identity-heading">
        <div class="identity-name-line">
          <strong>{participant?.kind === "local_user" && preferences.profileDisplayName.trim() ? `${displayName} ${t("chat.organization.youSuffix")}` : displayName}</strong>
          {#if participant?.kind === "ai_agent"}
            <span
              class="identity-status-dot"
              class:available={agent?.configurationState === "healthy"}
              role="img"
              aria-label={agentStatus()}
            ></span>
          {/if}
        </div>
        {#if participant?.kind === "local_user"}
          {#if localFullName && localFullName !== displayName}<small>{localFullName}</small>{/if}
        {/if}
      </div>
    </div>

    {#if participant?.kind !== "local_user"}<div class="identity-body">
      {#if cardModel}
        <div class="identity-settings-summary">
          <span class="identity-settings-label">{currentResponseSettings ? t("chat.organization.currentResponseSettings") : t("chat.organization.defaultSettings")}</span>
          <div class="identity-model-row">
            {#if fastEnabled}<span class="identity-fast-indicator" aria-label={t("chat.composer.fastEnabled")}><Zap size={13} fill="currentColor" /></span>{/if}
            <span class="identity-model-name">{formattedModelName}</span>
            {#if formattedReasoning}<span class="identity-effort-name">{formattedReasoning}</span>{/if}
          </div>
        </div>
      {:else}
        <p class="identity-description">{t("chat.organization.collaboratorIdentity")}</p>
      {/if}
      {#if usageThreadId}
        <div class="identity-context" data-identity-context aria-busy={!usageState && !usageReadFailed}>
          <span class="identity-settings-label">{t("chat.composer.contextWindow")}</span>
          {#if usageState}
            {#if usageMeter}<span>{contextUsageLabel()}</span>{:else}<small>{contextUsageLabel()}</small>{/if}
            {#if usageState.automaticCompactionReported || usageMeter?.warning}<small>{t("chat.composer.contextCompaction")}</small>{/if}
          {:else if usageReadFailed}<small>{t("chat.composer.contextUnavailable")}</small>
          {:else}<span class="identity-context-pending">{t("common.loading")}</span>{/if}
        </div>
      {/if}
      {#if cardProvider}<ProviderPlanLimits provider={cardProvider} />{/if}
    </div>{/if}

    {#if participant?.kind === "local_user" || participant?.kind === "ai_agent"}
      <footer>
        <button type="button" onclick={openIdentitySettings}><Settings size={13} />{participant.kind === "local_user" ? t("chat.organization.openProfileSettings") : t("chat.organization.openAgentSettings")}</button>
      </footer>
    {/if}
  </div>
{/if}

<style>
  .identity-trigger { min-width:0; color:inherit; text-align:left; }
  .identity-trigger:focus-visible { border-radius:0.25rem; outline:2px solid var(--ring); outline-offset:2px; }
  .avatar-trigger { display:grid; border-radius:22%; }
  .name-trigger { overflow:hidden; font:inherit; font-weight:inherit; text-overflow:ellipsis; white-space:nowrap; }
  .name-trigger:hover,.name-trigger[aria-expanded="true"] { color:color-mix(in srgb,var(--primary) 72%,var(--foreground)); }
  .mention-trigger { display:inline; border-radius:0.28rem; background:color-mix(in srgb,var(--primary) 14%,transparent); padding:0.05em 0.22em; color:color-mix(in srgb,var(--primary) 76%,var(--foreground)); font:inherit; font-weight:650; line-height:inherit; box-decoration-break:clone; -webkit-box-decoration-break:clone; }
  .mention-trigger:hover,.mention-trigger[aria-expanded="true"] { background:color-mix(in srgb,var(--primary) 23%,transparent); color:var(--foreground); }
  .identity-card { position:fixed; z-index:140; max-height:calc(100vh - 1rem); overflow:auto; overscroll-behavior:contain; }
  .identity-header { display:grid; grid-template-columns:40px minmax(0,1fr); align-items:center; gap:0.65rem; padding:0.75rem 0.8rem 0.4rem; }
  .identity-heading { display:grid; min-width:0; }
  .identity-name-line { display:flex; min-width:0; align-items:center; gap:0.38rem; }
  .identity-heading strong { overflow:hidden; font-size:calc(0.9rem * var(--type-scale)); font-weight:650; line-height:1.2rem; text-overflow:ellipsis; white-space:nowrap; }
  .identity-heading small { overflow:hidden; color:var(--muted-foreground); font-size:var(--panel-detail-font-size); text-overflow:ellipsis; white-space:nowrap; }
  .identity-status-dot { width:0.42rem; height:0.42rem; flex:0 0 auto; border-radius:999px; background:var(--status-tentative); }
  .identity-status-dot.available { background:var(--action-confirm-signal); }
  .identity-body { display:grid; gap:0.65rem; padding:0.3rem 0.8rem 0.7rem; }
  .identity-description { overflow-wrap:anywhere; font-size:var(--panel-font-size); line-height:1.1rem; }
  .identity-settings-summary { display:grid; min-width:0; gap:0.18rem; }
  .identity-settings-label { color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  .identity-model-row { display:flex; min-width:0; align-items:center; gap:0.3rem; color:var(--foreground); font-size:var(--panel-font-size); }
  .identity-fast-indicator { display:grid; flex:0 0 auto; place-items:center; }
  .identity-model-name { overflow:hidden; min-width:0; text-overflow:ellipsis; white-space:nowrap; }
  .identity-effort-name { flex:0 0 auto; }
  .identity-context { display:grid; min-width:0; gap:0.22rem; font-size:var(--panel-font-size); }
  .identity-context small { color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  .identity-context-pending { color:var(--muted-foreground); }
  footer { display:flex; justify-content:flex-end; padding:0.25rem 0.65rem 0.5rem; }
  footer button { display:inline-flex; min-height:1.75rem; align-items:center; gap:0.35rem; border-radius:var(--floating-item-radius); padding:0.25rem 0.45rem; color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  footer button:hover,footer button:focus-visible { background:var(--accent); color:var(--foreground); }
  @media (forced-colors:active) { .identity-card { border-color:CanvasText; } }
</style>
