<script lang="ts">
  import Globe from "@lucide/svelte/icons/globe";
  import Smartphone from "@lucide/svelte/icons/smartphone";
  import { getEventColor } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { BLOCKER_APP_HOSTS, BLOCKER_APP_NAMES } from "$lib/vault/welcome-blocker";
  import {
    LIMITS_BUDGET_MINUTES,
    limitsBrowserPage,
    limitsNotice,
    limitsPageIn,
    limitsPan,
    limitsPhoneApp,
    limitsUsage,
    limitsYTouch,
  } from "$lib/vault/welcome-limits-scene";
  import WelcomeBlockerBrowser from "./WelcomeBlockerBrowser.svelte";
  import WelcomeBlockerPhone from "./WelcomeBlockerPhone.svelte";
  import WelcomeBlockerStage from "./WelcomeBlockerStage.svelte";
  import WelcomeParodyIcon from "./WelcomeParodyIcon.svelte";

  /**
   * One frame of the welcome scene on daily limits: Y's daily limit, drawn as Settings draws it, above a laptop's
   * browser and a phone. Time on Y's website and on its app fills one budget, each device in its own color, until both
   * are blocked. Every style is a function of `elapsed`.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const { t } = getLocalization();
  const theme = getTheme();

  const HEADER_H = 64;
  /** Palette slots for the limit's sources, so the drawing follows the active theme's event colors. */
  const BROWSER_COLOR = 18;
  const PHONE_COLOR = 22;

  const usage = $derived(limitsUsage(elapsed));
  const browserColor = $derived(getEventColor(BROWSER_COLOR, theme.current).bg);
  const phoneColor = $derived(getEventColor(PHONE_COLOR, theme.current).bg);

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
      <div class="flex min-w-0 items-center gap-2">
        <WelcomeParodyIcon app="y" size={14} />
        <span class="text-[12px] font-medium">{BLOCKER_APP_NAMES.y}</span>
        <span class="ml-auto truncate text-[10.5px] text-muted-foreground">
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
      <div class="flex gap-1.5 text-[10px]">
        <span class="flex items-center gap-1 rounded-full border border-border px-1.5 py-px">
          <span class="size-1.5 rounded-full" style:background-color={browserColor}></span>
          <Globe size={10} strokeWidth={1.8} aria-hidden="true" />
          {BLOCKER_APP_HOSTS.y}
        </span>
        <span class="flex items-center gap-1 rounded-full border border-border px-1.5 py-px">
          <span class="size-1.5 rounded-full" style:background-color={phoneColor}></span>
          <Smartphone size={10} strokeWidth={1.8} aria-hidden="true" />
          {BLOCKER_APP_NAMES.y}
        </span>
      </div>
    </div>
  {/snippet}
  {#snippet browser()}
    <WelcomeBlockerBrowser app="y" page={limitsBrowserPage(elapsed)} pageIn={limitsPageIn(elapsed)} />
  {/snippet}
  {#snippet phone(phoneWidth, phoneHeight)}
    <WelcomeBlockerPhone
      width={phoneWidth}
      height={phoneHeight}
      app={limitsPhoneApp(elapsed)}
      notice={limitsNotice(elapsed)}
      touches={{ y: limitsYTouch(elapsed) }}
    />
  {/snippet}
</WelcomeBlockerStage>
