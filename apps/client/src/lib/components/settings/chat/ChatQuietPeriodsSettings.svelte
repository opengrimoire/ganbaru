<script lang="ts">
  import { onMount } from "svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash";
  import { readChatQuietPeriods, readChatQuietState, saveChatQuietPeriods } from "$lib/api/chat";
  import {
    CHAT_MAX_QUIET_WINDOWS,
    type ChatQuietStateRead,
    type ChatQuietWindow,
  } from "$lib/chat/contracts";
  import { chatLocalMinuteTime } from "$lib/chat/composer/message-scheduling";
  import {
    addChatQuietWindow,
    chatQuietTimeMinutes,
    removeChatQuietWindow,
    updateChatQuietWindow,
  } from "$lib/chat/composer/quiet-periods";
  import { chatQuietWindowSummary } from "$lib/chat/composer/recurrence-presentation";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { announceChatQuietPeriodsSaved } from "$lib/chat/notifications/quiet-status.svelte";
  import { formatTimeLabel } from "$lib/calendar/utils";
  import ChatWeekdayToggles from "$lib/components/chat/composer/ChatWeekdayToggles.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { SETTINGS_ICON_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { localTimezone } from "$lib/stores/calendar/event-payloads";
  import { getPreferences } from "$lib/stores/preferences.svelte";

  /** This device's quiet windows, during which repeating Chat messages wait. They never sync. */
  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  let windows = $state<ChatQuietWindow[] | null>(null);
  let quietState = $state<ChatQuietStateRead | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);

  const display = $derived({
    t,
    locale: localization.locale,
    timeFormat: preferences.calendarTimeFormat,
    localTimeZone: localTimezone(),
  });
  const quietStateLabel = $derived.by(() => {
    if (!quietState?.quiet) return null;
    const ends = quietState.endsAt
      ? formatDateTime(localization.locale, Date.parse(quietState.endsAt), { dateStyle: "medium", timeStyle: "short" })
      : null;
    if (quietState.reason === "focus") {
      return ends ? t("settings.chat.quietPeriods.quietFocusUntil", ends) : t("settings.chat.quietPeriods.quietFocus");
    }
    return ends ? t("settings.chat.quietPeriods.quietWindowUntil", ends) : t("settings.chat.quietPeriods.quietWindow");
  });

  onMount(() => {
    void load();
  });

  async function load(): Promise<void> {
    error = null;
    try {
      const [preferencesRead, state] = await Promise.all([readChatQuietPeriods(), readChatQuietState()]);
      windows = preferencesRead.windows;
      quietState = state;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
    }
  }

  async function save(next: ChatQuietWindow[] | null): Promise<void> {
    if (next === null || saving) return;
    const previous = windows;
    windows = next;
    saving = true;
    error = null;
    try {
      windows = (await saveChatQuietPeriods({ windows: next })).windows;
    } catch (cause: unknown) {
      windows = previous;
      error = chatErrorMessage(cause);
      saving = false;
      return;
    }
    announceChatQuietPeriodsSaved();
    try {
      quietState = await readChatQuietState();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
    } finally {
      saving = false;
    }
  }

  function timeOptions(current: number): { value: string; label: string }[] {
    return chatQuietTimeMinutes(current).map((minute) => ({
      value: String(minute),
      label: formatTimeLabel(chatLocalMinuteTime(minute), preferences.calendarTimeFormat),
    }));
  }
</script>

<section class="flex flex-col gap-4">
  <div class="px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.quietPeriods.heading")}</h2>
    <p class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.chat.quietPeriods.description")}</p>
  </div>
  {#if error}<Notice class="text-sm" message={error} onRetry={windows ? undefined : () => void load()} />{/if}
  {#if quietStateLabel}<p role="status" class="px-1 text-[0.8rem] text-muted-foreground">{quietStateLabel}</p>{/if}
  {#if windows}
    {#if windows.length === 0}
      <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.chat.quietPeriods.empty")}</p>
    {/if}
    {#each windows as window, index (index)}
      <div class="quiet-window">
        <div class="flex items-center gap-2">
          <span class="min-w-0 flex-1 truncate text-[0.866667rem] text-foreground">{chatQuietWindowSummary(window, display)}</span>
          <button
            type="button"
            class={SETTINGS_ICON_BUTTON_CLASS}
            aria-label={t("settings.chat.quietPeriods.remove")}
            disabled={saving}
            onclick={() => void save(removeChatQuietWindow(windows ?? [], index))}
          ><Trash2 size={13} /></button>
        </div>
        <ChatWeekdayToggles
          mask={window.weekdays}
          label={t("settings.chat.quietPeriods.days")}
          disabled={saving}
          onChange={(weekdays) => void save(updateChatQuietWindow(windows ?? [], index, { weekdays }))}
        />
        <div class="flex flex-wrap items-center gap-2 text-[0.8rem] text-muted-foreground">
          <span>{t("settings.chat.quietPeriods.from")}</span>
          <Select
            inline
            class="w-28"
            ariaLabel={t("settings.chat.quietPeriods.from")}
            value={String(window.startMinute)}
            options={timeOptions(window.startMinute)}
            disabled={saving}
            onChange={(value) => void save(updateChatQuietWindow(windows ?? [], index, { startMinute: Number(value) }))}
          />
          <span>{t("settings.chat.quietPeriods.to")}</span>
          <Select
            inline
            class="w-28"
            ariaLabel={t("settings.chat.quietPeriods.to")}
            value={String(window.endMinute)}
            options={timeOptions(window.endMinute).filter((option) => option.value !== String(window.startMinute))}
            disabled={saving}
            onChange={(value) => void save(updateChatQuietWindow(windows ?? [], index, { endMinute: Number(value) }))}
          />
        </div>
      </div>
    {/each}
    <div class="px-1">
      <button
        type="button"
        class={SETTINGS_SECONDARY_BUTTON_CLASS}
        disabled={saving || windows.length >= CHAT_MAX_QUIET_WINDOWS}
        onclick={() => void save(addChatQuietWindow(windows ?? []))}
      ><Plus size={13} />{t("settings.chat.quietPeriods.add")}</button>
    </div>
  {/if}
</section>

<style>
  .quiet-window { display:grid; gap:0.45rem; border:1px solid var(--border); border-radius:0.375rem; padding:0.6rem; }
</style>
