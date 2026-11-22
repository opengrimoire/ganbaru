<script lang="ts">
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { ProviderFamilyMetadataRead, ProviderInstallGuide } from "$lib/chat/contracts";
  import CommandBox from "$lib/components/ui/CommandBox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import NumberedStep from "$lib/components/ui/NumberedStep.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { installDocsLabel, situationNeedsInstall, type ProviderInstallSituation } from "./provider-install-steps";

  /**
   * What this device knows about a provider's CLI, followed by the provider's own steps to
   * install it and sign in, one at a time with a copy button for each command, and a link to
   * the provider's guide. Ganbaru shows the commands and never runs them. With `onRefresh`, the
   * last step holds a Refresh button; without it, the step points to the Set up action instead.
   */
  let {
    family,
    guide,
    situation,
    onRefresh,
  }: {
    family: ProviderFamilyMetadataRead;
    guide: ProviderInstallGuide;
    situation: ProviderInstallSituation;
    /** Checks the provider again once the terminal steps are done; the caller reports failures. */
    onRefresh?: () => Promise<void>;
  } = $props();

  type InstallStep = "open" | "requirement" | "requirement-reopen" | "install" | "reopen" | "sign-in" | "finish";

  const { t } = getLocalization();
  /**
   * The parent keeps this component mounted across provider families, so the refresh in flight and the guide link
   * error name the family they belong to and only show while that family is shown. A late result for a family the
   * parent left behind never reaches the next one.
   */
  let refreshInFlight = $state.raw<{ readonly familyId: string } | null>(null);
  let guideLinkError = $state.raw<{ readonly familyId: string; readonly message: string } | null>(null);
  const refreshing = $derived(refreshInFlight?.familyId === family.familyId);
  const linkError = $derived(guideLinkError?.familyId === family.familyId ? guideLinkError.message : null);

  const cliName = $derived(family.defaultExecutableCandidates[0] || family.familyId);
  const powerShell = $derived(guide.shell === "powershell");
  const prompt = $derived(powerShell ? "PS>" : "$");
  const needsInstall = $derived(situationNeedsInstall(situation));
  const requirement = $derived(needsInstall ? guide.requirement : null);
  /** The steps in order; a terminal only sees the `PATH` an installer changed once it is opened again. */
  const steps = $derived.by((): InstallStep[] => {
    if (!needsInstall) return ["open", "sign-in", "finish"];
    const prepare: InstallStep[] = requirement ? ["requirement", "requirement-reopen"] : [];
    return ["open", ...prepare, "install", "reopen", "sign-in", "finish"];
  });
  const statusMessage = $derived.by(() => {
    switch (situation.kind) {
      case "installed": return t("settings.agents.providers.install.installed", cliName, situation.path);
      case "missing-at": return t("settings.agents.providers.install.missingAt", cliName, situation.path);
      case "signed-out": return t("settings.agents.providers.install.signedOut", family.displayName);
      case "not-installed":
      case "unknown": return null;
    }
  });

  async function refresh(): Promise<void> {
    if (!onRefresh || refreshing) return;
    const request = { familyId: family.familyId };
    refreshInFlight = request;
    try {
      await onRefresh();
    } finally {
      if (refreshInFlight === request) refreshInFlight = null;
    }
  }

  /** Opens the provider's guide, which validation already holds to `https`. */
  async function openGuide(): Promise<void> {
    const familyId = family.familyId;
    guideLinkError = null;
    try {
      await openUrl(guide.docsUrl);
    } catch (cause: unknown) {
      console.warn("the provider install guide could not be opened", cause);
      guideLinkError = { familyId, message: t("settings.agents.providers.install.guideFailed") };
    }
  }
</script>

<div class="flex w-full max-w-md min-w-0 flex-col gap-3">
  {#if statusMessage}
    <Notice
      tone={situation.kind === "installed" ? "neutral" : "warning"}
      icon={situation.kind === "installed" ? CircleCheck : undefined}
      role="status"
      class="text-[0.8rem]"
      message={statusMessage}
      copy={false}
    />
  {/if}
  <ol class="flex flex-col">
    {#each steps as step, index (step)}
      {@const number = index + 1}
      {#if step === "open"}
        <NumberedStep {number} total={steps.length}>
          <p class="min-w-0 flex-1 pt-0.5 pb-4 text-[0.8rem] leading-5 text-foreground">
            {powerShell
              ? t("settings.agents.providers.install.openPowerShell")
              : t("settings.agents.providers.install.openTerminal")}
          </p>
        </NumberedStep>
      {:else if step === "requirement" && requirement}
        <NumberedStep {number} total={steps.length}>
          <div class="min-w-0 flex-1 pt-0.5 pb-4">
            <p class="text-[0.8rem] leading-5 text-foreground">
              {t("settings.agents.providers.install.runRequirementInstall", requirement.name, cliName)}
            </p>
            <CommandBox class="mt-2 text-[0.75rem]" command={requirement.command} {prompt} />
          </div>
        </NumberedStep>
      {:else if step === "install"}
        <NumberedStep {number} total={steps.length}>
          <div class="min-w-0 flex-1 pt-0.5 pb-4">
            <p class="text-[0.8rem] leading-5 text-foreground">{t("settings.agents.providers.install.runInstall")}</p>
            <CommandBox class="mt-2 text-[0.75rem]" command={guide.command} {prompt} />
          </div>
        </NumberedStep>
      {:else if step === "reopen" || step === "requirement-reopen"}
        <NumberedStep {number} total={steps.length}>
          <p class="min-w-0 flex-1 pt-0.5 pb-4 text-[0.8rem] leading-5 text-foreground">
            {powerShell
              ? t("settings.agents.providers.install.reopenPowerShell")
              : t("settings.agents.providers.install.reopenTerminal")}
          </p>
        </NumberedStep>
      {:else if step === "sign-in"}
        <NumberedStep {number} total={steps.length}>
          <div class="min-w-0 flex-1 pt-0.5 pb-4">
            <p class="text-[0.8rem] leading-5 text-foreground">
              {situation.kind === "installed"
                ? t("settings.agents.providers.install.runSignInIfNeeded")
                : t("settings.agents.providers.install.runSignIn")}
            </p>
            <CommandBox class="mt-2 text-[0.75rem]" command={needsInstall ? guide.signInAfterInstallCommand : guide.signInCommand} {prompt} />
          </div>
        </NumberedStep>
      {:else}
        <NumberedStep {number} total={steps.length} beside={onRefresh ? "field" : "text"}>
          <div class={onRefresh ? "min-w-0 flex-1" : "min-w-0 flex-1 pt-0.5"}>
            {#if !onRefresh}
              <p class="text-[0.8rem] leading-5 text-foreground">{t("settings.agents.providers.install.finishSetUp")}</p>
            {:else}
              <button
                type="button"
                class="inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-2.5 text-[0.733333rem] font-semibold whitespace-nowrap text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
                disabled={refreshing}
                onclick={() => void refresh()}
              >
                <RefreshCw size={13} class={refreshing ? "animate-spin" : undefined} aria-hidden="true" />
                {t("settings.agents.providers.refresh")}
              </button>
            {/if}
          </div>
        </NumberedStep>
      {/if}
    {/each}
  </ol>
  <p class="text-[0.8rem] leading-5 text-muted-foreground">
    {t("settings.agents.providers.install.guide")}
    <button
      type="button"
      class="inline-flex items-baseline gap-0.5 text-primary underline underline-offset-2 hover:text-primary/80"
      dir="ltr"
      onclick={() => void openGuide()}
    >
      {installDocsLabel(guide.docsUrl)}<ExternalLink size={12} class="self-center" aria-hidden="true" />
    </button>
  </p>
  {#if linkError}
    <Notice message={linkError} onDismiss={() => (guideLinkError = null)} />
  {/if}
</div>
