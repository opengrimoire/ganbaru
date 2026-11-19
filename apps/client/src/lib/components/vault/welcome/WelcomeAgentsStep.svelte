<script lang="ts">
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { onMount } from "svelte";
  import { probeChatProvider, readChatSettings, refreshAllChatProviders } from "$lib/api/chat/settings";
  import { familiesInPickerOrder } from "$lib/chat/composer/model-picker-model";
  import type { ChatSettingsRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { providerFamilyAvailabilities, type ProviderFamilyAvailability } from "$lib/chat/provider-availability";
  import { providerCliSummary, providerFamilyCliName, providerFamilyDisplayName } from "$lib/chat/provider-setup";
  import ProviderHeading, { type ProviderDisplayState } from "$lib/components/settings/agents/ProviderHeading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import TooltipHost from "$lib/components/ui/TooltipHost.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import WelcomeAgentSetupDialog, { type WelcomeAgentFamily, type WelcomeAgentSetupView } from "./WelcomeAgentSetupDialog.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /**
   * The desktop welcome screen after Chat: a card for every AI agent this computer can run, checked as the screen
   * opens, with the same state, CLI version, account, and plan limits Settings shows. Each card checks its agent again
   * and opens its setup in a dialog: the install and sign-in steps, or the full setup form of Settings.
   */
  const { t } = getLocalization();

  type AgentCard = ProviderFamilyAvailability & { family: WelcomeAgentFamily };

  let settings = $state<ChatSettingsRead | null>(null);
  let checkingAll = $state(true);
  let probingIds = $state<string[]>([]);
  let checkError = $state<string | null>(null);
  let setup = $state<{ familyId: string; view: WelcomeAgentSetupView } | null>(null);
  let mounted = true;

  const cards = $derived.by((): AgentCard[] => {
    if (!settings) return [];
    const families = settings.providerFamilies.map((family) => ({
      ...family,
      displayName: providerFamilyDisplayName(family, t("chat.providers.acp.name")),
    }));
    return providerFamilyAvailabilities(familiesInPickerOrder(families, settings.providerInstances, Date.now()), settings);
  });
  const setupCard = $derived(setup ? cards.find((card) => card.family.familyId === setup?.familyId) ?? null : null);

  let inFlight: Promise<void> | null = null;

  /** Checks every agent and adds a CLI installed since, joining a check already running. */
  function checkAll(): Promise<void> {
    inFlight ??= runCheckAll().finally(() => {
      inFlight = null;
    });
    return inFlight;
  }

  /** Shows what this device already knows at once, then checks every agent again, which can take a few seconds. */
  async function runCheckAll(): Promise<void> {
    checkingAll = true;
    checkError = null;
    try {
      if (!settings) {
        const cached = await readChatSettings();
        if (mounted) settings = cached;
      }
      await refreshAllChatProviders();
      await reloadSettings();
    } catch (error: unknown) {
      if (mounted) checkError = chatErrorMessage(error, t("vaultSetup.agents.checkFailed"));
    } finally {
      if (mounted) checkingAll = false;
    }
  }

  async function reloadSettings(): Promise<void> {
    const refreshed = await readChatSettings();
    if (mounted) settings = refreshed;
  }

  /** Checks one agent: its configuration when it has one, or every agent so a newly installed CLI is added. */
  async function checkCard(card: AgentCard): Promise<void> {
    if (!card.instance) return checkAll();
    const instanceId = card.instance.configuration.instanceId;
    if (probingIds.includes(instanceId)) return;
    probingIds = [...probingIds, instanceId];
    checkError = null;
    try {
      await probeChatProvider(instanceId);
      await reloadSettings();
    } catch (error: unknown) {
      if (mounted) checkError = chatErrorMessage(error, t("vaultSetup.agents.checkFailed"));
    } finally {
      if (mounted) probingIds = probingIds.filter((id) => id !== instanceId);
    }
  }

  function cardBusy(card: AgentCard): boolean {
    return checkingAll || (card.instance !== null && probingIds.includes(card.instance.configuration.instanceId));
  }

  onMount(() => {
    void checkAll();
    return () => {
      mounted = false;
    };
  });

  function displayState({ availability }: AgentCard): ProviderDisplayState {
    switch (availability.kind) {
      case "ready": return "healthy";
      case "disabled": return "disabled";
      case "installed":
      case "not-installed": return "unconfigured";
      case "signed-out":
      case "missing-at":
      case "unchecked":
      case "attention": return "attention";
    }
  }

  function stateLabel({ availability }: AgentCard): string {
    switch (availability.kind) {
      case "ready": return t("settings.agents.providers.ready");
      case "disabled": return t("settings.agents.providers.disabled");
      case "installed": return t("settings.agents.providers.notConfigured");
      case "not-installed":
      case "missing-at": return t("settings.agents.providers.notInstalled");
      case "signed-out": return t("settings.agents.providers.authenticationRequired");
      case "unchecked": return t("settings.agents.providers.neverChecked");
      case "attention": return t("settings.agents.providers.needsAttention");
    }
  }

  /**
   * The setup an agent's card opens: the install or sign-in steps while the CLI is missing or signed out and the family
   * has a guide, and the full setup form otherwise.
   */
  function setupView({ family, availability, instance }: AgentCard): WelcomeAgentSetupView {
    const advanced = instance
      ? { mode: "edit" as const, instanceId: instance.configuration.instanceId }
      : { mode: "create" as const, familyId: family.familyId };
    const guide = family.installGuide;
    switch (availability.kind) {
      case "signed-out":
      case "missing-at":
      case "installed":
      case "not-installed":
        return guide ? { kind: "guide", guide, situation: availability, advanced } : { kind: "advanced", target: advanced };
      case "ready":
      case "disabled":
      case "unchecked":
      case "attention":
        return { kind: "advanced", target: advanced };
    }
  }

  function openSetup(card: AgentCard): void {
    setup = { familyId: card.family.familyId, view: setupView(card) };
  }

  /** Checks the agent whose setup is open, closing it once the agent is ready and keeping its steps current. */
  async function refreshSetup(card: AgentCard): Promise<void> {
    await checkCard(card);
    const current = cards.find((candidate) => candidate.family.familyId === card.family.familyId);
    if (!setup || setup.familyId !== card.family.familyId || !current) return;
    if (current.availability.kind === "ready") {
      setup = null;
      return;
    }
    const next = setupView(current);
    if (setup.view.kind === "guide" && next.kind === "guide") setup.view = next;
  }

  function closeSetup(): void {
    setup = null;
    void reloadSettings().catch((error: unknown) => {
      if (mounted) checkError = chatErrorMessage(error, t("vaultSetup.agents.checkFailed"));
    });
  }
</script>

<TooltipHost />

<WelcomeSceneStep headline={t("vaultSetup.agents.headline")} body={t("vaultSetup.agents.body")}>
  <div class="@container flex flex-col gap-5">
    {#if checkError}
      <Notice message={checkError} onDismiss={() => (checkError = null)} />
    {/if}
    {#if cards.length > 0}
      <ul
        class="grid grid-cols-1 gap-3 @min-[480px]:grid-cols-2 @min-[760px]:grid-cols-3"
        aria-label={t("vaultSetup.agents.listLabel")}
        aria-busy={checkingAll}
      >
        {#each cards as card (card.family.familyId)}
          {@const busy = cardBusy(card)}
          <li class="flex min-h-16 min-w-0 items-center rounded-lg border border-border bg-card px-2.5 py-3">
            <div class="min-w-0 flex-1">
              <ProviderHeading
                familyId={card.family.familyId}
                familyName={card.family.displayName}
                title={card.family.displayName}
                state={displayState(card)}
                stateLabel={stateLabel(card)}
                subtitle={card.instance ? providerCliSummary(card.family, card.instance) : providerFamilyCliName(card.family)}
                provider={card.instance}
                avatarSize={34}
              >
                {#snippet actions()}
                  <button
                    type="button"
                    class="agent-action"
                    disabled={busy}
                    aria-label={t("settings.agents.providers.refresh")}
                    data-app-tooltip={t("settings.agents.providers.refresh")}
                    onclick={() => void checkCard(card)}
                  >
                    <RefreshCw size={14} class={busy ? "animate-spin" : undefined} />
                  </button>
                  {#if card.instance}
                    <button
                      type="button"
                      class="agent-action"
                      aria-label={t("settings.agents.providers.configure")}
                      data-app-tooltip={t("settings.agents.providers.configure")}
                      onclick={() => openSetup(card)}
                    >
                      <Pencil size={14} />
                    </button>
                  {:else}
                    <button
                      type="button"
                      class="agent-action"
                      aria-label={t("settings.agents.providers.setUp", card.family.displayName)}
                      data-app-tooltip={t("settings.agents.providers.setUp", card.family.displayName)}
                      onclick={() => openSetup(card)}
                    >
                      <Plus size={14} />
                    </button>
                  {/if}
                {/snippet}
              </ProviderHeading>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
    <p class="text-sm leading-6 text-muted-foreground">{t("vaultSetup.agents.later")}</p>
  </div>
</WelcomeSceneStep>

{#if setup && setupCard}
  <WelcomeAgentSetupDialog
    family={setupCard.family}
    bind:view={setup.view}
    onRefresh={() => refreshSetup(setupCard)}
    onClose={closeSetup}
  />
{/if}

<style>
  .agent-action { display:inline-grid; width:1.75rem; height:1.75rem; place-items:center; border:1px solid var(--border); border-radius:0.375rem; background:var(--background); color:var(--muted-foreground); }
  .agent-action:hover:not(:disabled) { background:var(--accent); color:var(--foreground); }
  .agent-action:disabled { cursor:default; }
  :global(.dark) .agent-action { background:transparent; }
</style>
