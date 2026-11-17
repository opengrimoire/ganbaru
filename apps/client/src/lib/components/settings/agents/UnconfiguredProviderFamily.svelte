<script lang="ts">
  import type { ProviderFamilyMetadataRead } from "$lib/chat/contracts";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ProviderInstallSteps from "./ProviderInstallSteps.svelte";
  import { unconfiguredFamilySituation } from "./provider-install-steps";

  /**
   * A provider family this device has no configuration for: whether its CLI is installed, the
   * provider's steps to install it and sign in, and the action that sets it up in Ganbaru.
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
</script>

<section class="flex flex-col items-start gap-3 px-1 text-[0.8rem] text-muted-foreground">
  <p>{t("settings.agents.providers.noConfigurations", family.displayName)}</p>
  {#if family.installGuide}
    <ProviderInstallSteps
      {family}
      guide={family.installGuide}
      situation={unconfiguredFamilySituation(installedExecutables)}
      finish="set-up"
    />
  {/if}
  <button
    type="button"
    class="text-[0.8rem] font-semibold text-foreground underline-offset-2 hover:underline"
    onclick={onSetUp}
  >{t("settings.agents.providers.setUp", family.displayName)}</button>
</section>
