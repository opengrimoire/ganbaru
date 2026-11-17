<script lang="ts">
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import ExternalLink from "@lucide/svelte/icons/external-link";
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
   * the provider's guide. Ganbaru shows the commands and never runs them.
   */
  let {
    family,
    guide,
    situation,
    finish,
  }: {
    family: ProviderFamilyMetadataRead;
    guide: ProviderInstallGuide;
    situation: ProviderInstallSituation;
    /** What the person chooses back in Ganbaru once the terminal steps are done. */
    finish: "set-up" | "refresh";
  } = $props();

  const { t } = getLocalization();
  let linkError = $state<string | null>(null);

  const cliName = $derived(family.defaultExecutableCandidates[0] || family.familyId);
  const prompt = $derived(guide.shell === "powershell" ? "PS>" : "$");
  const needsInstall = $derived(situationNeedsInstall(situation));
  const total = $derived(needsInstall ? 4 : 3);
  const statusMessage = $derived.by(() => {
    switch (situation.kind) {
      case "installed": return t("settings.agents.providers.install.installed", cliName, situation.path);
      case "not-installed": return t("settings.agents.providers.install.notInstalled", cliName);
      case "missing-at": return t("settings.agents.providers.install.missingAt", cliName, situation.path);
      case "signed-out": return t("settings.agents.providers.install.signedOut", family.displayName);
      case "unknown": return null;
    }
  });

  /** Opens the provider's guide, which validation already holds to `https`. */
  async function openGuide(): Promise<void> {
    linkError = null;
    try {
      await openUrl(guide.docsUrl);
    } catch (cause: unknown) {
      console.warn("the provider install guide could not be opened", cause);
      linkError = t("settings.agents.providers.install.guideFailed");
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
    <NumberedStep number={1} {total}>
      <p class="min-w-0 flex-1 pt-0.5 pb-4 text-[0.8rem] leading-5 text-foreground">
        {guide.shell === "powershell"
          ? t("settings.agents.providers.install.openPowerShell")
          : t("settings.agents.providers.install.openTerminal")}
      </p>
    </NumberedStep>
    {#if needsInstall}
      <NumberedStep number={2} {total}>
        <div class="min-w-0 flex-1 pt-0.5 pb-4">
          <p class="text-[0.8rem] leading-5 text-foreground">{t("settings.agents.providers.install.runInstall", cliName)}</p>
          <CommandBox class="mt-2 text-[0.75rem]" command={guide.command} {prompt} />
        </div>
      </NumberedStep>
    {/if}
    <NumberedStep number={needsInstall ? 3 : 2} {total}>
      <div class="min-w-0 flex-1 pt-0.5 pb-4">
        <p class="text-[0.8rem] leading-5 text-foreground">
          {needsInstall
            ? t("settings.agents.providers.install.runSignInAfterInstall")
            : situation.kind === "installed"
              ? t("settings.agents.providers.install.runSignInIfNeeded")
              : t("settings.agents.providers.install.runSignIn")}
        </p>
        <CommandBox class="mt-2 text-[0.75rem]" command={guide.signInCommand} {prompt} />
      </div>
    </NumberedStep>
    <NumberedStep number={total} {total}>
      <p class="min-w-0 flex-1 pt-0.5 text-[0.8rem] leading-5 text-foreground">
        {finish === "set-up"
          ? t("settings.agents.providers.install.finishSetUp", family.displayName)
          : t("settings.agents.providers.install.finishRefresh")}
      </p>
    </NumberedStep>
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
    <Notice message={linkError} onDismiss={() => (linkError = null)} />
  {/if}
</div>
