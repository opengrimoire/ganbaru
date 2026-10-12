<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import Settings from "@lucide/svelte/icons/settings";
  import Zap from "@lucide/svelte/icons/zap";
  import type { ChatParticipantRead, ChatRuntimeApprovalPolicy } from "$lib/chat/contracts";
  import { chatParticipantDisplayName } from "$lib/chat/teammates/participant-display";
  import type { ChatModelParticipant } from "$lib/chat/teammates/participant-identity";
  import { compactModelName, compactModelOptionLabel } from "$lib/chat/composer/model-picker-model";
  import { teammateExecutionSummary } from "$lib/chat/teammates/draft";
  import { chatTeammateModelParticipant } from "$lib/chat/teammates/identity";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { portal } from "$lib/utils/portal";
  import ChatModelAvatar from "./ChatModelAvatar.svelte";
  import ChatParticipantAvatar from "./ChatParticipantAvatar.svelte";
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
  }: {
    participant?: ChatParticipantRead | null;
    model?: ChatModelParticipant | null;
    presentation: IdentityPresentation;
    triggerLabel?: string | null;
    currentResponseSettings?: boolean;
    size?: number;
    shape?: "default" | "compact";
  } = $props();

  const { t } = getLocalization();
  const chat = getChat();
  const preferences = getPreferences();
  const settings = getSettingsLauncher();
  const cardId = `chat-identity-${crypto.randomUUID()}`;
  const previewDelayMs = 180;
  const closeDelayMs = 140;
  const viewportInsetPx = 8;
  const cardGapPx = 7;
  const cardWidthPx = FLOATING_WIDTH.lg;
  let triggerElement = $state<HTMLButtonElement | null>(null);
  let cardElement = $state<HTMLElement | null>(null);
  let open = $state(false);
  let pinned = $state(false);
  let cardStyle = $state("visibility:hidden");
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
  let closeTimer: ReturnType<typeof setTimeout> | null = null;
  const teammate = $derived(participant?.kind === "ai_teammate"
    ? chat.teammateIdentities.find((entry) => entry.participant.id === participant.id) ?? null
    : null);
  const teammateModel = $derived(participant?.kind === "ai_teammate"
    ? chatTeammateModelParticipant(participant.id, chat.teammateIdentities, chat.settings)
    : null);
  const teammateProvider = $derived(teammate?.latestPolicy
    ? chat.settings?.providerInstances.find((entry) => (
      entry.configuration.instanceId === teammate.latestPolicy?.providerInstanceId
    )) ?? null
    : null);
  const displayName = $derived(participant
    ? chatParticipantDisplayName(
      participant,
      preferences.profileDisplayName,
      t("chat.timeline.you"),
    )
    : model?.displayName ?? "");
  const visibleTriggerLabel = $derived(triggerLabel
    ?? (presentation === "mention" ? `@${displayName}` : displayName));
  const cardModel = $derived(teammateModel ?? model);
  const cardReasoning = $derived(teammateReasoning() ?? cardModel?.defaultReasoning ?? null);
  const formattedModelName = $derived(cardModel
    ? compactModelName(cardModel.displayName, cardModel.providerFamilyId)
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
    const policy = teammate?.latestPolicy;
    if (!policy) return false;
    if (policy.speed !== null) return policy.speed === "fast";
    const modelDefinition = teammateProvider?.modelCatalog?.models.find((candidate) => (
      candidate.id === policy.modelId
    )) ?? null;
    return teammateExecutionSummary(policy.modelOptions, modelDefinition).speed === "fast";
  });
  const approvalPolicy = $derived(participant?.kind === "ai_teammate"
      ? chat.selectedChannel?.memberships.find((membership) => (
        membership.participant.id === participant.id && membership.removedAt === null
      ))?.aiAccess?.runtimeApprovalOverride ?? null
    : null);
  const approvalLabel = $derived(approvalPolicy ? approvalPolicyLabel(approvalPolicy) : null);

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
    window.addEventListener("pointerdown", closeFromOutside, true);
    window.addEventListener("keydown", closeFromKeyboard, true);
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
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
  }

  function cancelClose(): void {
    if (closeTimer === null) return;
    clearTimeout(closeTimer);
    closeTimer = null;
  }

  function schedulePreview(): void {
    cancelClose();
    if (open || previewTimer !== null) return;
    previewTimer = setTimeout(() => {
      previewTimer = null;
      open = true;
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

  function togglePinned(event: MouseEvent): void {
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

  function positionCard(): void {
    if (!open || !triggerElement) return;
    const triggerBounds = triggerElement.getBoundingClientRect();
    const cardBounds = cardElement?.getBoundingClientRect();
    const cardHeight = cardBounds?.height ?? 260;
    const cardWidth = cardBounds?.width ?? cardWidthPx;
    const availableBelow = window.innerHeight - triggerBounds.bottom - viewportInsetPx;
    const top = availableBelow >= cardHeight + cardGapPx
      ? triggerBounds.bottom + cardGapPx
      : Math.max(viewportInsetPx, triggerBounds.top - cardHeight - cardGapPx);
    const idealLeft = triggerBounds.left;
    const left = Math.min(
      Math.max(viewportInsetPx, idealLeft),
      Math.max(viewportInsetPx, window.innerWidth - cardWidth - viewportInsetPx),
    );
    cardStyle = `left:${Math.round(left)}px;top:${Math.round(top)}px;visibility:visible`;
  }

  function teammateStatus(): string {
    if (participant?.archivedAt) return t("chat.organization.archivedParticipant");
    if (teammate?.configurationState === "healthy") return t("chat.organization.availableParticipant");
    return t("chat.organization.needsSetup");
  }

  function approvalPolicyLabel(policy: ChatRuntimeApprovalPolicy): string {
    if (policy === "ask") return t("settings.chat.teammates.runtime.ask");
    if (policy === "autoApprove") return t("settings.chat.teammates.runtime.autoApprove");
    if (policy === "unattended") return t("settings.chat.teammates.runtime.unattended");
    return t("settings.chat.teammates.runtime.providerCustom");
  }

  function teammateReasoning(): string | null {
    const policy = teammate?.latestPolicy;
    const selectedValue = policy?.effort
      ?? policy?.modelOptions.find((option) => /effort|reasoning/iu.test(option.key))?.value;
    const value = typeof selectedValue === "string"
      ? selectedValue
      : selectedValue?.kind === "choice" ? selectedValue.value : null;
    if (!value) return teammateModel?.defaultReasoning ?? null;
    const modelDefinition = teammateProvider?.modelCatalog?.models.find((candidate) => (
      candidate.id === policy?.modelId
    ));
    const optionDefinition = modelDefinition?.options.find((option) => (
      option.kind === "choice" && /effort|reasoning/iu.test(`${option.key} ${option.label}`)
    ));
    if (optionDefinition?.kind !== "choice") return value;
    return optionDefinition.options.find((option) => option.value === value)?.label ?? value;
  }

  function openIdentitySettings(): void {
    closeCard();
    if (participant?.kind === "local_user") {
      settings.open("profile");
      return;
    }
    if (participant?.kind === "ai_teammate") {
      settings.open("chat", { chatSubsection: "teammates", chatTeammateId: participant.id });
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
  aria-haspopup="dialog"
  aria-expanded={open}
  aria-controls={open ? cardId : undefined}
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
          <strong>{displayName}</strong>
          {#if participant?.kind === "ai_teammate"}
            <span
              class="identity-status-dot"
              class:available={teammate?.configurationState === "healthy"}
              role="img"
              aria-label={teammateStatus()}
            ></span>
          {/if}
        </div>
        {#if participant?.kind === "local_user"}<small>{t("chat.organization.localProfile")}</small>
        {:else if participant?.kind === "ai_teammate" && teammate?.role}<small>{teammate.role}</small>
        {/if}
      </div>
    </div>

    <div class="identity-body">
      {#if participant?.kind === "local_user"}
        <div class="privacy-note"><LockKeyhole size={14} /><span>{t("chat.organization.localProfileDetail")}</span></div>
      {:else if cardModel}
        <div class="identity-settings-summary">
          <span class="identity-settings-label">{currentResponseSettings ? t("chat.organization.currentResponseSettings") : t("chat.organization.defaultSettings")}</span>
          <div class="identity-model-row">
            {#if fastEnabled}<span class="identity-fast-indicator" aria-label={t("chat.composer.fastEnabled")}><Zap size={13} fill="currentColor" /></span>{/if}
            <span class="identity-model-name">{formattedModelName}</span>
            {#if formattedReasoning}<span class="identity-effort-name">{formattedReasoning}</span>{/if}
            {#if approvalLabel}
              <span class="identity-model-divider" aria-hidden="true">|</span>
              <span class="identity-approval">{approvalLabel}</span>
            {/if}
          </div>
        </div>
      {:else}
        <p class="identity-description">{t("chat.organization.collaboratorIdentity")}</p>
      {/if}
    </div>

    {#if participant?.kind === "local_user" || participant?.kind === "ai_teammate"}
      <footer>
        <button type="button" onclick={openIdentitySettings}><Settings size={13} />{participant.kind === "local_user" ? t("chat.organization.openProfileSettings") : t("chat.organization.openTeammateSettings")}</button>
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
  .identity-status-dot.available { background:var(--action-confirm); }
  .identity-body { display:grid; gap:0.65rem; padding:0.3rem 0.8rem 0.7rem; }
  .identity-description { overflow-wrap:anywhere; font-size:var(--panel-font-size); line-height:1.1rem; }
  .identity-settings-summary { display:grid; min-width:0; gap:0.18rem; }
  .identity-settings-label { color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  .identity-model-row { display:flex; min-width:0; align-items:center; gap:0.3rem; color:var(--foreground); font-size:var(--panel-font-size); }
  .identity-fast-indicator { display:grid; flex:0 0 auto; place-items:center; }
  .identity-model-name { overflow:hidden; min-width:0; text-overflow:ellipsis; white-space:nowrap; }
  .identity-effort-name { flex:0 0 auto; }
  .identity-model-divider { flex:0 0 auto; color:var(--muted-foreground); }
  .identity-approval { overflow:hidden; min-width:0; color:var(--foreground); text-overflow:ellipsis; white-space:nowrap; }
  .privacy-note { display:grid; grid-template-columns:1rem minmax(0,1fr); gap:0.55rem; align-items:start; color:var(--muted-foreground); }
  .privacy-note :global(svg) { margin-top:0.1rem; color:color-mix(in srgb,var(--primary) 70%,var(--foreground)); }
  .privacy-note span { font-size:var(--panel-detail-font-size); line-height:1rem; }
  footer { display:flex; justify-content:flex-end; padding:0.25rem 0.65rem 0.5rem; }
  footer button { display:inline-flex; min-height:1.75rem; align-items:center; gap:0.35rem; border-radius:var(--floating-item-radius); padding:0.25rem 0.45rem; color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  footer button:hover,footer button:focus-visible { background:var(--accent); color:var(--foreground); }
  @media (forced-colors:active) { .identity-card { border-color:CanvasText; } }
</style>
