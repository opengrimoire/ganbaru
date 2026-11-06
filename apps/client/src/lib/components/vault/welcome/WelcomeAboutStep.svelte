<script lang="ts">
  import CodeXml from "@lucide/svelte/icons/code-xml";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import UserRoundX from "@lucide/svelte/icons/user-round-x";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Component, Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import AboutInformationDialog from "../AboutInformationDialog.svelte";

  /**
   * The welcome screen about the app itself: free and open source, no account, and data kept on the user's devices,
   * with the license, acknowledgments, and source code one tap away.
   */
  let { actions }: { actions: Snippet } = $props();

  const { t } = getLocalization();
  const SOURCE_URL = "https://github.com/opengrimoire/ganbaru";
  const linkClass = "inline-flex min-h-10 items-center gap-1.5 text-sm font-medium text-foreground underline-offset-4 hover:underline";

  type CredentialIcon = Component<{ size?: number; strokeWidth?: number; "aria-hidden"?: boolean | "true" | "false" }>;

  let informationDialog = $state<"license" | "acknowledgments" | null>(null);
  let sourceError = $state(false);

  const credentials = $derived<readonly { icon: CredentialIcon; title: string; body: string }[]>([
    { icon: CodeXml, title: t("vaultSetup.welcomeFreeOpenSource"), body: t("vaultSetup.about.freeOpenSourceBody") },
    { icon: UserRoundX, title: t("vaultSetup.welcomeNoAccount"), body: t("vaultSetup.about.noAccountBody") },
    { icon: LockKeyhole, title: t("vaultSetup.welcomeLocalData"), body: t("vaultSetup.about.localDataBody") },
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
  <div class="mx-auto flex min-h-full w-full max-w-4xl flex-col px-4 pb-6 pt-8 @min-[560px]:px-8 @min-[860px]:px-12 @min-[860px]:py-10">

    <div class="flex flex-1 flex-col justify-center gap-7 py-6 @min-[860px]:gap-10">
      <div class="max-w-2xl space-y-2 @min-[860px]:space-y-4">
        <h1 class="text-2xl font-semibold leading-tight tracking-tight text-balance text-foreground @min-[560px]:text-3xl @min-[860px]:text-4xl">
          {t("vaultSetup.about.headline")}
        </h1>
        <p class="text-sm leading-6 text-pretty text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">
          {t("vaultSetup.about.body")}
        </p>
      </div>

      <ul class="grid gap-5 @min-[680px]:grid-cols-3 @min-[680px]:gap-8">
        {#each credentials as credential (credential.title)}
          <li class="space-y-1.5">
            <p class="inline-flex items-center gap-2 text-sm font-semibold text-foreground @min-[560px]:text-base">
              <credential.icon size={17} strokeWidth={1.8} aria-hidden="true" />
              {credential.title}
            </p>
            <p class="text-sm leading-6 text-pretty text-muted-foreground">{credential.body}</p>
          </li>
        {/each}
      </ul>

      <div class="space-y-1">
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
      </div>
    </div>

    <div class="shrink-0 border-t border-border pt-4">{@render actions()}</div>
  </div>
</section>

{#if informationDialog}
  <AboutInformationDialog kind={informationDialog} onClose={() => { informationDialog = null; }} />
{/if}
