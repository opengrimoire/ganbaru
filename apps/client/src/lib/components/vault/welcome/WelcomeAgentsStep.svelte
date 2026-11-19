<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import { onMount } from "svelte";
  import { readChatSettings, refreshAllChatProviders } from "$lib/api/chat/settings";
  import { familiesInPickerOrder } from "$lib/chat/composer/model-picker-model";
  import type { ChatSettingsRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { providerFamilyAvailabilities, type ProviderAvailability } from "$lib/chat/provider-availability";
  import { providerFamilyDisplayName } from "$lib/chat/provider-setup";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import ProviderInstallSteps from "$lib/components/settings/agents/ProviderInstallSteps.svelte";
  import type { ProviderInstallSituation } from "$lib/components/settings/agents/provider-install-steps";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { WELCOME_LINK_CLASS } from "$lib/vault/welcome-layout";
  import { cn } from "$lib/utils";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /**
   * The desktop welcome screen after Chat: every coding agent this computer can run, checked as the screen opens, with
   * whether it is ready and signed in. An agent that is not opens the same install and sign-in steps Settings shows,
   * and checking again adds a CLI the person just installed, as opening Settings does.
   */
  const { t } = getLocalization();

  let settings = $state<ChatSettingsRead | null>(null);
  let checking = $state(true);
  let checkError = $state<string | null>(null);
  let expandedFamilyId = $state<string | null>(null);
  let mounted = true;

  const rows = $derived.by(() => {
    if (!settings) return [];
    const families = settings.providerFamilies.map((family) => ({
      ...family,
      displayName: providerFamilyDisplayName(family, t("chat.providers.acp.name")),
    }));
    return providerFamilyAvailabilities(
      familiesInPickerOrder(families, settings.providerInstances, Date.now()),
      settings,
    );
  });

  /** Reads what this device already knows at once, then checks every provider again, which can take a few seconds. */
  async function check(): Promise<void> {
    checking = true;
    checkError = null;
    try {
      if (!settings) {
        const cached = await readChatSettings();
        if (mounted) settings = cached;
      }
      await refreshAllChatProviders();
      const refreshed = await readChatSettings();
      if (mounted) settings = refreshed;
    } catch (error: unknown) {
      if (mounted) checkError = chatErrorMessage(error, t("vaultSetup.agents.checkFailed"));
    } finally {
      if (mounted) checking = false;
    }
  }

  onMount(() => {
    void check();
    return () => {
      mounted = false;
    };
  });

  /** The install guide's starting point for an agent that is not ready, or `null` when only Settings can help. */
  function installSituation(availability: ProviderAvailability): ProviderInstallSituation | null {
    switch (availability.kind) {
      case "signed-out":
      case "missing-at":
      case "installed":
      case "not-installed":
        return availability;
      case "ready":
      case "unchecked":
      case "attention":
        return null;
    }
  }

  function statusLabel(availability: ProviderAvailability): string {
    if (checking && availability.kind !== "ready") return t("vaultSetup.agents.status.checking");
    switch (availability.kind) {
      case "ready":
        return availability.accountLabel
          ? t("vaultSetup.agents.status.readyAs", availability.accountLabel)
          : t("vaultSetup.agents.status.ready");
      case "signed-out": return t("vaultSetup.agents.status.signedOut");
      case "installed": return t("vaultSetup.agents.status.installed");
      case "not-installed": return t("vaultSetup.agents.status.notInstalled");
      case "missing-at": return t("vaultSetup.agents.status.missingAt");
      case "unchecked": return t("vaultSetup.agents.status.unchecked");
      case "attention": return t("vaultSetup.agents.status.attention");
    }
  }

  function statusDotClass(availability: ProviderAvailability): string {
    if (availability.kind === "ready") return "bg-(--action-confirm-signal)";
    if (checking) return "bg-muted-foreground opacity-70";
    return availability.kind === "not-installed" ? "bg-destructive" : "bg-(--status-tentative)";
  }

  function toggle(familyId: string): void {
    expandedFamilyId = expandedFamilyId === familyId ? null : familyId;
  }
</script>

<WelcomeSceneStep headline={t("vaultSetup.agents.headline")} body={t("vaultSetup.agents.body")}>
  <div class="flex max-w-2xl flex-col gap-4">
    {#if checkError}
      <Notice message={checkError} onDismiss={() => (checkError = null)} />
    {/if}
    {#if rows.length > 0}
      <ul class="flex flex-col divide-y divide-border rounded-lg border border-border" aria-label={t("vaultSetup.agents.listLabel")} aria-busy={checking}>
        {#each rows as { family, availability } (family.familyId)}
          {@const situation = installSituation(availability)}
          {@const expanded = expandedFamilyId === family.familyId}
          {#snippet summary()}
            <ChatModelAvatar familyId={family.familyId} label={family.displayName} size={32} />
            <span class="flex min-w-0 flex-1 flex-col">
              <strong class="truncate text-sm font-semibold text-foreground">{family.displayName}</strong>
              <span class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                <i class={cn("size-1.5 shrink-0 rounded-full", statusDotClass(availability))} aria-hidden="true"></i>
                <span class="truncate">{statusLabel(availability)}</span>
              </span>
            </span>
          {/snippet}
          <li class="flex flex-col">
            {#if situation && family.installGuide}
              <button
                type="button"
                class="flex min-h-14 w-full min-w-0 items-center gap-3 px-3 py-2 text-left hover:bg-accent"
                aria-expanded={expanded}
                onclick={() => toggle(family.familyId)}
              >
                {@render summary()}
                <ChevronDown size={16} class={cn("shrink-0 text-muted-foreground transition-transform", expanded && "rotate-180")} aria-hidden="true" />
              </button>
              {#if expanded}
                <div class="px-3 pt-1 pb-3">
                  <ProviderInstallSteps {family} guide={family.installGuide} {situation} onRefresh={check} />
                </div>
              {/if}
            {:else}
              <div class="flex min-h-14 min-w-0 items-center gap-3 px-3 py-2">{@render summary()}</div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    <div class="flex flex-wrap items-center gap-x-5">
      <button type="button" class={WELCOME_LINK_CLASS} disabled={checking} onclick={() => void check()}>
        {t("vaultSetup.agents.checkAgain")}
      </button>
    </div>
    <p class="text-sm leading-6 text-muted-foreground">{t("vaultSetup.agents.later")}</p>
  </div>
</WelcomeSceneStep>
