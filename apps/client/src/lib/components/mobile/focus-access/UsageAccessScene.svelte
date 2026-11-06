<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import GuideHeader from "./GuideHeader.svelte";
  import GuideLine from "./GuideLine.svelte";
  import GuideTap from "./GuideTap.svelte";
  import GuideToggle from "./GuideToggle.svelte";

  /**
   * Android's Usage access page for the app: turn its switch on, then accept the full-screen warning by checking the
   * box and tapping OK once its countdown ends.
   */
  let { phase }: { phase: number } = $props();

  /** The phase the warning opens on; its OK counts down one second per phase from there. */
  const WARNING_PHASE = 2;
  const COUNTDOWN_SECONDS = 3;

  const { t } = getLocalization();
  const warning = $derived(phase >= WARNING_PHASE && phase <= 6);
  const aware = $derived(phase >= 4);
  const secondsLeft = $derived(Math.max(0, WARNING_PHASE + COUNTDOWN_SECONDS - phase));
  const okReady = $derived(aware && secondsLeft === 0);
</script>

<GuideHeader title={t("mobile.focusOnboarding.guideUsageAccess")} />
<div class="flex items-center gap-2 px-3 py-2">
  <span class="min-w-0 flex-1 text-xs font-medium leading-snug text-foreground">{t("mobile.focusOnboarding.guidePermitUsageAccess")}</span>
  <GuideToggle on={phase >= 7} tapping={phase === 1} />
</div>
<div class="flex flex-col gap-1.5 px-3 pt-3">
  <GuideLine class="w-full" />
  <GuideLine class="w-full" />
  <GuideLine class="w-24" />
</div>

<div
  class={cn(
    "absolute inset-x-0 bottom-0 top-3 flex flex-col rounded-t-2xl border-t border-border bg-card px-3 pb-3 pt-4 transition-transform duration-400 ease-out motion-reduce:transition-none",
    warning ? "translate-y-0" : "translate-y-full",
  )}
>
  <div class="flex flex-col items-center gap-3">
    <TriangleAlert class="size-7 text-destructive" aria-hidden="true" />
    <div class="flex w-full flex-col gap-1.5">
      <GuideLine class="w-full" />
      <GuideLine class="w-full" />
      <GuideLine class="w-28" />
    </div>
  </div>
  <div class="mt-auto flex items-center gap-2">
    <span
      class={cn(
        "relative flex size-4 shrink-0 items-center justify-center rounded-full border transition-colors duration-200",
        aware ? "border-primary bg-primary text-primary-foreground" : "border-muted-foreground/60",
      )}
    >
      {#if aware}<Check class="size-2.5" strokeWidth={3} aria-hidden="true" />{/if}
      <GuideTap active={phase === 3} />
    </span>
    <div class="flex flex-1 flex-col gap-1.5">
      <GuideLine class="w-full" />
      <GuideLine class="w-20" />
    </div>
  </div>
  <div class="mt-3 grid grid-cols-2 gap-2 text-xs font-medium">
    <span class="flex h-7 items-center justify-center rounded-full bg-foreground/8 text-foreground">{t("mobile.focusOnboarding.guideCancel")}</span>
    <span
      class={cn(
        "relative flex h-7 items-center justify-center rounded-full tabular-nums transition-colors duration-200",
        okReady ? "bg-destructive text-destructive-foreground" : "bg-destructive/25 text-destructive-foreground/60",
      )}
    >
      {secondsLeft > 0 ? t("mobile.focusOnboarding.guideOkCountdown", secondsLeft) : t("mobile.focusOnboarding.guideOk")}
      <GuideTap active={phase === 6} />
    </span>
  </div>
</div>
