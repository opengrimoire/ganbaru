<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { NOTES_END_HOLD_MS, notesSceneEnd, notesSceneStill } from "$lib/vault/welcome-notes-scene";
  import WelcomeNotesScene, {
    NOTES_SCENE_MAX_HEIGHT,
    NOTES_SCENE_MAX_WIDTH,
    NOTES_SCENE_MIN_HEIGHT,
    NOTES_SCENE_MIN_WIDTH,
  } from "./WelcomeNotesScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on Notes: a page's database gaining a row in its table, then shown as a board, on a loop. */
  const { t } = getLocalization();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
</script>

<WelcomeSceneStep headline={t("vaultSetup.notes.headline")} body={t("vaultSetup.notes.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={NOTES_SCENE_MIN_WIDTH}
    maxWidth={NOTES_SCENE_MAX_WIDTH}
    minHeight={NOTES_SCENE_MIN_HEIGHT}
    maxHeight={NOTES_SCENE_MAX_HEIGHT}
    duration={notesSceneEnd(mobileShell)}
    hold={NOTES_END_HOLD_MS}
    still={notesSceneStill(mobileShell)}
    description={t(mobileShell ? "vaultSetup.notes.sceneDescriptionTouch" : "vaultSetup.notes.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeNotesScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
