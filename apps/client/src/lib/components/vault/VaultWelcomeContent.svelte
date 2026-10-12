<script lang="ts">
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import VaultLanguageDropdown from "./VaultLanguageDropdown.svelte";
  import AboutInformationDialog from "./AboutInformationDialog.svelte";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";

  let { onContinue }: { onContinue: () => void } = $props();

  const { t } = getLocalization();
  const SOURCE_URL = "https://github.com/opengrimoire/ganbaru-ai";
  let informationDialog = $state<"license" | "acknowledgments" | null>(null);
  let sourceError = $state(false);

  async function openSource(): Promise<void> {
    sourceError = false;
    try {
      await openUrl(SOURCE_URL);
    } catch (error: unknown) {
      console.warn("Failed to open source repository:", error);
      sourceError = true;
    }
  }
</script>

<section class="h-full overflow-y-auto px-5 min-[560px]:px-8">
  <div class="mx-auto grid min-h-full w-full max-w-xl grid-rows-[minmax(2rem,1fr)_auto_minmax(2rem,1fr)] py-6 min-[760px]:py-8">
    <div class="flex items-end pb-6"><VaultLanguageDropdown /></div>

    <div class="space-y-8">
      <div class="space-y-4">
        <h1 class="text-3xl font-semibold leading-tight tracking-tight text-foreground min-[560px]:text-4xl">
          {t("vaultSetup.welcomeTitle")}
        </h1>
        <p class="text-base leading-7 text-muted-foreground">
          {t("settings.about.summaryDetails")}
        </p>
      </div>

      <div class="flex flex-wrap gap-x-5 gap-y-3 border-t border-border pt-5">
        <button type="button" class="inline-flex min-h-10 items-center gap-2 text-sm font-medium text-foreground underline-offset-4 hover:underline" onclick={() => { informationDialog = "license"; }}>
          {t("vaultSetup.licenseButton")}
        </button>
        <button type="button" class="inline-flex min-h-10 items-center gap-2 text-sm font-medium text-foreground underline-offset-4 hover:underline" onclick={() => { informationDialog = "acknowledgments"; }}>
          {t("settings.about.acknowledgmentsHeading")}
        </button>
        <button type="button" class="inline-flex min-h-10 items-center gap-2 text-sm font-medium text-foreground underline-offset-4 hover:underline" onclick={() => void openSource()}>
          {t("vaultSetup.sourceCode")}
          <ExternalLink size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
      </div>
      {#if sourceError}<p role="alert" class="text-sm text-destructive">{t("vaultSetup.sourceOpenError")}</p>{/if}

      <button use:onboardingPrimaryAction type="button" class="inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-5 py-2.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 min-[560px]:w-auto" onclick={onContinue}>
        {t("vaultSetup.continue")}
        <ArrowRight size={16} strokeWidth={1.8} aria-hidden="true" />
      </button>
    </div>
    <div></div>
  </div>
</section>

{#if informationDialog}
  <AboutInformationDialog kind={informationDialog} onClose={() => { informationDialog = null; }} />
{/if}
