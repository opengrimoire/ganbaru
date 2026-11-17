<script lang="ts">
  import type { ProviderFamilyMetadataRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import ProviderInstallSteps from "./ProviderInstallSteps.svelte";
  import { unconfiguredFamilySituation } from "./provider-install-steps";

  /**
   * A provider family this device has no configuration for: whether its CLI is installed, the
   * provider's steps to install it and sign in, and the actions that set it up in Ganbaru. Where
   * automatic setup applies, the last step refreshes providers, which adds the CLI once it is
   * found, and the manual setup stays available as the advanced path.
   */
  let {
    family,
    installedExecutables,
    onSetUp,
  }: {
    /** The family, with its display name already localized. */
    family: ProviderFamilyMetadataRead;
    /** The programs the settings read found for the family, or `undefined` when the device does not search for it. */
    installedExecutables: readonly string[] | undefined;
    onSetUp: () => void;
  } = $props();

  const { t } = getLocalization();
  const chat = getChat();
  let refreshError = $state<string | null>(null);
  let notAdded = $state(false);

  const situation = $derived(unconfiguredFamilySituation(installedExecutables));
  /** Refresh adds a found CLI unless the device does not search for it or the person removed it before. */
  const automatic = $derived(situation.kind !== "unknown"
    && !(chat.settings?.configuration.automaticProviderSetupDisabled.includes(family.familyId) ?? false));

  async function refresh(): Promise<void> {
    refreshError = null;
    notAdded = false;
    try {
      await chat.refreshAllProviders();
    } catch (error: unknown) {
      refreshError = chatErrorMessage(error, t("settings.agents.providers.operationFailed"));
      return;
    }
    notAdded = !(chat.settings?.providerInstances.some((provider) => provider.configuration.familyId === family.familyId) ?? false);
  }
</script>

<section class="flex flex-col items-start gap-3 px-1 text-[0.8rem] text-muted-foreground">
  <p>{t("settings.agents.providers.noConfigurations", family.displayName)}</p>
  {#if family.installGuide}
    <ProviderInstallSteps
      {family}
      guide={family.installGuide}
      {situation}
      onRefresh={automatic ? refresh : undefined}
    />
  {/if}
  {#if refreshError}
    <Notice class="text-[0.8rem]" message={refreshError} onDismiss={() => (refreshError = null)} />
  {:else if notAdded}
    <Notice
      class="text-[0.8rem]"
      tone="warning"
      role="status"
      copy={false}
      message={t("settings.agents.providers.install.notAdded", family.displayName)}
    />
  {/if}
  <button
    type="button"
    class="text-[0.8rem] font-semibold text-foreground underline-offset-2 hover:underline"
    onclick={onSetUp}
  >{automatic ? t("settings.agents.providers.setUpAdvanced", family.displayName) : t("settings.agents.providers.setUp", family.displayName)}</button>
</section>
