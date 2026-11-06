<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { FILM_BEATS } from "$lib/vault/welcome-film";
  import WelcomeFilmScene, { FILM_SCENE_MAX_HEIGHT, FILM_SCENE_MIN_HEIGHT, FILM_SCENE_WIDTH } from "./WelcomeFilmScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The Calendar's welcome screen: a Focus session planned in the Calendar and run from it, on a loop. */
  const { t } = getLocalization();
</script>

<WelcomeSceneStep headline={t("vaultSetup.welcomeHeadline")} body={t("vaultSetup.welcomeBody")}>
  <WelcomeScene
    class="flex-1"
    width={FILM_SCENE_WIDTH}
    minHeight={FILM_SCENE_MIN_HEIGHT}
    maxHeight={FILM_SCENE_MAX_HEIGHT}
    duration={FILM_BEATS.end}
    description={t("vaultSetup.film.description")}
  >
    {#snippet frame(elapsed, height)}<WelcomeFilmScene {elapsed} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
