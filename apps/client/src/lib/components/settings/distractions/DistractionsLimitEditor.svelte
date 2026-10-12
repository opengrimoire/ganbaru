<script lang="ts">
  import { tick } from "svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import Eraser from "@lucide/svelte/icons/eraser";
  import Save from "@lucide/svelte/icons/save";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import EventColorPicker from "$lib/components/calendar/EventColorPicker.svelte";
  import { FALLBACK_COLOR_INDEX, type EventColor } from "$lib/calendar/types";
  import { EVENT_COLOR_OPTIONS } from "$lib/calendar/utils";
  import {
    isProtectedDistractionsDesktopAppName,
    normalizeDistractionsAppName,
    type DistractionsUsageLimit,
  } from "$lib/distractions";
  import {
    getDistractions,
    type DistractionsUsageLimitEntryDraft,
  } from "$lib/stores/distractions.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { getDistractionsUsage } from "$lib/stores/distractions-usage.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import DistractionsAppSelector, {
    type DistractionsAppSelection,
  } from "$lib/components/settings/distractions/DistractionsAppSelector.svelte";
  import DistractionsMobileAppSelector, {
    type DistractionsMobileAppSelection,
  } from "./DistractionsMobileAppSelector.svelte";
  import type { DistractionsLimitEditorTarget } from "$lib/settings/types";

  type DailyBudgetOption = "none" | "15" | "30" | "45" | "60" | "90" | "120" | "180" | "240" | "custom";
  type WeeklyBudgetOption =
    | "none"
    | "60"
    | "120"
    | "180"
    | "300"
    | "420"
    | "600"
    | "840"
    | "1260"
    | "1680"
    | "custom";

  let {
    target,
    onDone,
    onCancel,
    compactLayout = false,
    iconRailLayout = false,
    onScrollContainerChange = () => {},
    onScrollbarInsetsChange = () => {},
  }: {
    target: DistractionsLimitEditorTarget;
    onDone: () => void;
    onCancel: () => void;
    compactLayout?: boolean;
    iconRailLayout?: boolean;
    onScrollContainerChange?: (scrollContainer: HTMLElement | undefined) => void;
    onScrollbarInsetsChange?: (insets: { top: number; bottom: number }) => void;
  } = $props();

  const distractions = getDistractions();
  const usage = getDistractionsUsage();
  const theme = getTheme();
  const { t } = getLocalization();
  const isAndroid = __GANBARU_AI_BUILD_PLATFORM__ === "android";
  const SOURCE_COLOR_GRID_COLUMNS = 4;
  const SOURCE_COLOR_COLUMN_ORDER = [0, 3, 1, 2] as const;
  const SOURCE_COLOR_PICK_ORDER = createSourceColorPickOrder();

  function budgetDurationLabel(minutes: number): string {
    if (minutes < 60) return t("settings.distractions.limits.editor.minutes", minutes);
    const hours = Math.floor(minutes / 60);
    const remainingMinutes = minutes % 60;
    if (remainingMinutes === 0) return t("settings.distractions.limits.editor.hours", hours);
    return t("settings.distractions.limits.editor.hoursMinutes", hours, remainingMinutes);
  }

  const dailyBudgetOptions = $derived.by<readonly { value: DailyBudgetOption; label: string }[]>(() => [
    { value: "none", label: t("settings.distractions.limits.editor.none") },
    { value: "15", label: budgetDurationLabel(15) },
    { value: "30", label: budgetDurationLabel(30) },
    { value: "45", label: budgetDurationLabel(45) },
    { value: "60", label: budgetDurationLabel(60) },
    { value: "90", label: budgetDurationLabel(90) },
    { value: "120", label: budgetDurationLabel(120) },
    { value: "180", label: budgetDurationLabel(180) },
    { value: "240", label: budgetDurationLabel(240) },
    { value: "custom", label: t("settings.distractions.limits.editor.custom") },
  ]);
  const weeklyBudgetOptions = $derived.by<readonly { value: WeeklyBudgetOption; label: string }[]>(() => [
    { value: "none", label: t("settings.distractions.limits.editor.none") },
    { value: "60", label: budgetDurationLabel(60) },
    { value: "120", label: budgetDurationLabel(120) },
    { value: "300", label: budgetDurationLabel(300) },
    { value: "420", label: budgetDurationLabel(420) },
    { value: "600", label: budgetDurationLabel(600) },
    { value: "840", label: budgetDurationLabel(840) },
    { value: "1260", label: budgetDurationLabel(1260) },
    { value: "1680", label: budgetDurationLabel(1680) },
    { value: "custom", label: t("settings.distractions.limits.editor.custom") },
  ]);
  const dailyBudgetPresetValues = new Set<DailyBudgetOption>([
    "none",
    "15",
    "30",
    "45",
    "60",
    "90",
    "120",
    "180",
    "240",
  ]);
  const weeklyBudgetPresetValues = new Set<WeeklyBudgetOption>([
    "none",
    "60",
    "120",
    "180",
    "300",
    "420",
    "600",
    "840",
    "1260",
    "1680",
  ]);
  let draftName = $state("");
  let draftDailyBudgetMode = $state<DailyBudgetOption>("60");
  let draftDailyCustomMinutes = $state("60");
  let draftWeeklyBudgetMode = $state<WeeklyBudgetOption>("none");
  let draftWeeklyCustomHours = $state("5");
  let draftEntries = $state<DistractionsUsageLimitEntryDraft[]>([]);
  let formError = $state("");
  let desktopAppPickerEntryId = $state<string | null>(null);
  let mobileAppPickerEntryId = $state<string | null>(null);
  let pendingDeleteEntryId = $state<string | null>(null);
  let editorRootEl: HTMLElement | undefined = $state();
  let editorScrollEl: HTMLElement | undefined = $state();
  let hydratedKey = "";
  const contentPaddingX = $derived(compactLayout ? "0.75rem" : iconRailLayout ? "1.25rem" : "2rem");
  const contentPaddingTop = $derived(compactLayout ? "1rem" : iconRailLayout ? "1.25rem" : "2rem");
  const contentPaddingBottom = $derived(compactLayout ? "1rem" : iconRailLayout ? "1.25rem" : "2rem");
  const limitNameWarning = $derived(limitNameRequirementMessage());

  const targetLimit = $derived.by<DistractionsUsageLimit | null>(() => {
    if (target.mode !== "edit") return null;
    return distractions.usageLimits.find((limit) => limit.id === target.limitId) ?? null;
  });

  function dailyBudgetModeForMinutes(minutes: number | null): DailyBudgetOption {
    if (minutes === null) return "none";
    const value = String(minutes) as DailyBudgetOption;
    return dailyBudgetPresetValues.has(value) ? value : "custom";
  }

  function weeklyBudgetModeForMinutes(minutes: number | null | undefined): WeeklyBudgetOption {
    if (minutes === null || minutes === undefined) return "none";
    const value = String(minutes) as WeeklyBudgetOption;
    return weeklyBudgetPresetValues.has(value) ? value : "custom";
  }

  function setDraftDailyBudgetMode(value: string): void {
    const next = dailyBudgetOptions.find((option) => option.value === value)?.value ?? "custom";
    draftDailyBudgetMode = next;
    if (next !== "custom" && next !== "none") {
      draftDailyCustomMinutes = next;
      formError = "";
    }
  }

  function setDraftWeeklyBudgetMode(value: string): void {
    const next = weeklyBudgetOptions.find((option) => option.value === value)?.value ?? "custom";
    draftWeeklyBudgetMode = next;
    if (next !== "custom" && next !== "none") {
      draftWeeklyCustomHours = String(Number.parseInt(next, 10) / 60);
      formError = "";
    }
  }

  function parseDraftDailyMinutes(): number | null | "invalid" {
    if (draftDailyBudgetMode === "none") return null;
    const raw = draftDailyBudgetMode === "custom" ? draftDailyCustomMinutes : draftDailyBudgetMode;
    if (!/^\d+$/.test(raw.trim())) return "invalid";
    const minutes = Number.parseInt(raw, 10);
    return minutes >= 1 && minutes <= 1440 ? minutes : "invalid";
  }

  function parseDraftWeeklyMinutes(): number | null | "invalid" {
    if (draftWeeklyBudgetMode === "none") return null;
    if (draftWeeklyBudgetMode !== "custom") {
      const minutes = Number.parseInt(draftWeeklyBudgetMode, 10);
      return minutes >= 1 && minutes <= 7 * 24 * 60 ? minutes : "invalid";
    }
    const raw = draftWeeklyCustomHours.trim();
    if (!/^(?:\d+|\d+\.\d|\.\d)$/.test(raw)) return "invalid";
    const hours = Number.parseFloat(raw);
    if (!Number.isFinite(hours) || hours < 0.1 || hours > 168) return "invalid";
    return Math.round(hours * 60);
  }

  function createEntryId(): string {
    const suffix = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
      ? crypto.randomUUID().replace(/-/g, "").slice(0, 12)
      : `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
    return `entry-${suffix}`;
  }

  function createSourceColorPickOrder(): readonly EventColor[] {
    const rows = Math.ceil(EVENT_COLOR_OPTIONS.length / SOURCE_COLOR_GRID_COLUMNS);
    const colors: EventColor[] = [];
    for (const column of SOURCE_COLOR_COLUMN_ORDER) {
      for (let row = 0; row < rows; row += 1) {
        const color = EVENT_COLOR_OPTIONS[(row * SOURCE_COLOR_GRID_COLUMNS) + column];
        if (typeof color === "number") colors.push(color);
      }
    }
    return colors;
  }

  function entryColorForIndex(index: number): EventColor {
    return SOURCE_COLOR_PICK_ORDER[index % SOURCE_COLOR_PICK_ORDER.length] ?? FALLBACK_COLOR_INDEX;
  }

  function nextEntryColor(): EventColor {
    const usedColors = new Set(
      draftEntries
        .map((entry) => entry.color)
        .filter((color): color is EventColor => typeof color === "number"),
    );
    return SOURCE_COLOR_PICK_ORDER.find((color) => !usedColors.has(color))
      ?? entryColorForIndex(draftEntries.length);
  }

  function createEntryDraft(color: EventColor = nextEntryColor()): DistractionsUsageLimitEntryDraft {
    return {
      id: createEntryId(),
      name: "",
      color,
      websiteHost: "",
      mobileAppName: "",
      mobileAppPackage: "",
      desktopAppName: "",
      desktopAppMatchNames: [],
    };
  }

  function entryHasAnySource(entry: DistractionsUsageLimitEntryDraft): boolean {
    return entry.websiteHost.trim() !== ""
      || entry.mobileAppName.trim() !== ""
      || entry.desktopAppName.trim() !== "";
  }

  function entryHasAnyField(entry: DistractionsUsageLimitEntryDraft): boolean {
    return entry.name.trim() !== "" || entryHasAnySource(entry);
  }

  function activeEntries(): DistractionsUsageLimitEntryDraft[] {
    return draftEntries.filter(entryHasAnyField);
  }

  function limitNameRequirementMessage(): string {
    if (activeEntries().length <= 1 || draftName.trim() !== "") return "";
    return t("settings.distractions.limits.editor.addLimitNameForMultipleSources");
  }

  function hydrateDraft(): void {
    formError = "";
    if (target.mode === "create") {
      draftName = "";
      draftDailyBudgetMode = "60";
      draftDailyCustomMinutes = "60";
      draftWeeklyBudgetMode = "none";
      draftWeeklyCustomHours = "5";
      draftEntries = [createEntryDraft(entryColorForIndex(0))];
      return;
    }
    const limit = targetLimit;
    if (!limit) {
      draftName = "";
      draftDailyBudgetMode = "60";
      draftDailyCustomMinutes = "60";
      draftWeeklyBudgetMode = "none";
      draftWeeklyCustomHours = "5";
      draftEntries = [createEntryDraft(entryColorForIndex(0))];
      formError = t("settings.distractions.limits.editor.limitMissing");
      return;
    }
    draftName = limit.name;
    draftDailyBudgetMode = dailyBudgetModeForMinutes(limit.minutesPerDay);
    draftDailyCustomMinutes = String(limit.minutesPerDay ?? 60);
    draftWeeklyBudgetMode = weeklyBudgetModeForMinutes(limit.minutesPerWeek);
    draftWeeklyCustomHours = String((limit.minutesPerWeek ?? 300) / 60);
    draftEntries = limit.entries.map((entry, index) => ({
      id: entry.id,
      name: entry.name ?? "",
      color: entry.color ?? entryColorForIndex(index),
      websiteHost: entry.websiteHost ?? "",
      mobileAppName: entry.mobileAppName ?? "",
      mobileAppPackage: entry.mobileAppPackage ?? "",
      desktopAppName: entry.desktopAppName ?? "",
      desktopAppMatchNames: entry.desktopAppMatchNames,
    }));
    if (draftEntries.length === 0) draftEntries = [createEntryDraft()];
  }

  $effect.pre(() => {
    const key = target.mode === "edit" ? `edit:${target.limitId}` : "create";
    if (key === hydratedKey) return;
    hydratedKey = key;
    hydrateDraft();
  });

  $effect(() => {
    onScrollContainerChange(editorScrollEl);
    reportScrollbarInsets();

    const contentEl = editorRootEl?.closest<HTMLElement>("[data-settings-content]");
    if (!editorRootEl || !editorScrollEl || !contentEl) {
      return () => {
        onScrollContainerChange(undefined);
        onScrollbarInsetsChange({ top: 0, bottom: 0 });
      };
    }

    const observer = new ResizeObserver(reportScrollbarInsets);
    observer.observe(editorRootEl);
    observer.observe(editorScrollEl);
    observer.observe(contentEl);
    window.addEventListener("resize", reportScrollbarInsets);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", reportScrollbarInsets);
      onScrollContainerChange(undefined);
      onScrollbarInsetsChange({ top: 0, bottom: 0 });
    };
  });

  function reportScrollbarInsets(): void {
    const contentEl = editorRootEl?.closest<HTMLElement>("[data-settings-content]");
    if (!editorScrollEl || !contentEl) {
      onScrollbarInsetsChange({ top: 0, bottom: 0 });
      return;
    }
    const contentRect = contentEl.getBoundingClientRect();
    const scrollRect = editorScrollEl.getBoundingClientRect();
    onScrollbarInsetsChange({
      top: Math.max(0, scrollRect.top - contentRect.top),
      bottom: Math.max(0, contentRect.bottom - scrollRect.bottom),
    });
  }

  async function addEntry(): Promise<void> {
    draftEntries = [...draftEntries, createEntryDraft()];
    formError = "";
    await tick();
    editorScrollEl?.scrollTo({
      top: editorScrollEl.scrollHeight,
      behavior: "smooth",
    });
  }

  function updateEntry(
    id: string,
    field: keyof Omit<DistractionsUsageLimitEntryDraft, "id" | "color" | "desktopAppMatchNames">,
    value: string,
  ): void {
    draftEntries = draftEntries.map((entry) =>
      entry.id === id ? { ...entry, [field]: value } : entry
    );
    formError = "";
  }

  function updateEntryColor(id: string, color: EventColor | undefined): void {
    draftEntries = draftEntries.map((entry) =>
      entry.id === id ? { ...entry, color: color ?? null } : entry
    );
    formError = "";
  }

  function removeEntryImmediately(id: string): void {
    draftEntries = draftEntries.filter((entry) => entry.id !== id);
    if (draftEntries.length === 0) draftEntries = [createEntryDraft()];
    if (desktopAppPickerEntryId === id) desktopAppPickerEntryId = null;
    if (mobileAppPickerEntryId === id) mobileAppPickerEntryId = null;
    if (pendingDeleteEntryId === id) pendingDeleteEntryId = null;
    formError = "";
  }

  function requestRemoveEntry(entry: DistractionsUsageLimitEntryDraft): void {
    if (!entryHasAnyField(entry)) {
      removeEntryImmediately(entry.id);
      return;
    }
    pendingDeleteEntryId = entry.id;
  }

  function confirmDeleteEntry(): void {
    if (!pendingDeleteEntryId) return;
    removeEntryImmediately(pendingDeleteEntryId);
  }

  function cancelDeleteEntry(): void {
    pendingDeleteEntryId = null;
  }

  function openDesktopAppPicker(id: string): void {
    desktopAppPickerEntryId = id;
    formError = "";
  }

  function openMobileAppPicker(id: string): void {
    mobileAppPickerEntryId = id;
    formError = "";
  }

  function closeMobileAppPicker(): void {
    mobileAppPickerEntryId = null;
  }

  function existingMobileAppPackages(): string[] {
    return draftEntries
      .filter((entry) => entry.id !== mobileAppPickerEntryId)
      .map((entry) => entry.mobileAppPackage.trim())
      .filter(Boolean);
  }

  function chooseMobileApp(app: DistractionsMobileAppSelection): void {
    const activeId = mobileAppPickerEntryId;
    if (!activeId) return;
    draftEntries = draftEntries.map((entry) => entry.id === activeId
      ? { ...entry, mobileAppName: app.name, mobileAppPackage: app.packageName }
      : entry);
    mobileAppPickerEntryId = null;
    formError = "";
  }

  function clearMobileApp(id: string): void {
    draftEntries = draftEntries.map((entry) => entry.id === id
      ? { ...entry, mobileAppName: "", mobileAppPackage: "" }
      : entry);
    formError = "";
  }

  function closeDesktopAppPicker(): void {
    desktopAppPickerEntryId = null;
  }

  function existingDesktopAppPickerNames(): string[] {
    const activeId = desktopAppPickerEntryId;
    if (!activeId) return [];
    return draftEntries
      .filter((entry) => entry.id !== activeId)
      .flatMap((entry) => [
        entry.desktopAppName,
        ...entry.desktopAppMatchNames,
      ])
      .map(normalizeDistractionsAppName)
      .filter((name): name is string => Boolean(name));
  }

  function chooseDesktopApp(app: DistractionsAppSelection): void {
    const activeId = desktopAppPickerEntryId;
    if (!activeId) return;
    draftEntries = draftEntries.map((entry) =>
      entry.id === activeId
        ? {
          ...entry,
          desktopAppName: app.name,
          desktopAppMatchNames: app.matchNames,
        }
        : entry
    );
    desktopAppPickerEntryId = null;
    formError = "";
  }

  function clearDesktopApp(id: string): void {
    draftEntries = draftEntries.map((entry) =>
      entry.id === id
        ? {
          ...entry,
          desktopAppName: "",
          desktopAppMatchNames: [],
        }
        : entry
    );
    if (desktopAppPickerEntryId === id) desktopAppPickerEntryId = null;
    formError = "";
  }

  function validatedEntries(): DistractionsUsageLimitEntryDraft[] | null {
    const entries = activeEntries();
    if (entries.length === 0) {
      formError = t("settings.distractions.limits.editor.addOneSource");
      return null;
    }
    if (entries.some((entry) => !entryHasAnySource(entry))) {
      formError = t("settings.distractions.limits.editor.sourceNeedsTarget");
      return null;
    }
    for (const entry of entries) {
      const desktopName = entry.desktopAppName.trim()
        ? normalizeDistractionsAppName(entry.desktopAppName)
        : null;
      if (desktopName && isProtectedDistractionsDesktopAppName(desktopName)) {
        formError = t("settings.distractions.limits.editor.protectedDesktopApps");
        return null;
      }
    }
    return entries;
  }

  function saveLimit(): void {
    formError = "";
    const minutesPerDay = parseDraftDailyMinutes();
    if (minutesPerDay === "invalid") {
      formError = t("settings.distractions.limits.editor.dailyBudgetInvalid");
      return;
    }
    const minutesPerWeek = parseDraftWeeklyMinutes();
    if (minutesPerWeek === "invalid") {
      formError = t("settings.distractions.limits.editor.weeklyBudgetInvalid");
      return;
    }
    if (minutesPerDay === null && minutesPerWeek === null) {
      formError = t("settings.distractions.limits.editor.chooseBudget");
      return;
    }
    const entries = validatedEntries();
    if (!entries) return;
    if (entries.length > 1 && draftName.trim() === "") {
      formError = "";
      return;
    }
    const draft = {
      name: draftName,
      minutesPerDay,
      minutesPerWeek,
      entries,
    };
    const result = target.mode === "edit"
      ? distractions.updateUsageLimit(target.limitId, draft)
      : distractions.addUsageLimit(draft);
    if (result === "saved") {
      void usage.refresh();
      onDone();
      return;
    }
    const messages = {
      "invalid-name": t("settings.distractions.limits.editor.enterLimitName"),
      "invalid-minutes": t("settings.distractions.limits.editor.validBudget"),
      "invalid-budget": t("settings.distractions.limits.editor.chooseBudget"),
      "invalid-sources": t("settings.distractions.limits.editor.validSources"),
      "duplicate-source": t("settings.distractions.limits.editor.duplicateSource"),
      "protected-source": t("settings.distractions.limits.editor.protectedDesktopApps"),
      missing: t("settings.distractions.limits.editor.limitMissing"),
    } satisfies Record<Exclude<typeof result, "saved">, string>;
    formError = messages[result];
  }
</script>

<div bind:this={editorRootEl} class="flex h-full min-h-0 flex-col">
  <main
    bind:this={editorScrollEl}
    class="hide-scrollbar min-h-0 flex-1 overflow-y-auto"
  >
    <header
      class="shrink-0"
      style="padding-left: {contentPaddingX}; padding-right: {contentPaddingX}; padding-top: {contentPaddingTop};"
    >
      <div class="flex min-w-0 items-start justify-between gap-3 border-b border-border/70 pb-4">
        <div class="min-w-0">
          <h1 class="truncate text-[1rem] font-semibold text-foreground">
            {target.mode === "edit"
              ? t("settings.distractions.limits.editor.editTitle")
              : t("settings.distractions.limits.editor.addTitle")}
          </h1>
          <p class="mt-1 max-w-2xl text-[0.866667rem] text-muted-foreground">
            {t("settings.distractions.limits.editor.intro")}
          </p>
        </div>
      </div>
    </header>

    <div
      class="flex flex-col gap-6 py-6"
      style="padding-left: {contentPaddingX}; padding-right: {contentPaddingX};"
    >
      <section class="flex flex-col gap-4">
        <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.distractions.limits.editor.limitDetails")}</h2>

        <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
          <div class="min-w-0 flex-1">
            <label for="distractions-limit-name" class="text-[0.866667rem] text-foreground">{t("settings.distractions.limits.editor.limitName")}</label>
            <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.distractions.limits.editor.limitNameDescription")}</div>
          </div>
          <div class="flex w-72 max-w-full flex-col gap-1 max-[480px]:w-full">
            <input
              id="distractions-limit-name"
              bind:value={draftName}
              oninput={() => {
                formError = "";
              }}
              class="field w-full px-2.5 text-[0.8rem] text-foreground"
              aria-invalid={limitNameWarning ? "true" : "false"}
              aria-describedby={limitNameWarning ? "distractions-limit-name-warning" : undefined}
              placeholder={t("settings.distractions.limits.editor.limitNamePlaceholder")}
            />
            {#if limitNameWarning}
              <div id="distractions-limit-name-warning" class="text-[0.733333rem] text-destructive">
                {limitNameWarning}
              </div>
            {/if}
          </div>
        </div>

        <Select
          label={t("settings.distractions.limits.editor.dailyBudget")}
          description={t("settings.distractions.limits.editor.dailyBudgetDescription")}
          value={draftDailyBudgetMode}
          options={dailyBudgetOptions}
          onChange={setDraftDailyBudgetMode}
          class="w-72 max-[480px]:w-full"
        />

        {#if draftDailyBudgetMode === "custom"}
          <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
            <div class="min-w-0 flex-1">
              <label for="distractions-limit-custom-minutes" class="text-[0.866667rem] text-foreground">{t("settings.distractions.limits.editor.customMinutes")}</label>
              <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.distractions.limits.editor.customMinutesDescription")}</div>
            </div>
            <input
              id="distractions-limit-custom-minutes"
              bind:value={draftDailyCustomMinutes}
              oninput={() => {
                formError = "";
              }}
              type="text"
              inputmode="numeric"
              pattern="[0-9]*"
              class="field w-28 max-w-full px-2.5 text-[0.8rem] text-foreground max-[480px]:w-full"
              placeholder="75"
            />
          </div>
        {/if}

        <Select
          label={t("settings.distractions.limits.editor.weeklyBudget")}
          description={t("settings.distractions.limits.editor.weeklyBudgetDescription")}
          value={draftWeeklyBudgetMode}
          options={weeklyBudgetOptions}
          onChange={setDraftWeeklyBudgetMode}
          class="w-72 max-[480px]:w-full"
        />

        {#if draftWeeklyBudgetMode === "custom"}
          <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
            <div class="min-w-0 flex-1">
              <label for="distractions-limit-custom-weekly-hours" class="text-[0.866667rem] text-foreground">{t("settings.distractions.limits.editor.customWeeklyHours")}</label>
              <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.distractions.limits.editor.customWeeklyHoursDescription")}</div>
            </div>
            <input
              id="distractions-limit-custom-weekly-hours"
              bind:value={draftWeeklyCustomHours}
              oninput={() => {
                formError = "";
              }}
              type="text"
              inputmode="decimal"
              class="field w-28 max-w-full px-2.5 text-[0.8rem] text-foreground max-[480px]:w-full"
              placeholder="5.5"
            />
          </div>
        {/if}
      </section>

      <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

      <section class="flex flex-col gap-4">
        <div class="min-w-0 px-1">
          <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.distractions.limits.editor.linkedSources")}</h2>
          <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
            {t("settings.distractions.limits.editor.linkedSourcesDescription")}
          </div>
        </div>

        <div class="flex flex-col gap-3 px-1">
          {#each draftEntries as entry (entry.id)}
            <div class="flex min-w-0 flex-col gap-3 rounded-md border border-border bg-card/35 p-3 dark:bg-transparent">
              <div class="flex min-w-0 items-center gap-2">
                <input
                  value={entry.name}
                  oninput={(event) => updateEntry(entry.id, "name", event.currentTarget.value)}
                  aria-label={t("settings.distractions.limits.editor.sourceName")}
                  class="field h-8 min-w-0 flex-1 px-2.5 text-[0.8rem] text-foreground"
                  placeholder={t("settings.distractions.limits.editor.sourceNamePlaceholder")}
                />
                <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-border bg-card text-muted-foreground transition-colors hover:bg-accent hover:text-foreground dark:bg-transparent">
                  <EventColorPicker
                    color={entry.color ?? undefined}
                    theme={theme.current}
                    ariaLabel={t("settings.distractions.limits.editor.selectSourceColor")}
                    onSelect={(color) => updateEntryColor(entry.id, color)}
                  />
                </div>
                <button
                  type="button"
                  onclick={() => requestRemoveEntry(entry)}
                  aria-label={t("settings.distractions.limits.editor.removeLinkedSourceRow")}
                  data-app-tooltip-disabled="true"
                  class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-border bg-card text-muted-foreground transition-colors hover:bg-accent hover:text-foreground dark:bg-transparent"
                >
                  <Trash2 size={13} strokeWidth={2.25} />
                </button>
              </div>

              <div class="grid min-w-0 grid-cols-1 gap-2 min-[680px]:grid-cols-3">
                <label class="flex min-w-0 flex-col gap-1">
                  <span class="text-[0.733333rem] font-medium text-muted-foreground">{t("settings.distractions.limits.editor.website")}</span>
                  <input
                    value={entry.websiteHost}
                    disabled={isAndroid}
                    oninput={(event) => updateEntry(entry.id, "websiteHost", event.currentTarget.value)}
                    class="field h-8 min-w-0 px-2.5 text-[0.8rem] text-foreground"
                    placeholder="domain.com"
                  />
                </label>

                <div class="flex min-w-0 flex-col gap-1">
                  <span class="text-[0.733333rem] font-medium text-muted-foreground">{t("settings.distractions.limits.editor.mobile")}</span>
                  {#if isAndroid}
                    <div class="field flex h-8 min-w-0 items-center gap-1 px-1.5">
                      <button type="button" onclick={() => openMobileAppPicker(entry.id)} class={["min-w-0 flex-1 truncate rounded-sm px-1.5 py-1 text-left text-[0.8rem] outline-none hover:bg-accent", entry.mobileAppName ? "text-foreground" : "text-muted-foreground"]}>
                        {entry.mobileAppName || t("settings.distractions.limits.editor.chooseApp")}
                      </button>
                      {#if entry.mobileAppName}
                        <button type="button" onclick={() => clearMobileApp(entry.id)} aria-label={t("settings.distractions.limits.editor.clearMobileApp")} class="flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent hover:text-foreground"><Eraser size={12} /></button>
                      {/if}
                    </div>
                    {#if entry.mobileAppName && !entry.mobileAppPackage}
                      <span class="text-[0.7rem] text-destructive">{t("settings.distractions.mobile.reselectForAndroid")}</span>
                    {/if}
                  {:else}
                    <input value={entry.mobileAppName} oninput={(event) => updateEntry(entry.id, "mobileAppName", event.currentTarget.value)} class="field h-8 min-w-0 px-2.5 text-[0.8rem] text-foreground" placeholder={t("settings.distractions.limits.editor.mobilePlaceholder")} />
                  {/if}
                </div>

                <div class="flex min-w-0 flex-col gap-1">
                  <span class="text-[0.733333rem] font-medium text-muted-foreground">{t("settings.distractions.limits.editor.desktop")}</span>
                  <div class="field flex h-8 min-w-0 items-center gap-1 px-1.5">
                    <button
                      type="button"
                      onclick={() => openDesktopAppPicker(entry.id)}
                      disabled={isAndroid}
                      class={[
                        "min-w-0 flex-1 truncate rounded-sm px-1.5 py-1 text-left text-[0.8rem] outline-none transition-colors hover:bg-accent focus-visible:bg-accent",
                        entry.desktopAppName ? "text-foreground" : "text-muted-foreground",
                      ]}
                    >
                      {entry.desktopAppName || t("settings.distractions.limits.editor.chooseApp")}
                    </button>
                    {#if entry.desktopAppName}
                      <button
                        type="button"
                        disabled={isAndroid}
                        onclick={() => clearDesktopApp(entry.id)}
                        aria-label={t("settings.distractions.limits.editor.clearDesktopApp")}
                        class="flex h-6 w-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                      >
                        <Eraser size={12} strokeWidth={2.25} />
                      </button>
                    {/if}
                  </div>
                </div>
              </div>
            </div>
          {/each}
        </div>

        <div class="flex justify-end px-1">
          <button
            type="button"
            onclick={addEntry}
            class="flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent dark:bg-transparent"
          >
            <Plus size={13} strokeWidth={2.25} />
            <span>{t("settings.distractions.limits.editor.addSource")}</span>
          </button>
        </div>
      </section>

    </div>
  </main>

  <footer
    class="shrink-0"
    style="padding-left: {contentPaddingX}; padding-right: {contentPaddingX}; padding-bottom: {contentPaddingBottom};"
  >
    <div class="flex flex-wrap items-center justify-between gap-2 border-t border-border/70 pt-3">
      <div class="min-w-0 flex-1 text-[0.8rem] text-destructive">
        {formError}
      </div>
      <div class="flex shrink-0 flex-wrap justify-end gap-2">
        <button
          type="button"
          onclick={onCancel}
          class="flex h-8 items-center justify-center rounded-md border border-border bg-card px-3 text-[0.8rem] text-foreground transition-colors hover:bg-accent dark:bg-transparent"
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          onclick={saveLimit}
          class="flex h-8 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90"
        >
          <Save size={13} strokeWidth={2.25} />
          <span>{t("common.save")}</span>
        </button>
      </div>
    </div>
  </footer>
</div>

{#if mobileAppPickerEntryId}
  <DistractionsMobileAppSelector
    title={t("settings.distractions.mobile.chooseAppToBlock")}
    single
    existingPackages={existingMobileAppPackages()}
    onSelect={chooseMobileApp}
    onCancel={closeMobileAppPicker}
  />
{/if}

{#if desktopAppPickerEntryId}
  <DistractionsAppSelector
    title={t("settings.distractions.limits.editor.chooseDesktopApp")}
    mode="single"
    existingNames={existingDesktopAppPickerNames()}
    protectAppSelf
    onSelect={chooseDesktopApp}
    onCancel={closeDesktopAppPicker}
  />
{/if}

{#if pendingDeleteEntryId}
  <ConfirmDialog
    title={t("settings.distractions.limits.editor.deleteLinkedSourceTitle")}
    message={t("settings.distractions.limits.editor.deleteLinkedSourceMessage")}
    confirmLabel={t("settings.distractions.shared.deleteAction")}
    cancelLabel={t("settings.distractions.shared.cancelAction")}
    onConfirm={confirmDeleteEntry}
    onCancel={cancelDeleteEntry}
  />
{/if}
