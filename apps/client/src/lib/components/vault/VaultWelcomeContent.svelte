<script lang="ts">
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { clampWelcomeStep, welcomeProgress, welcomeStageSteps, type WelcomeStage } from "$lib/vault/welcome-steps";
  import OnboardingNav, { type OnboardingNavPrimary } from "./OnboardingNav.svelte";
  import WelcomeAboutStep from "./welcome/WelcomeAboutStep.svelte";
  import WelcomeBlockingStep from "./welcome/WelcomeBlockingStep.svelte";
  import WelcomeCalendarStep from "./welcome/WelcomeCalendarStep.svelte";
  import WelcomeChatStep from "./welcome/WelcomeChatStep.svelte";
  import WelcomeLanguageStep from "./welcome/WelcomeLanguageStep.svelte";
  import WelcomeLimitsStep from "./welcome/WelcomeLimitsStep.svelte";
  import WelcomeMusicStep from "./welcome/WelcomeMusicStep.svelte";
  import WelcomeNotesStep from "./welcome/WelcomeNotesStep.svelte";
  import WelcomeProjectsStep from "./welcome/WelcomeProjectsStep.svelte";
  import WelcomeQuickNotesStep from "./welcome/WelcomeQuickNotesStep.svelte";
  import WelcomeTasksStep from "./welcome/WelcomeTasksStep.svelte";

  /**
   * The welcome screens of one `stage`: the language and the data screen the shell draws in `data` before the vault
   * exists, or the feature tour once it is open. Every screen shares one navigation bar that stays in place at the
   * bottom while the screens change above it at once, so moving between them never shifts the buttons. The last
   * screen's primary action is `last`, such as the default location on the data screen; the shell's other data choices
   * sit in the data screen itself. `step` is bindable so a shell can start on a later screen or show the progress in
   * its own chrome; with `showStepDots`, the navigation shows it across both stages instead.
   */
  let {
    stage,
    step = $bindable(0),
    showStepDots = true,
    data,
    last,
    lastBackDisabled = false,
  }: {
    stage: WelcomeStage;
    step?: number;
    showStepDots?: boolean;
    /** The data screen, which the setup stage needs. */
    data?: Snippet;
    last: OnboardingNavPrimary;
    /** Keeps Back off the last screen while its action is running. */
    lastBackDisabled?: boolean;
  } = $props();

  const { t } = getLocalization();
  const steps = $derived(welcomeStageSteps(stage));
  const index = $derived(clampWelcomeStep(stage, step));
  const current = $derived(steps[index]);
  const onLast = $derived(index === steps.length - 1);
  const progress = $derived(showStepDots ? welcomeProgress(stage, index) : undefined);

  function next(): void {
    if (!onLast) step = index + 1;
  }

  function back(): void {
    if (index > 0) step = index - 1;
  }
</script>

<div class="@container flex h-full flex-col">
  <div class="relative min-h-0 flex-1 overflow-hidden">
    <div class="absolute inset-0">
      {#if current === "data"}
        {@render data?.()}
      {:else if current === "language"}
        <WelcomeLanguageStep />
      {:else if current === "calendar"}
        <WelcomeCalendarStep />
      {:else if current === "blocking"}
        <WelcomeBlockingStep />
      {:else if current === "limits"}
        <WelcomeLimitsStep />
      {:else if current === "music"}
        <WelcomeMusicStep />
      {:else if current === "projects"}
        <WelcomeProjectsStep />
      {:else if current === "tasks"}
        <WelcomeTasksStep />
      {:else if current === "notes"}
        <WelcomeNotesStep />
      {:else if current === "quickNotes"}
        <WelcomeQuickNotesStep />
      {:else if current === "chat"}
        <WelcomeChatStep />
      {:else}
        <WelcomeAboutStep />
      {/if}
    </div>
  </div>

  <OnboardingNav
    primary={onLast ? last : { label: t("vaultSetup.continue"), onClick: next }}
    onBack={index > 0 ? back : undefined}
    backDisabled={onLast && lastBackDisabled}
    {progress}
  />
</div>
