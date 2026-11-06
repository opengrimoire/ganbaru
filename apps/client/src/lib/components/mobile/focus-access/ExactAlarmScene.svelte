<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import GuideAppIcon from "./GuideAppIcon.svelte";
  import GuideHeader from "./GuideHeader.svelte";
  import GuideLine from "./GuideLine.svelte";
  import GuideToggle from "./GuideToggle.svelte";

  /** Android's Alarms and reminders page for the app: turn its switch on. */
  let { phase, appLabel }: { phase: number; appLabel: string } = $props();

  const { t } = getLocalization();
</script>

<GuideHeader title={t("mobile.focusOnboarding.guideAlarms")} />
<div class="flex items-center gap-2 px-3 py-2">
  <GuideAppIcon />
  <span class="min-w-0 truncate text-xs font-medium text-foreground">{appLabel}</span>
</div>
<div class="mx-3 mt-1 flex items-center gap-2 border-t border-border pt-3">
  <span class="min-w-0 flex-1 text-xs leading-snug text-foreground">{t("mobile.focusOnboarding.guideAllowAlarms")}</span>
  <GuideToggle on={phase >= 2} tapping={phase === 1} />
</div>
<div class="flex flex-col gap-1.5 px-3 pt-4">
  <GuideLine class="w-36" />
  <GuideLine class="w-32" />
  <GuideLine class="w-20" />
</div>
