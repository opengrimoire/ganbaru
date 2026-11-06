<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import { WELCOME_STEPS, welcomeStepAt } from "$lib/vault/welcome-steps";
  import WelcomeAboutStep from "./welcome/WelcomeAboutStep.svelte";
  import WelcomeCalendarStep from "./welcome/WelcomeCalendarStep.svelte";
  import WelcomeLanguageStep from "./welcome/WelcomeLanguageStep.svelte";

  /**
   * The welcome screens before vault setup: the language, one per feature, then the app's credentials. Each screen
   * lays out its own content and places the shared navigation where it fits. Continue on the last screen hands
   * over to vault setup. `step` is bindable so a shell can show the progress in its own chrome; with `showStepDots`,
   * the screens show it beside their navigation instead.
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

{#snippet actions()}
  <div class="flex items-center gap-2">
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
          <span class={cn("h-1 w-4 rounded-full", dotIndex <= index ? "bg-primary" : "bg-foreground/15")}></span>
        {/each}
      </span>
      <span class="sr-only">{t("vaultSetup.stepProgress", index + 1, WELCOME_STEPS.length)}</span>
    {/if}
  </div>
{/snippet}

{#if current === "language"}
  <WelcomeLanguageStep {actions} />
{:else if current === "calendar"}
  <WelcomeCalendarStep {actions} />
{:else}
  <WelcomeAboutStep {actions} />
{/if}
