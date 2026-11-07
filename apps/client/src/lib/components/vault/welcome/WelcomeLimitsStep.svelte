<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { LIMITS_BEATS } from "$lib/vault/welcome-limits-scene";
  import {
    BLOCKER_SCENE_MAX_HEIGHT,
    BLOCKER_SCENE_MAX_WIDTH,
    BLOCKER_SCENE_MIN_HEIGHT,
    BLOCKER_SCENE_MIN_WIDTH,
  } from "./WelcomeBlockerStage.svelte";
  import WelcomeLimitsScene from "./WelcomeLimitsScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on daily limits: one budget for a site and its app filled from a laptop and a phone until both are blocked, on a loop. */
  const { t } = getLocalization();
</script>

<WelcomeSceneStep headline={t("vaultSetup.limits.headline")} body={t("vaultSetup.limits.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={BLOCKER_SCENE_MIN_WIDTH}
    maxWidth={BLOCKER_SCENE_MAX_WIDTH}
    minHeight={BLOCKER_SCENE_MIN_HEIGHT}
    maxHeight={BLOCKER_SCENE_MAX_HEIGHT}
    duration={LIMITS_BEATS.end}
    description={t("vaultSetup.limits.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeLimitsScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
