<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { MUSIC_BEATS, MUSIC_END_HOLD_MS, MUSIC_STILL } from "$lib/vault/welcome-music-scene";
  import WelcomeMusicScene, { MUSIC_SCENE_MAX_HEIGHT, MUSIC_SCENE_MAX_WIDTH, MUSIC_SCENE_MIN_HEIGHT, MUSIC_SCENE_MIN_WIDTH } from "./WelcomeMusicScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on Music: local files and YouTube in playlists, which a project's events play for work and for breaks, on a loop. */
  const { t } = getLocalization();
</script>

<WelcomeSceneStep headline={t("vaultSetup.music.headline")} body={t("vaultSetup.music.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={MUSIC_SCENE_MIN_WIDTH}
    maxWidth={MUSIC_SCENE_MAX_WIDTH}
    minHeight={MUSIC_SCENE_MIN_HEIGHT}
    maxHeight={MUSIC_SCENE_MAX_HEIGHT}
    duration={MUSIC_BEATS.end}
    hold={MUSIC_END_HOLD_MS}
    still={MUSIC_STILL}
    description={t("vaultSetup.music.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeMusicScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
