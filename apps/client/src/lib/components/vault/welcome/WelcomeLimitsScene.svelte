<script lang="ts">
  import { getEventColor } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { BLOCKER_APP_HOSTS, BLOCKER_APP_NAMES } from "$lib/vault/welcome-blocker";
  import {
    LIMITS_BUDGET_MINUTES,
    limitsBrowser,
    limitsEgogramTouch,
    limitsNotice,
    limitsPan,
    limitsPhoneApp,
    limitsUsage,
  } from "$lib/vault/welcome-limits-scene";
  import WelcomeBlockerBrowser from "./WelcomeBlockerBrowser.svelte";
  import WelcomeBlockerPhone from "./WelcomeBlockerPhone.svelte";
  import WelcomeBlockerStage from "./WelcomeBlockerStage.svelte";

  /**
   * One frame of the welcome scene on daily limits: a social media limit, drawn as Settings draws it, above a laptop's
   * browser and a phone. The limit holds both sites and Egogram's app; time on Y's website and on Egogram's app fills one
   * budget, each in its own color, until every entry is blocked, however often they are opened again. Every style is a
   * function of `elapsed`.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const { t } = getLocalization();
  const theme = getTheme();

  /** The header's height, which holds the limit card's fixed rows, its padding, and its border with room to spare. */
  const HEADER_H = 76;
  /** Palette slots for the limit's entries, so the drawing follows the active theme's event colors. */
  const BROWSER_COLOR = 18;
  const PHONE_COLOR = 22;
  const EGOGRAM_SITE_COLOR = 1;

  const usage = $derived(limitsUsage(elapsed));
  const browserColor = $derived(getEventColor(BROWSER_COLOR, theme.current).bg);
  const phoneColor = $derived(getEventColor(PHONE_COLOR, theme.current).bg);
  const egogramSiteColor = $derived(getEventColor(EGOGRAM_SITE_COLOR, theme.current).bg);
  const sources = $derived([
    { label: BLOCKER_APP_HOSTS.y, color: browserColor },
    { label: BLOCKER_APP_HOSTS.egogram, color: egogramSiteColor },
    { label: BLOCKER_APP_NAMES.egogram, color: phoneColor },
  ]);

  /** Minutes of use as Settings shows them, in hours and minutes. */
  function usageLabel(minutes: number): string {
    if (minutes < 60) return `${minutes}m`;
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return rest > 0 ? `${hours}h ${rest}m` : `${hours}h`;
  }
</script>

<WelcomeBlockerStage {width} {height} headerHeight={HEADER_H} pan={limitsPan(elapsed)}>
  {#snippet header()}
    <div class="flex w-full max-w-80 flex-col gap-1.5 rounded-xl border border-border bg-background px-3 py-2">
      <div class="flex h-4 min-w-0 items-center gap-2">
        <span class="truncate text-[12px] font-medium">{t("vaultSetup.limits.limitName")}</span>
        <span class="ml-auto shrink-0 text-[10.5px] text-muted-foreground tabular-nums">
          {t(
            "settings.distractions.limits.usedOfLimit",
            usageLabel(Math.floor(usage.browser + usage.phone)),
            usageLabel(LIMITS_BUDGET_MINUTES),
            t("settings.distractions.limits.today"),
          )}
        </span>
      </div>
      <div class="flex h-1.5 overflow-hidden rounded-full bg-muted">
        <span class="h-full" style:width={`${(usage.browser / LIMITS_BUDGET_MINUTES) * 100}%`} style:background-color={browserColor}></span>
        <span class="h-full" style:width={`${(usage.phone / LIMITS_BUDGET_MINUTES) * 100}%`} style:background-color={phoneColor}></span>
      </div>
      <div class="flex h-5 gap-1.5 text-[10px]">
        {#each sources as source (source.label)}
          <span class="inline-flex items-center gap-1.5 rounded-full border border-border px-2">
            <span class="size-1.5 rounded-full" style:background-color={source.color}></span>
            {source.label}
          </span>
        {/each}
      </div>
    </div>
  {/snippet}
  {#snippet browser()}
    <WelcomeBlockerBrowser frame={limitsBrowser(elapsed)} />
  {/snippet}
  {#snippet phone(phoneWidth, phoneHeight)}
    <WelcomeBlockerPhone
      width={phoneWidth}
      height={phoneHeight}
      app={limitsPhoneApp(elapsed)}
      notice={limitsNotice(elapsed)}
      touches={{ egogram: limitsEgogramTouch(elapsed) }}
    />
  {/snippet}
</WelcomeBlockerStage>
