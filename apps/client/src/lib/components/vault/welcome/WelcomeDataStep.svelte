<script lang="ts">
  import Folder from "@lucide/svelte/icons/folder";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { DATA_BEATS } from "$lib/vault/welcome-data-scene";
  import WelcomeDataScene, {
    DATA_SCENE_MAX_HEIGHT,
    DATA_SCENE_MAX_WIDTH,
    DATA_SCENE_MIN_HEIGHT,
    DATA_SCENE_MIN_WIDTH,
  } from "./WelcomeDataScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /**
   * The last welcome screen, where the person chooses where their data lives: the default location in full under the
   * text, and a scene of that folder filling with what the app keeps in it. The choices themselves live in the shared
   * navigation, so the default is the primary action there.
   */
  let {
    headline,
    body,
    location,
    locationLabel,
    developmentWarning = null,
  }: {
    headline: string;
    body: string;
    /** The default location's full path. */
    location: string;
    locationLabel: string;
    developmentWarning?: string | null;
  } = $props();

  const { t } = getLocalization();
</script>

<WelcomeSceneStep {headline} {body}>
  {#snippet details()}
    <div class="flex flex-col gap-1.5 pt-1">
      <p class="flex min-w-0 items-start gap-2 text-sm leading-6 text-muted-foreground">
        <Folder size={16} strokeWidth={1.8} class="mt-1 shrink-0 text-foreground" aria-hidden="true" />
        <span class="flex min-w-0 flex-wrap gap-x-2">
          <span class="shrink-0 font-medium text-foreground">{locationLabel}</span>
          <span class="min-w-0 break-all">{location}</span>
        </span>
      </p>
      {#if developmentWarning}
        <p class="text-sm leading-6 font-semibold text-warning">{developmentWarning}</p>
      {/if}
    </div>
  {/snippet}

  <WelcomeScene
    class="flex-1"
    minWidth={DATA_SCENE_MIN_WIDTH}
    maxWidth={DATA_SCENE_MAX_WIDTH}
    minHeight={DATA_SCENE_MIN_HEIGHT}
    maxHeight={DATA_SCENE_MAX_HEIGHT}
    duration={DATA_BEATS.end}
    loop={false}
    description={t("vaultSetup.data.sceneDescription")}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeDataScene {elapsed} {width} {height} path={location} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
