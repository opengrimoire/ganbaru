<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import { WELCOME_COLUMN_CLASS } from "$lib/vault/welcome-layout";
  import { WELCOME_STEPS, welcomeStepAt } from "$lib/vault/welcome-steps";
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
   * The welcome screens before vault setup: the language, one per feature, then the app itself. Every screen shares
   * one navigation bar that stays in place at the bottom while the screens change above it at once, so moving between
   * them never shifts the buttons. Continue on the last screen hands over to vault setup. `step` is bindable so a shell
   * can show the progress in its own chrome; with `showStepDots`, the navigation shows it instead.
   */
  let {
    step = $bindable(0),
    showStepDots = true,
    onContinue,
  }: { step?: number; showStepDots?: boolean; onContinue: () => void } = $props();

  const { t } = getLocalization();
  const current = $derived(welcomeStepAt(step));
  const index = $derived(WELCOME_STEPS.indexOf(current));

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
    <div class="absolute inset-0">
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

  <div class={cn(WELCOME_COLUMN_CLASS, "shrink-0 pb-6 @min-[860px]:pb-10")}>
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
        class="inline-flex min-h-11 flex-1 items-center justify-center gap-2 rounded-md bg-primary px-5 py-2.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
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