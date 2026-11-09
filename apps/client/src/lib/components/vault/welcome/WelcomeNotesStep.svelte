<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { NOTES_BEATS, NOTES_END_HOLD_MS, NOTES_STILL } from "$lib/vault/welcome-notes-scene";
  import WelcomeNotesScene, {
    NOTES_SCENE_MAX_HEIGHT,
    NOTES_SCENE_MAX_WIDTH,
    NOTES_SCENE_MIN_HEIGHT,
    NOTES_SCENE_MIN_WIDTH,
  } from "./WelcomeNotesScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on Notes: a page written with the slash menu, then its database gaining a row and shown as a board, on a loop. */
  const { t } = getLocalization();
</script>

<WelcomeSceneStep headline={t("vaultSetup.notes.headline")} body={t("vaultSetup.notes.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={NOTES_SCENE_MIN_WIDTH}
    maxWidth={NOTES_SCENE_MAX_WIDTH}
    minHeight={NOTES_SCENE_MIN_HEIGHT}
    maxHeight={NOTES_SCENE_MAX_HEIGHT}
    duration={NOTES_BEATS.end}
    hold={NOTES_END_HOLD_MS}
    still={NOTES_STILL}
    description={t("vaultSetup.notes.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeNotesScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
