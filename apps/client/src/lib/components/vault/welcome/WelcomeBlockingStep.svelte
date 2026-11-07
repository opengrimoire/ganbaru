<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BLOCKING_BEATS, BLOCKING_STILL } from "$lib/vault/welcome-blocking-scene";
  import {
    BLOCKER_SCENE_MAX_HEIGHT,
    BLOCKER_SCENE_MAX_WIDTH,
    BLOCKER_SCENE_MIN_HEIGHT,
    BLOCKER_SCENE_MIN_WIDTH,
  } from "./WelcomeBlockerStage.svelte";
  import WelcomeBlockingScene from "./WelcomeBlockingScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on blocking during Focus: a site and an app blocked while a Calendar event runs Focus, and open again after it, on a loop. */
  const { t } = getLocalization();
</script>

<WelcomeSceneStep headline={t("vaultSetup.blocking.headline")} body={t("vaultSetup.blocking.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={BLOCKER_SCENE_MIN_WIDTH}
    maxWidth={BLOCKER_SCENE_MAX_WIDTH}
    minHeight={BLOCKER_SCENE_MIN_HEIGHT}
    maxHeight={BLOCKER_SCENE_MAX_HEIGHT}
    duration={BLOCKING_BEATS.end}
    still={BLOCKING_STILL}
    description={t("vaultSetup.blocking.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeBlockingScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
