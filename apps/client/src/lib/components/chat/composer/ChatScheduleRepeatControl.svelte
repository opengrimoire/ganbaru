<script lang="ts">
  import {
    CHAT_REPEAT_CHOICES,
    CHAT_REPEAT_MAX_INTERVAL_HOURS,
    CHAT_REPEAT_MIN_INTERVAL_HOURS,
    type ChatRepeatChoice,
    type ChatRepeatSelection,
  } from "$lib/chat/composer/message-scheduling";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ChatWeekdayToggles from "./ChatWeekdayToggles.svelte";

  /** How a scheduled message repeats: a choice, its custom days, and its interval in hours. */
  let {
    repeat,
    summary = null,
    disabled = false,
    unavailableReason = null,
    onChange,
  }: {
    repeat: ChatRepeatSelection;
    /** Localized description of the rule the current choice produces, when it repeats. */
    summary?: string | null;
    disabled?: boolean;
    /** Explains why repeating is unavailable; the choice is disabled and the reason shows as its tooltip. */
    unavailableReason?: string | null;
    onChange: (repeat: ChatRepeatSelection) => void;
  } = $props();

  const locked = $derived(disabled || unavailableReason !== null);

  const { t } = getLocalization();
  const choiceLabels: Record<ChatRepeatChoice, () => string> = {
    none: () => t("chat.organization.repeat.none"),
    daily: () => t("chat.organization.repeat.daily"),
    weekdays: () => t("chat.organization.repeat.weekdays"),
    weekly: () => t("chat.organization.repeat.weekly"),
    custom_days: () => t("chat.organization.repeat.customDays"),
    interval: () => t("chat.organization.repeat.interval"),
  };
  const options = $derived(CHAT_REPEAT_CHOICES.map((choice) => ({ value: choice, label: choiceLabels[choice]() })));

  function isRepeatChoice(value: string): value is ChatRepeatChoice {
    return (CHAT_REPEAT_CHOICES as readonly string[]).includes(value);
  }

  function setIntervalHours(value: string): void {
    const hours = Number.parseInt(value, 10);
    if (!Number.isFinite(hours)) return;
    const bounded = Math.min(CHAT_REPEAT_MAX_INTERVAL_HOURS, Math.max(CHAT_REPEAT_MIN_INTERVAL_HOURS, hours));
    onChange({ ...repeat, intervalHours: bounded });
  }
</script>

<div class="repeat-control">
  <div class="repeat-row" class:unavailable={unavailableReason !== null} data-app-tooltip={unavailableReason ?? undefined}>
    <strong>{t("chat.organization.repeat.label")}</strong>
    <Select
      inline
      class="w-auto min-w-0 flex-1"
      ariaLabel={t("chat.organization.repeat.label")}
      value={repeat.choice}
      {options}
      disabled={locked}
      onChange={(value) => { if (isRepeatChoice(value)) onChange({ ...repeat, choice: value }); }}
    />
  </div>
  {#if unavailableReason === null && repeat.choice === "custom_days"}
    <ChatWeekdayToggles
      mask={repeat.weekdays}
      label={t("chat.organization.repeat.customDays")}
      {disabled}
      onChange={(weekdays) => onChange({ ...repeat, weekdays })}
    />
  {:else if unavailableReason === null && repeat.choice === "interval"}
    <label class="interval-row text-panel-detail text-muted-foreground">
      <span>{t("chat.organization.repeat.intervalHours")}</span>
      <input
        type="number"
        class="field w-16 text-foreground"
        min={CHAT_REPEAT_MIN_INTERVAL_HOURS}
        max={CHAT_REPEAT_MAX_INTERVAL_HOURS}
        step="1"
        value={repeat.intervalHours}
        {disabled}
        onchange={(event) => setIntervalHours(event.currentTarget.value)}
      />
    </label>
  {/if}
  {#if summary}<p class="text-panel-detail text-muted-foreground">{summary}</p>{/if}
</div>

<style>
  .repeat-control { display:grid; gap:0.3rem; padding:0.15rem 0.25rem 0.3rem; }
  .interval-row { display:flex; align-items:center; justify-content:space-between; gap:0.5rem; }
  .repeat-row { display:flex; align-items:center; gap:0.75rem; }
  .repeat-row strong { flex:0 0 auto; font-weight:600; }
  /* The disabled select passes the pointer to its row so the row shows the reason and the not-allowed cursor. */
  .repeat-row.unavailable { cursor:not-allowed; }
  .repeat-row.unavailable :global(button) { pointer-events:none; }
</style>
