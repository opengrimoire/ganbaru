<script lang="ts">
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleHelp from "@lucide/svelte/icons/circle-question-mark";
  import Folder from "@lucide/svelte/icons/folder";
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
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
   * in full, with the development build's warning behind a help button at the end of the path, any error about the
   * folder, and the shell's other choices as quiet text actions, all above a still illustration of that folder and what
   * the app keeps in it. Continuing with the default is the shared
   * navigation's primary action, so the bottom bar matches every other welcome screen.
   */
  let {
    headline,
    location,
    locationLabel,
    developmentWarning = null,
    error = null,
    choices,
  }: {
    headline: string;
    /** The default location's full path. */
    location: string;
    locationLabel: string;
    developmentWarning?: string | null;
    /** Why the last attempt to open or create a vault failed. */
    error?: string | null;
    /** The shell's other choices, styled with `WELCOME_LINK_CLASS`. */
    choices: Snippet;
  } = $props();

  const { t } = getLocalization();
  /** The desktop shell shows hover tooltips; a phone has no hover, so its help button opens the text under the path. */
  const hoverTooltips = BUILD_PLATFORM_PROFILE.shell === "desktop";
  let warningOpen = $state(false);
</script>

<WelcomeSceneStep {headline}>
  {#snippet details()}
    <div class="flex flex-col gap-1 pt-1">
      <div class="flex min-w-0 items-start gap-2 text-sm leading-6 text-muted-foreground">
        <Folder size={16} strokeWidth={1.8} class="mt-1 shrink-0 text-foreground" aria-hidden="true" />
        <span class="flex min-w-0 flex-wrap gap-x-2">
          <span class="shrink-0 font-medium text-foreground">{locationLabel}</span>
          <span class="min-w-0 break-all">
            {location}
            {#if developmentWarning}
              <button
                type="button"
                class="ml-1 inline-grid size-5 place-items-center rounded-md align-[-0.3rem] text-muted-foreground transition-colors hover:text-foreground"
                data-app-tooltip={hoverTooltips ? developmentWarning : undefined}
                data-app-tooltip-keep-on-click="true"
                aria-label={developmentWarning}
                aria-expanded={hoverTooltips ? undefined : warningOpen}
                onclick={() => { if (!hoverTooltips) warningOpen = !warningOpen; }}
              >
                <CircleHelp size={14} strokeWidth={1.8} aria-hidden="true" />
              </button>
            {/if}
          </span>
        </span>
      </div>
      {#if developmentWarning && warningOpen}
        <p class="pl-6 text-sm leading-6 text-muted-foreground">{developmentWarning}</p>
      {/if}
      {#if error}
        <p role="alert" class="flex min-w-0 items-start gap-2 text-sm leading-6 text-foreground">
          <CircleAlert size={16} strokeWidth={1.8} class="mt-1 shrink-0 text-foreground" aria-hidden="true" />
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
    description={t("vaultSetup.data.illustrationDescription")}
  >
    {#snippet frame(_elapsed, width, height)}<WelcomeDataScene {width} {height} path={location} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
