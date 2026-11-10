<script lang="ts">
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Folder from "@lucide/svelte/icons/folder";
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import WelcomeDataScene, {
    DATA_SCENE_MAX_HEIGHT,
    DATA_SCENE_MAX_WIDTH,
    DATA_SCENE_MIN_HEIGHT,
    DATA_SCENE_MIN_WIDTH,
  } from "./WelcomeDataScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /**
   * The last welcome screen, where the person chooses where their data lives. Under the title sit the default location
   * in full, any error about the folder, and the shell's other choices as quiet text actions, all above a still
   * illustration of that folder and what the app keeps in it. Continuing with the default is the shared navigation's
   * primary action, so the bottom bar matches every other welcome screen.
   */
  let {
    headline,
    location,
    locationLabel,
    showIcons = true,
    error = null,
    choices,
  }: {
    headline: string;
    /** The default location's full path. */
    location: string;
    locationLabel: string;
    /** Whether the location and error lines lead with their icons. */
    showIcons?: boolean;
    /** Why the last attempt to open or create a vault failed. */
    error?: string | null;
    /** The shell's other choices, styled with `WELCOME_LINK_CLASS`. */
    choices: Snippet;
  } = $props();

  const { t } = getLocalization();
</script>

<WelcomeSceneStep {headline}>
  {#snippet details()}
    <div class="flex flex-col gap-1 pt-1">
      <div class="flex min-w-0 items-start gap-2 text-sm leading-6 text-muted-foreground">
        {#if showIcons}
          <Folder size={16} strokeWidth={1.8} class="mt-1 shrink-0 text-foreground" aria-hidden="true" />
        {/if}
        <span class="flex min-w-0 flex-wrap gap-x-2">
          <span class="shrink-0 font-medium text-foreground">{locationLabel}</span>
          <span class="min-w-0 break-all">{location}</span>
        </span>
      </div>
      {#if error}
        <p role="alert" class="flex min-w-0 items-start gap-2 text-sm leading-6 text-foreground">
          {#if showIcons}
            <CircleAlert size={16} strokeWidth={1.8} class="mt-1 shrink-0 text-foreground" aria-hidden="true" />
          {/if}
          <span class="min-w-0 wrap-anywhere">{error}</span>
        </p>
      {/if}
      <div class="mt-1 flex flex-wrap gap-x-5">{@render choices()}</div>
    </div>
  {/snippet}

  <WelcomeScene
    class="flex-1"
    minWidth={DATA_SCENE_MIN_WIDTH}
    maxWidth={DATA_SCENE_MAX_WIDTH}
    minHeight={DATA_SCENE_MIN_HEIGHT}
    maxHeight={DATA_SCENE_MAX_HEIGHT}
    ownZoom
    description={t("vaultSetup.data.illustrationDescription")}
  >
    {#snippet frame(_elapsed, width, height)}<WelcomeDataScene {width} {height} path={location} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
