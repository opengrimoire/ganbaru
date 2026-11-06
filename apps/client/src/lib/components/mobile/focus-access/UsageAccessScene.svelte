<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import GuideHeader from "./GuideHeader.svelte";
  import GuideLine from "./GuideLine.svelte";
  import GuideTap from "./GuideTap.svelte";
  import GuideToggle from "./GuideToggle.svelte";

  /**
   * Android's Usage access page for the app: turn its switch on, then, once the full-screen warning's countdown ends,
   * check the box and tap OK.
   */
  let { phase }: { phase: number } = $props();

  /** The phase the warning opens on; its OK counts down one second per phase from there. */
  const WARNING_PHASE = 2;
  const COUNTDOWN_SECONDS = 5;
  /** The box is checked only once the countdown ends, then OK is tapped after a beat. */
  const CHECK_PHASE = WARNING_PHASE + COUNTDOWN_SECONDS;
  const OK_TAP_PHASE = CHECK_PHASE + 2;

  const { t } = getLocalization();
  const warning = $derived(phase >= WARNING_PHASE && phase <= OK_TAP_PHASE);
  const aware = $derived(phase > CHECK_PHASE);
  const secondsLeft = $derived(Math.max(0, WARNING_PHASE + COUNTDOWN_SECONDS - phase));
  const okReady = $derived(aware && secondsLeft === 0);
  const granted = $derived(phase > OK_TAP_PHASE);
</script>

<GuideHeader title={t("mobile.focusOnboarding.guideUsageAccess")} />
<div class="flex items-center gap-2 px-3 py-2">
  <span class="min-w-0 flex-1 text-xs font-medium leading-snug text-foreground">{t("mobile.focusOnboarding.guidePermitUsageAccess")}</span>
  <GuideToggle on={granted} tapping={phase === 1} />
</div>
<div class="flex flex-col gap-1.5 px-3 pt-3">
  <GuideLine class="w-full" />
  <GuideLine class="w-full" />
  <GuideLine class="w-24" />
</div>

<div
  class={cn(
    "absolute inset-0 flex flex-col bg-card px-3 pb-3 pt-5 transition-transform duration-400 ease-out motion-reduce:transition-none",
    warning ? "translate-y-0" : "translate-y-full",
  )}
>
  <div class="flex flex-col items-center gap-2">
    <svg viewBox="0 0 24 22" class="h-6 w-7" aria-hidden="true">
      <path d="M10.27 1.5a2 2 0 0 1 3.46 0l9.53 16.5A2 2 0 0 1 21.53 21H2.47a2 2 0 0 1-1.73-3L10.27 1.5Z" class="fill-destructive" />
      <rect x="10.75" y="6.5" width="2.5" height="7.5" rx="1.25" class="fill-destructive-foreground" />
      <circle cx="12" cy="17.25" r="1.4" class="fill-destructive-foreground" />
    </svg>
    <GuideLine class="w-12 bg-foreground/25" />
  </div>
  <div class="mt-4 flex flex-col gap-1">
    <GuideLine class="w-full" />
    <GuideLine class="w-full" />
    <GuideLine class="w-full" />
    <GuideLine class="w-20" />
  </div>
  <div class="mt-4 flex flex-col gap-3">
    {#each ["w-28", "w-20"] as titleWidth (titleWidth)}
      <div class="flex items-start gap-2">
        <span class="size-5 shrink-0 rounded-md bg-foreground/10"></span>
        <div class="flex flex-1 flex-col gap-1 pt-0.5">
          <GuideLine class={cn("bg-foreground/25", titleWidth)} />
          <GuideLine class="w-full bg-foreground/8" />
          <GuideLine class="w-14 bg-foreground/8" />
        </div>
      </div>
    {/each}
  </div>
  <div class="mt-auto flex items-center gap-2">
    <span
      class={cn(
        "relative flex size-4 shrink-0 items-center justify-center rounded-full border transition-colors duration-200",
        aware ? "border-primary bg-primary text-primary-foreground" : "border-muted-foreground/60",
      )}
    >
      {#if aware}<Check class="size-2.5" strokeWidth={3} aria-hidden="true" />{/if}
      <GuideTap active={phase === CHECK_PHASE} />
    </span>
    <div class="flex flex-1 flex-col gap-1.5">
      <GuideLine class="w-full" />
      <GuideLine class="w-20" />
    </div>
  </div>
  <div class="mt-3 grid grid-cols-2 gap-2 text-xs font-medium">
    <span class="flex h-7 items-center justify-center rounded-full bg-foreground/10 text-foreground">{t("mobile.focusOnboarding.guideCancel")}</span>
    <span
      class={cn(
        "relative flex h-7 items-center justify-center rounded-full tabular-nums transition-colors duration-200",
        okReady ? "bg-destructive text-destructive-foreground" : "bg-destructive/25 text-destructive-foreground/60",
      )}
    >
      {secondsLeft > 0 ? t("mobile.focusOnboarding.guideOkCountdown", secondsLeft) : t("mobile.focusOnboarding.guideOk")}
      <GuideTap active={phase === OK_TAP_PHASE} />
    </span>
  </div>
</div>
