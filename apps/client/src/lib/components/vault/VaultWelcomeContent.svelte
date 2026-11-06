<script lang="ts">
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import CodeXml from "@lucide/svelte/icons/code-xml";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import UserRoundX from "@lucide/svelte/icons/user-round-x";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Component } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import AboutInformationDialog from "./AboutInformationDialog.svelte";
  import VaultLanguageDropdown from "./VaultLanguageDropdown.svelte";
  import WelcomeFilm from "./welcome/WelcomeFilm.svelte";

  /**
   * The first onboarding screen: a short film of a day in the app next to the pitch, the credentials that set the app
   * apart, and links to the license, acknowledgments, and source. Wide containers place the film beside the text;
   * narrow ones stack it above, and it steps aside when there is no room for it. Continue works at any moment.
   */
  let { onContinue }: { onContinue: () => void } = $props();

  const { t } = getLocalization();
  const SOURCE_URL = "https://github.com/opengrimoire/ganbaru";
  const linkClass = "inline-flex min-h-10 items-center gap-1.5 text-sm font-medium text-foreground underline-offset-4 hover:underline";

  let informationDialog = $state<"license" | "acknowledgments" | null>(null);
  let sourceError = $state(false);

  const credentials = $derived<readonly { icon: Component<{ size?: number; strokeWidth?: number; "aria-hidden"?: boolean | "true" | "false" }>; label: string }[]>([
    { icon: CodeXml, label: t("vaultSetup.welcomeFreeOpenSource") },
    { icon: UserRoundX, label: t("vaultSetup.welcomeNoAccount") },
    { icon: LockKeyhole, label: t("vaultSetup.welcomeLocalData") },
  ]);

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

<section class="@container h-full overflow-x-hidden overflow-y-auto">
  <div
    class="mx-auto flex min-h-full w-full max-w-300 flex-col px-4 pb-6 pt-8 @min-[560px]:px-8 @min-[860px]:grid @min-[860px]:grid-cols-[minmax(18rem,26rem)_minmax(0,1fr)] @min-[860px]:grid-rows-[auto_minmax(0,1fr)] @min-[860px]:gap-x-10 @min-[860px]:px-12 @min-[860px]:py-10"
  >
    <div class="flex shrink-0 items-start"><VaultLanguageDropdown /></div>

    <WelcomeFilm class="min-h-0 flex-1 py-3 @min-[860px]:col-start-2 @min-[860px]:row-span-2 @min-[860px]:row-start-1 @min-[860px]:py-0" />

    <div class="shrink-0 space-y-4 @min-[860px]:col-start-1 @min-[860px]:row-start-2 @min-[860px]:self-center @min-[860px]:space-y-6">
      <div class="space-y-2 @min-[860px]:space-y-4">
        <h1 class="text-2xl font-semibold leading-tight tracking-tight text-balance text-foreground @min-[560px]:text-3xl @min-[860px]:text-4xl">
          {t("vaultSetup.welcomeHeadline")}
        </h1>
        <p class="text-sm leading-6 text-pretty text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">
          {t("vaultSetup.welcomeBody")}
        </p>
      </div>

      <ul class="flex flex-wrap gap-x-4 gap-y-1.5 text-xs font-medium text-foreground @min-[560px]:text-sm">
        {#each credentials as credential (credential.label)}
          <li class="inline-flex items-center gap-1.5">
            <credential.icon size={15} strokeWidth={1.8} aria-hidden="true" />
            {credential.label}
          </li>
        {/each}
      </ul>

      <div class="space-y-3 border-t border-border pt-3 @min-[860px]:pt-4">
        <div class="flex flex-wrap gap-x-5">
          <button type="button" class={linkClass} onclick={() => { informationDialog = "license"; }}>
            {t("vaultSetup.licenseButton")}
          </button>
          <button type="button" class={linkClass} onclick={() => { informationDialog = "acknowledgments"; }}>
            {t("settings.about.acknowledgmentsHeading")}
          </button>
          <button type="button" class={linkClass} onclick={() => void openSource()}>
            {t("vaultSetup.sourceCode")}
            <ExternalLink size={14} strokeWidth={1.8} aria-hidden="true" />
          </button>
        </div>
        {#if sourceError}<p role="alert" class="text-sm text-destructive">{t("vaultSetup.sourceOpenError")}</p>{/if}

        <button
          use:onboardingPrimaryAction
          type="button"
          class="inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-5 py-2.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 @min-[560px]:w-auto"
          onclick={onContinue}
        >
          {t("vaultSetup.continue")}
          <ArrowRight size={16} strokeWidth={1.8} aria-hidden="true" />
        </button>
      </div>
    </div>
  </div>
</section>

{#if informationDialog}
  <AboutInformationDialog kind={informationDialog} onClose={() => { informationDialog = null; }} />
{/if}
