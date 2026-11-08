<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { TASKS_BEATS, TASKS_END_HOLD_MS, TASKS_STILL } from "$lib/vault/welcome-tasks-scene";
  import WelcomeProjectsScene, {
    PROJECTS_SCENE_MAX_HEIGHT,
    PROJECTS_SCENE_MAX_WIDTH,
    PROJECTS_SCENE_MIN_HEIGHT,
    PROJECTS_SCENE_MIN_WIDTH,
  } from "./WelcomeProjectsScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on tasks: adding a task in the Novel project's List view and changing its status and priority from their cells, on a loop. */
  const { t } = getLocalization();
</script>

<WelcomeSceneStep headline={t("vaultSetup.tasks.headline")} body={t("vaultSetup.tasks.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={PROJECTS_SCENE_MIN_WIDTH}
    maxWidth={PROJECTS_SCENE_MAX_WIDTH}
    minHeight={PROJECTS_SCENE_MIN_HEIGHT}
    maxHeight={PROJECTS_SCENE_MAX_HEIGHT}
    duration={TASKS_BEATS.end}
    hold={TASKS_END_HOLD_MS}
    still={TASKS_STILL}
    description={t("vaultSetup.tasks.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeProjectsScene part="tasks" {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
