<script lang="ts">
  import { tick } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { commitIntegerDraft, moveRovingIndex, panelInputKeydown } from "$lib/calendar/event-panel-utils";
  import { formatNotificationOffset } from "$lib/calendar/event-notifications";
  import Bell from "@lucide/svelte/icons/bell";
  import X from "@lucide/svelte/icons/x";
  import Plus from "@lucide/svelte/icons/plus";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const PRESET_MINUTES = [
    0,
    5,
    10,
    30,
    60,
    1440,
  ];

  const CUSTOM_UNIT_MINUTES = [
    1,
    60,
    1440,
    10080,
  ];

  let {
    enabled,
    selected = $bindable(new Set<number>()),
    customNotifications = $bindable<{ amount: number; unit: number }[]>([]),
    expanded,
    onToggle,
    onExpand,
    onChange,
    deliveryNotice = null,
    deliveryActionLabel = null,
    deliveryActionBusy = false,
    onDeliveryAction,
  }: {
    enabled: boolean;
    selected: Set<number>;
    customNotifications: { amount: number; unit: number }[];
    expanded: boolean;
    onToggle: () => void;
    onExpand: () => void;
    onChange: () => void;
    deliveryNotice?: string | null;
    deliveryActionLabel?: string | null;
    deliveryActionBusy?: boolean;
    onDeliveryAction?: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);
  const presetOptions = $derived.by(() =>
    PRESET_MINUTES.map((value) => ({
      value,
      label: formatNotificationOffset(value, t),
    })),
  );
  const unitOptions = $derived.by(() =>
    CUSTOM_UNIT_MINUTES.map((value) => ({ value, label: unitLabel(value) })),
  );

  let openDropdownIndex = $state<number | null>(null);
  let dropdownButtons: (HTMLButtonElement | undefined)[] = $state([]);
  let sectionEl: HTMLDivElement | undefined = $state();
  let presetFocusIndex = $state(0);
  let unitFocusIndex = $state(0);
  let customAmountDrafts = $state<string[]>([]);

  $effect(() => {
    customAmountDrafts = customNotifications.map((notif) => String(notif.amount));
  });

  function togglePreset(minutes: number) {
    const next = new Set(selected);
    if (next.has(minutes)) next.delete(minutes);
    else next.add(minutes);
    selected = next;
    onChange();
  }

  function addCustomNotification() {
    if (customNotifications.length >= 2) return;
    customNotifications = [...customNotifications, { amount: 1, unit: 10080 }];
    onChange();
  }

  function removeCustomNotification(idx: number) {
    customNotifications = customNotifications.filter((_, i) => i !== idx);
    onChange();
  }

  function updateCustomNotification(idx: number, amount: number, unit: number) {
    customNotifications = customNotifications.map((n, i) => i === idx ? { amount, unit } : n);
    onChange();
  }

  async function focusPresetButton(index: number) {
    await tick();
    sectionEl
      ?.querySelector<HTMLButtonElement>(`[data-notification-roving="preset"][data-roving-index="${index}"]`)
      ?.focus();
  }

  async function focusDropdownButton(idx: number) {
    await tick();
    dropdownButtons[idx]?.focus();
  }

  async function focusUnitButton(index: number) {
    await tick();
    sectionEl
      ?.querySelector<HTMLButtonElement>(`[data-notification-unit-index="${index}"]`)
      ?.focus();
  }

  function handlePresetKeydown(e: KeyboardEvent, index: number) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const nextIndex = moveRovingIndex({
      currentIndex: index,
      itemCount: presetOptions.length,
      key: e.key,
      orientation: "vertical",
    });
    if (nextIndex === index) return;
    e.preventDefault();
    e.stopPropagation();
    presetFocusIndex = nextIndex;
    void focusPresetButton(nextIndex);
  }

  function selectedUnitIndex(unit: number): number {
    return Math.max(0, unitOptions.findIndex((option) => option.value === unit));
  }

  function openDropdown(idx: number, source: "keyboard" | "pointer") {
    openDropdownIndex = idx;
    unitFocusIndex = selectedUnitIndex(customNotifications[idx]?.unit ?? 1);
    if (source === "keyboard") void focusUnitButton(unitFocusIndex);
  }

  function closeDropdown(source: "keyboard" | "pointer") {
    const idx = openDropdownIndex;
    openDropdownIndex = null;
    if (source === "keyboard" && idx !== null) void focusDropdownButton(idx);
  }

  function toggleDropdown(idx: number) {
    if (openDropdownIndex === idx) closeDropdown("pointer");
    else openDropdown(idx, "pointer");
  }

  function selectUnit(idx: number, amount: number, unit: number, source: "keyboard" | "pointer") {
    updateCustomNotification(idx, amount, unit);
    closeDropdown(source);
  }

  function handleDropdownButtonKeydown(e: KeyboardEvent, idx: number) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    e.stopPropagation();
    openDropdown(idx, "keyboard");
  }

  function handleUnitKeydown(e: KeyboardEvent, idx: number, optionIndex: number, amount: number, unit: number) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;

    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closeDropdown("keyboard");
      return;
    }

    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      e.stopPropagation();
      selectUnit(idx, amount, unit, "keyboard");
      return;
    }

    const nextIndex = moveRovingIndex({
      currentIndex: optionIndex,
      itemCount: unitOptions.length,
      key: e.key,
      orientation: "vertical",
    });
    if (nextIndex === optionIndex) return;
    e.preventDefault();
    e.stopPropagation();
    unitFocusIndex = nextIndex;
    void focusUnitButton(nextIndex);
  }

  function commitCustomAmountDraft(idx: number) {
    const current = customNotifications[idx];
    if (!current) return;
    const result = commitIntegerDraft(customAmountDrafts[idx] ?? "", current.amount, 1, 999);
    const nextDrafts = [...customAmountDrafts];
    nextDrafts[idx] = String(result.value);
    customAmountDrafts = nextDrafts;
    if (result.committed) updateCustomNotification(idx, result.value, current.unit);
  }

  function restoreCustomAmountDraft(idx: number) {
    const current = customNotifications[idx];
    if (!current) return;
    const nextDrafts = [...customAmountDrafts];
    nextDrafts[idx] = String(current.amount);
    customAmountDrafts = nextDrafts;
  }

  function setCustomAmountDraft(idx: number, value: string) {
    const nextDrafts = [...customAmountDrafts];
    nextDrafts[idx] = value;
    customAmountDrafts = nextDrafts;
  }

  function handleNumberDraftKeydown(e: KeyboardEvent, idx: number) {
    if (!e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
      if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        commitCustomAmountDraft(idx);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        restoreCustomAmountDraft(idx);
        return;
      }
    }
    panelInputKeydown(e);
  }

  function positionDropdown(node: HTMLElement, idx: number) {
    const trigger = dropdownButtons[idx];
    if (!trigger) return { destroy() {} };
    const triggerRect = trigger.getBoundingClientRect();
    const dropdownWidth = node.offsetWidth || 120;
    let left = triggerRect.left;
    left = Math.max(8, Math.min(window.innerWidth - dropdownWidth - 8, left));
    node.style.left = `${left}px`;
    node.style.top = `${triggerRect.bottom + 4}px`;
    return { destroy() {} };
  }

  function unitLabel(unit: number): string {
    if (unit === 60) return t("calendar.notifications.unitHours");
    if (unit === 1440) return t("calendar.notifications.unitDays");
    if (unit === 10080) return t("calendar.notifications.unitWeeks");
    return t("calendar.notifications.unitMinutes");
  }

  const summary = $derived.by(() => {
    if (!enabled) return "";
    const all: number[] = [...selected];
    for (const notification of customNotifications) all.push(notification.amount * notification.unit);
    if (all.length === 0) return t("calendar.notifications.none");
    const sorted = [...new Set(all)].sort((a, b) => a - b);
    return formatList(locale, sorted.map((m) => {
      const preset = presetOptions.find((p) => p.value === m);
      return preset ? preset.label : formatNotificationOffset(m, t);
    }));
  });
</script>

<div bind:this={sectionEl} class="flex flex-col rounded-none overflow-hidden" style="background-color: var(--panel-contrast);">
  <div class="section-header flex items-stretch">
    <button onclick={onToggle}
      class="flex w-10 shrink-0 items-center justify-center
        {enabled ? 'bg-black/3 dark:bg-black/30 text-foreground' : 'text-muted-foreground/50'}">
      <Bell size={14} />
    </button>
    <button onclick={onExpand}
      class="flex flex-1 items-center gap-2.5 px-3 py-2 text-left">
      <span class="translate-y-[1.13px] text-[0.8rem] {enabled ? 'text-foreground' : 'text-muted-foreground'}">{t("calendar.notifications.title")}</span>
      <span class="ml-auto translate-y-[1.13px] truncate text-[0.733333rem] text-muted-foreground">{summary}</span>
    </button>
  </div>
  {#if expanded}
    <div transition:slide={{ duration: 180, easing: cubicOut }} data-section="notifications" class="flex flex-col gap-1.5 p-2.5" style="background-color: var(--panel-bg);">
      {#if deliveryNotice}
        <div class="mb-1 flex flex-col gap-2 rounded-md border border-warning/35 bg-warning/8 px-2.5 py-2 text-[0.733333rem] leading-5 text-foreground">
          <span>{deliveryNotice}</span>
          {#if deliveryActionLabel && onDeliveryAction}
            <button
              type="button"
              disabled={deliveryActionBusy}
              class="min-h-8 self-start rounded-md border border-border bg-card px-2.5 text-[0.733333rem] font-medium text-foreground disabled:opacity-60"
              onclick={onDeliveryAction}
            >
              {deliveryActionLabel}
            </button>
          {/if}
        </div>
      {/if}
      <div class="flex flex-col gap-0.5">
        {#each presetOptions as opt, index}
          <button
            onclick={() => togglePreset(opt.value)}
            onfocus={() => { presetFocusIndex = index; }}
            onkeydown={(e) => handlePresetKeydown(e, index)}
            data-notification-roving="preset"
            data-roving-index={index}
            tabindex={presetFocusIndex === index ? 0 : -1}
            class="flex items-center gap-2.5 rounded-none px-2.5 py-1.5 text-left text-[0.8rem] text-foreground"
          >
            <div class="size-3 shrink-0
              {selected.has(opt.value) ? 'bg-form-indicator' : 'border border-muted-foreground/40'}">
            </div>
            <span>{opt.label}</span>
          </button>
        {/each}
      </div>
      {#each customNotifications as notification, idx}
        <div class="flex items-center gap-2 px-2">
          <input type="number" value={customAmountDrafts[idx] ?? String(notification.amount)} min={1} max={999}
            oninput={(e) => setCustomAmountDraft(idx, e.currentTarget.value)}
            onblur={() => commitCustomAmountDraft(idx)}
            class="num-input w-11 rounded bg-black/5 px-1 py-0.5 text-center text-[0.8rem] text-event-panel-input-text outline-none dark:bg-black/15"
            onkeydown={(e) => handleNumberDraftKeydown(e, idx)} />
          <button bind:this={dropdownButtons[idx]}
            onclick={() => toggleDropdown(idx)}
            onkeydown={(e) => handleDropdownButtonKeydown(e, idx)}
            class="rounded bg-black/5 px-2.5 py-0.5 text-[0.8rem] dark:bg-black/15
              {openDropdownIndex === idx ? 'ring-1 ring-primary/60' : 'hover:bg-black/5 dark:hover:bg-black/15'}
              text-event-panel-input-text">
            {unitOptions.find((u) => u.value === notification.unit)?.label ?? t("calendar.notifications.unitMinutes")}
          </button>
          <span class="text-[0.8rem] text-muted-foreground">{t("calendar.notifications.before")}</span>
          <button onclick={() => { removeCustomNotification(idx); openDropdownIndex = null; }}
            aria-label={t("calendar.notifications.removeCustom")}
            class="ml-auto flex h-6 w-6 items-center justify-center rounded text-muted-foreground hover:bg-destructive/10 hover:text-destructive">
            <X size={13} />
          </button>

          <!-- Floating unit dropdown -->
          {#if openDropdownIndex === idx}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="fixed inset-0 z-60" onclick={() => closeDropdown("pointer")}></div>
            <div class="fixed z-61 rounded-lg bg-popover py-1 shadow-lg ring-1 ring-border/60"
              use:positionDropdown={idx}>
              {#each unitOptions as u, unitIndex}
                <button onclick={() => selectUnit(idx, notification.amount, u.value, "pointer")}
                  data-notification-unit-index={unitIndex}
                  tabindex={unitFocusIndex === unitIndex ? 0 : -1}
                  onfocus={() => { unitFocusIndex = unitIndex; }}
                  onkeydown={(e) => handleUnitKeydown(e, idx, unitIndex, notification.amount, u.value)}
                  class="flex w-full items-center px-3.5 py-1.5 text-left text-[0.8rem]
                    {notification.unit === u.value
                      ? 'bg-black/5 dark:bg-black/15 text-foreground'
                      : 'text-foreground hover:bg-black/5 dark:hover:bg-black/15'}">
                  {u.label}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
      {#if customNotifications.length < 2}
        <button onclick={addCustomNotification}
          class="flex items-center gap-1.5 self-start rounded-none px-2.5 py-1 text-[0.8rem] text-muted-foreground hover:bg-black/5 hover:text-foreground dark:hover:bg-black/15">
          <Plus size={13} /> <span>{t("calendar.notifications.custom")}</span>
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .num-input {
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .num-input::-webkit-inner-spin-button,
  .num-input::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
</style>
