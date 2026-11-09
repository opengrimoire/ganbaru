<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { QUICK_NOTES_END_HOLD_MS, quickNotesSceneEnd, quickNotesSceneStill } from "$lib/vault/welcome-quick-notes-scene";
  import WelcomeQuickNotesScene, {
    QUICK_NOTES_SCENE_MAX_HEIGHT,
    QUICK_NOTES_SCENE_MAX_WIDTH,
    QUICK_NOTES_SCENE_MIN_HEIGHT,
    QUICK_NOTES_SCENE_MIN_WIDTH,
  } from "./WelcomeQuickNotesScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on Quick notes: notes tagged with a new tag, then filtered down to it, on a loop. */
  const { t } = getLocalization();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
</script>

<WelcomeSceneStep headline={t("vaultSetup.quickNotes.headline")} body={t("vaultSetup.quickNotes.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={QUICK_NOTES_SCENE_MIN_WIDTH}
    maxWidth={QUICK_NOTES_SCENE_MAX_WIDTH}
    minHeight={QUICK_NOTES_SCENE_MIN_HEIGHT}
    maxHeight={QUICK_NOTES_SCENE_MAX_HEIGHT}
    duration={quickNotesSceneEnd(mobileShell)}
    hold={QUICK_NOTES_END_HOLD_MS}
    still={quickNotesSceneStill(mobileShell)}
    description={t(mobileShell ? "vaultSetup.quickNotes.sceneDescriptionTouch" : "vaultSetup.quickNotes.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeQuickNotesScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
