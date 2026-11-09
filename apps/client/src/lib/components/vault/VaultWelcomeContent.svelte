<script lang="ts">
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { WELCOME_STEPS, welcomeStepAt } from "$lib/vault/welcome-steps";
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
   * The welcome screens, then the choice of where data lives: the language, one per feature, the app itself, and the
   * data screen the shell draws in `data`. Every screen shares one navigation bar that stays in place at the bottom
   * while the screens change above it at once, so moving between them never shifts the buttons. On the data screen the
   * primary action is `dataPrimary`, the default location; the shell's other choices sit in the data screen itself.
   * `step` counts the welcome screens and then the data screen, and is bindable so a shell can start on the data screen
   * or show the progress in its own chrome; with `showStepDots`, the navigation shows it instead.
   */
  let {
    step = $bindable(0),
    showStepDots = true,
    data,
    dataPrimary,
    dataBackDisabled = false,
  }: {
    step?: number;
    showStepDots?: boolean;
    data: Snippet;
    dataPrimary: OnboardingNavPrimary;
    /** Keeps Back off the data screen while a choice there is running. */
    dataBackDisabled?: boolean;
  } = $props();

  /** The data screen's step, right after the last welcome screen. */
  const DATA_STEP = WELCOME_STEPS.length;
  const { t } = getLocalization();
  const onData = $derived(step >= DATA_STEP);
  const current = $derived(welcomeStepAt(step));
  const index = $derived(onData ? DATA_STEP : WELCOME_STEPS.indexOf(current));
  const progress = $derived(showStepDots ? { index, count: DATA_STEP + 1 } : undefined);

  function next(): void {
    if (index < DATA_STEP) step = index + 1;
  }

  function back(): void {
    if (index > 0) step = index - 1;
  }
</script>

<div class="@container flex h-full flex-col">
  <div class="relative min-h-0 flex-1 overflow-hidden">
    <div class="absolute inset-0">
      {#if onData}
        {@render data()}
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
    primary={onData ? dataPrimary : { label: t("vaultSetup.continue"), onClick: next }}
    onBack={index > 0 ? back : undefined}
    backDisabled={onData && dataBackDisabled}
    {progress}
  />
</div>
