<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { PROJECTS_BEATS, PROJECTS_END_HOLD_MS, PROJECTS_STILL } from "$lib/vault/welcome-projects-scene";
  import WelcomeProjectsScene, {
    PROJECTS_SCENE_MAX_HEIGHT,
    PROJECTS_SCENE_MAX_WIDTH,
    PROJECTS_SCENE_MIN_HEIGHT,
    PROJECTS_SCENE_MIN_WIDTH,
  } from "./WelcomeProjectsScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on project defaults: setting the Novel project's event defaults in its settings, then a Calendar event that takes them once the project is picked, on a loop. */
  const { t } = getLocalization();
</script>

<WelcomeSceneStep headline={t("vaultSetup.projects.headline")} body={t("vaultSetup.projects.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={PROJECTS_SCENE_MIN_WIDTH}
    maxWidth={PROJECTS_SCENE_MAX_WIDTH}
    minHeight={PROJECTS_SCENE_MIN_HEIGHT}
    maxHeight={PROJECTS_SCENE_MAX_HEIGHT}
    duration={PROJECTS_BEATS.end}
    hold={PROJECTS_END_HOLD_MS}
    still={PROJECTS_STILL}
    description={t("vaultSetup.projects.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeProjectsScene part="defaults" {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
