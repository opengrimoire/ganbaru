<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import { untrack } from "svelte";
  import { fly } from "svelte/transition";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { prefersReducedMotion } from "$lib/utils/smooth-swap";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import { WELCOME_STEPS, welcomeStepAt } from "$lib/vault/welcome-steps";
  import WelcomeAboutStep from "./welcome/WelcomeAboutStep.svelte";
  import WelcomeBlockingStep from "./welcome/WelcomeBlockingStep.svelte";
  import WelcomeCalendarStep from "./welcome/WelcomeCalendarStep.svelte";
  import WelcomeLanguageStep from "./welcome/WelcomeLanguageStep.svelte";
  import WelcomeLimitsStep from "./welcome/WelcomeLimitsStep.svelte";
  import WelcomeMusicStep from "./welcome/WelcomeMusicStep.svelte";

  /**
   * The welcome screens before vault setup: the language, one per feature, then the app itself. Every screen shares
   * one navigation bar that stays in place at the bottom while the screens slide in above it, so moving between them
   * never shifts the buttons. Continue on the last screen hands over to vault setup. `step` is bindable so a shell
   * can show the progress in its own chrome; with `showStepDots`, the navigation shows it instead.
   */
  let {
    step = $bindable(0),
    showStepDots = true,
    onContinue,
  }: { step?: number; showStepDots?: boolean; onContinue: () => void } = $props();

  /** How far a screen slides in from, matching the Android access screens. */
  const ENTER_OFFSET_PX = 24;
  const enterMs = prefersReducedMotion() ? 0 : 260;
  const { t } = getLocalization();
  const current = $derived(welcomeStepAt(step));
  const index = $derived(WELCOME_STEPS.indexOf(current));

  /** The screen arrives from the side it is on: the right when moving forward, the left when going back. */
  let shownIndex = untrack(() => index);
  let enterFrom = $state(1);
  $effect.pre(() => {
    const next = index;
    if (next === shownIndex) return;
    enterFrom = next > shownIndex ? 1 : -1;
    shownIndex = next;
  });

  function next(): void {
    if (index < WELCOME_STEPS.length - 1) step = index + 1;
    else onContinue();
  }

  function back(): void {
    if (index > 0) step = index - 1;
  }
</script>

<div class="@container flex h-full flex-col">
  <div class="relative min-h-0 flex-1 overflow-hidden">
    {#key current}
      <div class="absolute inset-0" in:fly={{ x: ENTER_OFFSET_PX * enterFrom, duration: enterMs }}>
        {#if current === "language"}
          <WelcomeLanguageStep />
        {:else if current === "calendar"}
          <WelcomeCalendarStep />
        {:else if current === "blocking"}
          <WelcomeBlockingStep />
        {:else if current === "limits"}
          <WelcomeLimitsStep />
        {:else if current === "music"}
          <WelcomeMusicStep />
        {:else}
          <WelcomeAboutStep />
        {/if}
      </div>
    {/key}
  </div>

  <div class="mx-auto w-full max-w-180 shrink-0 px-4 pb-6 @min-[560px]:px-8 @min-[760px]:px-10 @min-[860px]:pb-10">
    <div class="flex items-center gap-2 border-t border-border pt-4">
      {#if index > 0}
        <button
          type="button"
          class="inline-flex min-h-11 shrink-0 items-center justify-center gap-2 rounded-md border border-border bg-background px-4 py-2.5 text-sm font-medium text-foreground transition-colors hover:bg-accent"
          onclick={back}
        >
          <ArrowLeft size={16} strokeWidth={1.8} aria-hidden="true" />
          {t("common.back")}
        </button>
      {/if}
      <button
        use:onboardingPrimaryAction
        type="button"
        class="inline-flex min-h-11 flex-1 items-center justify-center gap-2 rounded-md bg-primary px-5 py-2.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 @min-[560px]:flex-none"
        onclick={next}
      >
        {t("vaultSetup.continue")}
        <ArrowRight size={16} strokeWidth={1.8} aria-hidden="true" />
      </button>
      {#if showStepDots}
        <span class="ml-auto flex shrink-0 gap-1.5 pl-3" aria-hidden="true">
          {#each WELCOME_STEPS as dot, dotIndex (dot)}
            <span class={cn("h-1 w-4 rounded-full transition-colors duration-300", dotIndex <= index ? "bg-primary" : "bg-foreground/15")}></span>
          {/each}
        </span>
        <span class="sr-only">{t("vaultSetup.stepProgress", index + 1, WELCOME_STEPS.length)}</span>
      {/if}
    </div>
  </div>
</div>
