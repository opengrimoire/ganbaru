<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { DISTRACTIONS_BEATS } from "$lib/vault/welcome-distractions-scene";
  import WelcomeDistractionsScene, {
    DISTRACTIONS_SCENE_MAX_HEIGHT,
    DISTRACTIONS_SCENE_MAX_WIDTH,
    DISTRACTIONS_SCENE_MIN_HEIGHT,
    DISTRACTIONS_SCENE_MIN_WIDTH,
  } from "./WelcomeDistractionsScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The distraction blocker's welcome screen: a site and an app blocked during Focus, then a daily limit across devices, on a loop. */
  const { t } = getLocalization();
</script>

<WelcomeSceneStep headline={t("vaultSetup.distractions.headline")} body={t("vaultSetup.distractions.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={DISTRACTIONS_SCENE_MIN_WIDTH}
    maxWidth={DISTRACTIONS_SCENE_MAX_WIDTH}
    minHeight={DISTRACTIONS_SCENE_MIN_HEIGHT}
    maxHeight={DISTRACTIONS_SCENE_MAX_HEIGHT}
    duration={DISTRACTIONS_BEATS.end}
    description={t("vaultSetup.distractions.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeDistractionsScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
