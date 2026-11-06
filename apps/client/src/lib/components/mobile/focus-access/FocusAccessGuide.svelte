<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    FOCUS_ACCESS_GUIDES,
    type FocusAccessGuideCaption,
    type FocusAccessStep,
  } from "$lib/mobile/focus-access-steps";
  import { cn } from "$lib/utils";
  import AppBlockingScene from "./AppBlockingScene.svelte";
  import AutostartScene from "./AutostartScene.svelte";
  import BatteryScene from "./BatteryScene.svelte";
  import ExactAlarmScene from "./ExactAlarmScene.svelte";
  import NotificationsScene from "./NotificationsScene.svelte";
  import UsageAccessScene from "./UsageAccessScene.svelte";

  /**
   * A looping drawing of the Android screens a step opens. The drawing is decorative; screen readers get the
   * instructions it shows as a list instead.
   */
  let { step, appLabel }: { step: FocusAccessStep; appLabel: string } = $props();

  /** How long the drawing fades out before it starts over. */
  const RESTART_FADE_MS = 300;
  const { t } = getLocalization();
  const phases = untrack(() => FOCUS_ACCESS_GUIDES[step]);

  /** The drawn phone at zoom 1, matching its `h-72 w-52` screen plus the `p-1.5` bezel. */
  const PHONE_HEIGHT_PX = 300;
  const PHONE_WIDTH_PX = 220;
  /** How far the phone shrinks on short screens and grows into spare room before it stops looking like a guide. */
  const MIN_ZOOM = 0.7;
  const MAX_ZOOM = 1.35;

  let availableWidth = $state(0);
  let availableHeight = $state(0);
  const zoom = $derived(phoneZoom(availableWidth, availableHeight));

  let phase = $state(0);
  let cycle = $state(0);
  let restarting = $state(false);

  const instructions = $derived([...new Set(phases.map((entry) => captionText(entry.caption)))]);

  /** The phone's zoom that fills the room left between the text and the actions, within readable bounds. */
  function phoneZoom(width: number, height: number): number {
    if (width <= 0 || height <= 0) return 1;
    const fit = Math.min(width / PHONE_WIDTH_PX, height / PHONE_HEIGHT_PX);
    return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, fit));
  }

  function captionText(caption: FocusAccessGuideCaption): string {
    switch (caption) {
      case "allow": return t("mobile.focusOnboarding.captionAllow", appLabel);
      case "search": return t("mobile.focusOnboarding.captionSearch", appLabel);
      case "turn-on": return t("mobile.focusOnboarding.captionTurnOn");
      case "no-restrictions": return t("mobile.focusOnboarding.captionNoRestrictions");
      case "accept-warning": return t("mobile.focusOnboarding.captionAcceptWarning");
      case "open-downloaded": return t("mobile.focusOnboarding.captionOpenDownloaded");
      case "choose-service": return t("mobile.focusOnboarding.captionChooseService", t("mobile.focusOnboarding.guideServiceLabel"));
      case "tap-allow": return t("mobile.focusOnboarding.captionTapAllow");
    }
  }

  onMount(() => {
    let timer = 0;
    const hold = (): void => {
      timer = window.setTimeout(advance, phases[phase].durationMs);
    };
    const advance = (): void => {
      if (phase < phases.length - 1) {
        phase += 1;
        hold();
        return;
      }
      restarting = true;
      timer = window.setTimeout(() => {
        phase = 0;
        cycle += 1;
        restarting = false;
        hold();
      }, RESTART_FADE_MS);
    };
    hold();
    return () => window.clearTimeout(timer);
  });
</script>

<figure class="flex w-full flex-1 flex-col items-center">
  <!-- The phone sits out of the flow so its zoom never changes the room it is measured against. -->
  <div
    aria-hidden="true"
    class="relative min-h-52 w-full flex-1"
    bind:clientWidth={availableWidth}
    bind:clientHeight={availableHeight}
  >
    <div class="absolute inset-0 flex items-center justify-center">
      <div class="rounded-4xl bg-foreground/6 p-1.5" style:zoom={zoom}>
        <div
          class={cn(
            "relative h-72 w-52 overflow-hidden rounded-3xl bg-background transition-opacity duration-300 motion-reduce:transition-none",
            restarting && "opacity-0",
          )}
        >
          {#key cycle}
            <div class="absolute inset-0">
              {#if step === "notifications"}
                <NotificationsScene {phase} {appLabel} />
              {:else if step === "exact-alarm"}
                <ExactAlarmScene {phase} {appLabel} />
              {:else if step === "autostart"}
                <AutostartScene {phase} {appLabel} />
              {:else if step === "battery"}
                <BatteryScene {phase} {appLabel} />
              {:else if step === "usage-access"}
                <UsageAccessScene {phase} {appLabel} />
              {:else}
                <AppBlockingScene {phase} />
              {/if}
            </div>
          {/key}
        </div>
      </div>
    </div>
  </div>
  <figcaption class="sr-only">
    <ol>
      {#each instructions as instruction (instruction)}<li>{instruction}</li>{/each}
    </ol>
  </figcaption>
</figure>
