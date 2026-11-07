<script lang="ts">
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { ABOUT_BEATS } from "$lib/vault/welcome-about-scene";
  import AboutInformationDialog from "../AboutInformationDialog.svelte";
  import WelcomeAboutScene, { ABOUT_SCENE_MAX_HEIGHT, ABOUT_SCENE_MAX_WIDTH, ABOUT_SCENE_MIN_HEIGHT, ABOUT_SCENE_MIN_WIDTH } from "./WelcomeAboutScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /**
   * The welcome screen about the app itself: free and open source, no account, and data kept on the user's devices,
   * told by a scene of a payment terminal that confirms there is nothing to pay and prints a receipt that prices every
   * feature free and is stamped with the license, and with the license text, acknowledgments, and source code one tap away.
   */
  const { t } = getLocalization();
  const SOURCE_URL = "https://github.com/opengrimoire/ganbaru";
  const linkClass = "inline-flex min-h-10 items-center gap-1.5 text-sm font-medium text-foreground underline-offset-4 hover:underline";

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

<WelcomeSceneStep headline={t("vaultSetup.about.headline")} body={t("vaultSetup.about.body")}>
  {#snippet details()}
    <div class="-mb-2">
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
  {/snippet}

  <WelcomeScene
    class="flex-1"
    minWidth={ABOUT_SCENE_MIN_WIDTH}
    maxWidth={ABOUT_SCENE_MAX_WIDTH}
    minHeight={ABOUT_SCENE_MIN_HEIGHT}
    maxHeight={ABOUT_SCENE_MAX_HEIGHT}
    duration={ABOUT_BEATS.end}
    description={t("vaultSetup.about.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeAboutScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>

{#if informationDialog}
  <AboutInformationDialog kind={informationDialog} onClose={() => { informationDialog = null; }} />
{/if}
