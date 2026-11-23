<script lang="ts" module>
  /** The primary action of an onboarding screen. */
  export interface OnboardingNavPrimary {
    label: string;
    onClick: () => void;
    /** Shows a spinner in place of the arrow while the action runs. */
    busy?: boolean;
    disabled?: boolean;
    /** Whether the label is followed by a forward arrow; on by default, since the action moves on. */
    arrow?: boolean;
  }

  /**
   * A notice the user acknowledges before the primary action takes effect. It points at the primary action, which
   * dims while the notice shows so the acknowledgement reads as the next step.
   */
  export interface OnboardingNavNotice {
    message: string;
    acknowledgeLabel: string;
    onAcknowledge: () => void;
    /** Closes the notice without acknowledging it, after a press elsewhere or Escape. */
    onDismiss: () => void;
  }

  /** An onboarding screen's position among the screens its dots count. */
  export interface OnboardingNavProgress {
    index: number;
    count: number;
  }
</script>

<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { keepInViewport } from "$lib/utils/keep-in-viewport";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import { WELCOME_COLUMN_CLASS } from "$lib/vault/welcome-layout";

  /**
   * The navigation every onboarding screen shares at its bottom: Back, when the screen has somewhere to go back to or
   * `backLocked` shows it disabled, and the primary action, which Enter also runs. A screen's other choices sit right
   * above them, under the same divider, so the primary action keeps its place as screens change. With `progress`, dots
   * after the primary action show how far along the screens are.
   */
  let {
    primary,
    onBack,
    backDisabled = false,
    backLocked = false,
    secondary,
    progress,
    primaryNotice = null,
    class: className = "",
  }: {
    primary: OnboardingNavPrimary;
    /** Goes back a screen; without it, Back is left out. */
    onBack?: () => void;
    backDisabled?: boolean;
    /** Shows Back disabled when the screen has nowhere to go back to, so the navigation keeps its shape. */
    backLocked?: boolean;
    /** The screen's other choices, above Back and the primary action. */
    secondary?: Snippet;
    progress?: OnboardingNavProgress;
    /** A notice shown above the primary action while it waits for acknowledgement. */
    primaryNotice?: OnboardingNavNotice | null;
    class?: string;
  } = $props();

  const { t } = getLocalization();

  /** Moves focus to the notice's acknowledgement so Enter and screen readers land on it. */
  function focusOnMount(node: HTMLElement): void {
    node.focus();
  }
</script>

<div class={cn(WELCOME_COLUMN_CLASS, "shrink-0 pb-6 @min-[860px]:pb-10", className)}>
  <div class="flex flex-col gap-2 border-t border-border pt-4">
    {@render secondary?.()}
    <div class="flex items-center gap-2">
      {#if onBack || backLocked}
        <button
          type="button"
          class="inline-flex min-h-11 shrink-0 items-center justify-center gap-2 rounded-md border border-border bg-background px-4 py-2.5 text-sm font-medium text-foreground transition-colors hover:bg-accent active:bg-accent disabled:cursor-not-allowed disabled:opacity-60"
          disabled={backDisabled || !onBack}
          onclick={onBack}
        >
          <ArrowLeft size={16} strokeWidth={1.8} aria-hidden="true" />
          {t("common.back")}
        </button>
      {/if}
      <!-- Presses on the primary action count as inside the notice, so pressing it again keeps the notice open. -->
      <div
        class="relative flex min-w-0 flex-1"
        use:dismissOnOutside={{ enabled: primaryNotice !== null, onDismiss: () => primaryNotice?.onDismiss() }}
      >
        <button
          use:onboardingPrimaryAction
          type="button"
          class={cn(
            "inline-flex min-h-11 min-w-0 flex-1 items-center justify-center gap-2 rounded-md bg-primary px-5 py-2.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 active:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60",
            primaryNotice && "opacity-40",
          )}
          disabled={primary.disabled || primary.busy}
          aria-busy={primary.busy || undefined}
          aria-expanded={primaryNotice ? true : undefined}
          onclick={primary.onClick}
        >
          <span class="min-w-0 truncate">{primary.label}</span>
          {#if primary.busy}
            <LoaderCircle size={16} strokeWidth={2} class="shrink-0 animate-spin" aria-hidden="true" />
          {:else if primary.arrow ?? true}
            <ArrowRight size={16} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
          {/if}
        </button>
        {#if primaryNotice}
          <!-- A zero-width anchor at the primary action's center; the notice overflows it equally on both sides, which
               keeps the notice's own translate free for keepInViewport. -->
          <div class="absolute bottom-[calc(100%+0.75rem)] left-1/2 z-40 flex w-0 justify-center">
            <div
              use:keepInViewport
              role="dialog"
              aria-labelledby="onboarding-primary-notice"
              class="surface-floating flex w-floating shrink-0 flex-col gap-3 p-3"
            >
              <p id="onboarding-primary-notice" class="text-sm text-foreground">{primaryNotice.message}</p>
              <button
                use:focusOnMount
                type="button"
                class="inline-flex min-h-10 items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 active:bg-primary/90"
                onclick={primaryNotice.onAcknowledge}
              >
                {primaryNotice.acknowledgeLabel}
              </button>
            </div>
          </div>
          <!-- The pointer stays centered on the primary action even when the notice slides to fit a narrow screen. -->
          <span
            class="absolute bottom-[calc(100%+0.75rem-6px)] left-1/2 z-40 size-3 -translate-x-1/2 rotate-45 border-r border-b border-border bg-popover"
            aria-hidden="true"
          ></span>
        {/if}
      </div>
      {#if progress}
        <span class="ml-auto flex shrink-0 gap-1.5 pl-3" aria-hidden="true">
          {#each { length: progress.count } as _, dotIndex (dotIndex)}
            <span class={cn("h-1 w-4 rounded-full transition-colors duration-300", dotIndex <= progress.index ? "bg-primary" : "bg-foreground/15")}></span>
          {/each}
        </span>
        <span class="sr-only">{t("vaultSetup.stepProgress", progress.index + 1, progress.count)}</span>
      {/if}
    </div>
  </div>
</div>
