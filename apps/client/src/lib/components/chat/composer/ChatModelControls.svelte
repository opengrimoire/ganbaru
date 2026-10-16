<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Plus from "@lucide/svelte/icons/plus";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Search from "@lucide/svelte/icons/search";
  import Star from "@lucide/svelte/icons/star";
  import Zap from "@lucide/svelte/icons/zap";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import type {
    ModelOptionSelection,
    ModelOptionValue,
    ProviderInstanceRead,
    ProviderModel,
  } from "$lib/chat/contracts";
  import {
    composerModelSelection,
    defaultModelOptions,
    isProviderAvailable,
    readComposerModelSelection,
  } from "$lib/chat/composer/model";
  import { formatModelDisplayName, modelCompany } from "$lib/chat/composer/model-company";
  import {
    buildFavoriteModelEntries,
    buildModelCompanySections,
    buildQuickEffortChoices,
    compactModelName,
    compactModelOptionLabel,
    isKnownModelOption,
    modelEffortStopPosition,
    modelOptionRole,
    visibleProviderModels,
    type KnownModelOption,
    type QuickEffortChoice,
  } from "$lib/chat/composer/model-picker-model";
  import * as chatApi from "$lib/api/chat";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { trapTabFocus } from "$lib/chat/focus-navigation";
  import { isPointerAimingAtSubmenu, SUBMENU_AIM_TOLERANCES } from "$lib/utils/menu-aim";
  import { portal } from "$lib/utils/portal";
  import { keepInViewport } from "$lib/utils/keep-in-viewport";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import ChatProviderIcon from "$lib/components/chat/identity/ChatProviderIcon.svelte";
  import ChatProviderForkDialog from "./ChatProviderForkDialog.svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { cn } from "$lib/utils";

  interface PendingProviderModelSelection {
    providerInstanceId: string;
    modelId: string | null;
    providerManaged: boolean;
  }
  interface FlyoutPosition {
    left: number;
    top: number;
  }
  interface PickerRect {
    top: number;
    right: number;
    bottom: number;
    left: number;
  }
  interface ChatModelControlSelection {
    providerInstanceId: string | null;
    modelId: string | null;
    providerManaged: boolean;
    options: ModelOptionSelection[];
  }
  type PickerView = "overview" | "advanced";
  type FlyoutView = "models" | "option";

  let {
    value,
    disabled = false,
    onChange,
  }: {
    value?: ChatModelControlSelection;
    disabled?: boolean;
    onChange?: (selection: ChatModelControlSelection) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = useChat();
  const settings = getSettingsLauncher();
  let pickerOpen = $state(false);
  let pickerWasOpen = false;
  let pickerRoot: HTMLDivElement | undefined = $state();
  let pickerPanel: HTMLDivElement | undefined = $state();
  let pickerTrigger: HTMLButtonElement | undefined = $state();
  let overviewPanel: HTMLDivElement | undefined = $state();
  let advancedPanel: HTMLDivElement | undefined = $state();
  let advancedToggle: HTMLButtonElement | undefined = $state();
  let advancedHeading: HTMLButtonElement | undefined = $state();
  let flyoutPanel: HTMLDivElement | undefined = $state();
  let pickerStageHeight = $state<number | null>(null);
  let pickerPosition = $state<FlyoutPosition | null>(null);
  let pickerLayoutReady = $state(false);
  let pickerPlacement = $state<"above" | "below">("above");
  let effortPressing = $state(false);
  let effortDragging = $state(false);
  let effortHandleHovered = $state(false);
  let modelSearch: HTMLInputElement | undefined = $state();
  let modelListElement: HTMLDivElement | undefined = $state();
  let providerForkDialog: HTMLDivElement | undefined = $state();
  let pendingProviderModel = $state<PendingProviderModelSelection | null>(null);
  let modelQuery = $state("");
  let modelPickerError = $state<string | null>(null);
  let collapsedModelSections = $state<Set<string>>(new Set());
  let view = $state<PickerView>("overview");
  let flyout = $state<FlyoutView | null>(null);
  let flyoutPosition = $state<FlyoutPosition | null>(null);
  let optionViewKey = $state<string | null>(null);
  let quickAnchorModelId = $state<string | null>(null);
  let activeFlyoutTrigger: HTMLElement | null = null;
  let effortClickResetTimer: ReturnType<typeof setTimeout> | null = null;
  let effortPointerId: number | null = null;
  let effortPointerOrigin: { x: number; y: number } | null = null;
  let effortLastDragIndex = -1;
  let suppressEffortPointerClick = false;
  const flyoutGapPx = 6;
  const flyoutViewportInsetPx = 8;
  const pickerGapPx = 7;
  const pickerBoundaryInsetPx = 8;
  const effortDragThresholdPx = 5;
  const effortTrackHeightRem = 1.75;
  const effortEndpointInsetRem = effortTrackHeightRem / 2;
  const dummyEffortStops = [0, 1, 2, 3, 4] as const;
  const controlled = $derived(value !== undefined);
  /** The composer popover is placed by CSS above its trigger, so nudge it back inside a narrow screen. */
  const keepComposerPopoverInViewport = (node: HTMLElement) => (controlled ? undefined : keepInViewport(node));
  const providers = $derived(chat.settings?.providerInstances ?? []);
  const providerInstanceId = $derived(value === undefined
    ? chat.composer.providerInstanceId
    : value.providerInstanceId);
  const provider = $derived(providers.find((entry) => entry.configuration.instanceId === providerInstanceId) ?? null);
  const unconfiguredFamilies = $derived((chat.settings?.providerFamilies ?? []).filter((family) => !providers.some((entry) => entry.configuration.familyId === family.familyId)));
  const selection = $derived(value === undefined
    ? readComposerModelSelection(chat.composer.modelSelection)
    : {
        modelId: value.modelId,
        providerManaged: value.providerManaged,
        options: value.options,
      });
  const catalogSelection = $derived({
    providerInstanceId: provider?.configuration.instanceId ?? null,
    modelId: selection.modelId,
    providerManaged: selection.providerManaged,
  });
  const models = $derived(visibleProviderModels(provider, catalogSelection));
  const favoriteModelEntries = $derived.by(() => buildFavoriteModelEntries(
    providers,
    chat.settings?.configuration.rememberedSelections ?? [],
    catalogSelection,
    modelQuery,
  ));
  const modelCompanySections = $derived.by(() => buildModelCompanySections(
    providers,
    unconfiguredFamilies,
    catalogSelection,
    modelQuery,
    t("chat.composer.providerManagedModel"),
    localization.locale,
  ));
  const selectedModel = $derived(models.find((model) => model.id === selection.modelId) ?? null);
  const quickAnchorModel = $derived(models.find((model) => model.id === quickAnchorModelId) ?? selectedModel);
  const knownOptions = $derived((selectedModel?.options ?? []).filter(isKnownModelOption));
  const effortDefinition = $derived(knownOptions.find((definition) => modelOptionRole(definition) === "effort") ?? null);
  const speedDefinition = $derived.by(() => {
    const candidates = [selectedModel, quickAnchorModel, ...models];
    const visited = new Set<string>();
    for (const candidate of candidates) {
      if (!candidate || visited.has(candidate.id)) continue;
      visited.add(candidate.id);
      for (const definition of candidate.options) {
        if (isKnownModelOption(definition) && modelOptionRole(definition) === "speed") return definition;
      }
    }
    return null;
  });
  const otherDefinitions = $derived(knownOptions.filter((definition) => modelOptionRole(definition) === "other"));
  const optionViewDefinition = $derived(knownOptions.find((definition) => definition.key === optionViewKey)
    ?? (speedDefinition?.key === optionViewKey ? speedDefinition : null));
  const quickEffortChoices = $derived(buildQuickEffortChoices(
    quickAnchorModel,
    models,
    provider?.configuration.familyId ?? null,
    displayModelName,
    humanizeOptionLabel,
  ));
  const selectedQuickEffortIndex = $derived(quickEffortChoices.findIndex((choice) => choice.modelId === selection.modelId && choice.effortValue === selectedEffortValue()));

  onDestroy(() => {
    cancelEffortClickReset();
  });

  $effect(() => {
    if (!pickerOpen && pickerWasOpen) queueMicrotask(() => pickerTrigger?.focus());
    pickerWasOpen = pickerOpen;
  });

  $effect(() => {
    if (!flyout || !flyoutPanel) return;
    const reposition = () => positionFlyout();
    reposition();
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    if (typeof ResizeObserver === "undefined") {
      return () => {
        window.removeEventListener("resize", reposition);
        window.removeEventListener("scroll", reposition, true);
      };
    }
    const observer = new ResizeObserver(reposition);
    observer.observe(flyoutPanel);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  });

  $effect(() => {
    if (!controlled || !pickerOpen || !pickerLayoutReady || !pickerPanel || !pickerTrigger) return;
    const reposition = () => positionPicker();
    reposition();
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    if (typeof ResizeObserver === "undefined") {
      return () => {
        window.removeEventListener("resize", reposition);
        window.removeEventListener("scroll", reposition, true);
      };
    }
    const observer = new ResizeObserver(reposition);
    observer.observe(pickerPanel);
    observer.observe(pickerTrigger);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  });

  $effect(() => {
    if (!pickerOpen) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (pickerRoot?.contains(event.target) || pickerPanel?.contains(event.target) || flyoutPanel?.contains(event.target) || providerForkDialog?.contains(event.target)) return;
      closePicker();
    };
    window.addEventListener("pointerdown", closeOnOutsidePointer, true);
    return () => window.removeEventListener("pointerdown", closeOnOutsidePointer, true);
  });

  $effect(() => {
    if (!pickerOpen) {
      pickerStageHeight = null;
      return;
    }
    const activePanel = view === "overview" ? overviewPanel : advancedPanel;
    if (!activePanel) return;
    const updateHeight = () => {
      const renderedHeight = activePanel.getBoundingClientRect().height;
      const nextHeight = view === "advanced"
        ? Math.max(activePanel.scrollHeight, renderedHeight)
        : renderedHeight;
      if (nextHeight > 0) pickerStageHeight = Math.ceil(nextHeight);
    };
    updateHeight();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(updateHeight);
    observer.observe(activePanel);
    return () => observer.disconnect();
  });

  function togglePicker(): void {
    if (pickerOpen) {
      closePicker();
      return;
    }
    const anchoredChoices = buildQuickEffortChoices(
      models.find((model) => model.id === quickAnchorModelId) ?? null,
      models,
      provider?.configuration.familyId ?? null,
      displayModelName,
      humanizeOptionLabel,
    );
    if (!anchoredChoices.some((choice) => choice.modelId === selection.modelId && choice.effortValue === selectedEffortValue())) {
      quickAnchorModelId = selectedModel?.id ?? null;
    }
    if (!provider) view = "advanced";
    flyout = provider ? null : "models";
    openPicker();
  }

  function closePicker(): void {
    pickerOpen = false;
    pickerPosition = null;
    pickerLayoutReady = false;
    modelQuery = "";
    modelPickerError = null;
    collapsedModelSections = new Set();
    cancelEffortClickReset();
    suppressEffortPointerClick = false;
    effortHandleHovered = false;
    resetEffortPointerState();
    closeFlyout();
  }

  function isModelSectionCollapsed(sectionId: string): boolean {
    return collapsedModelSections.has(sectionId);
  }

  function toggleModelSection(sectionId: string): void {
    const next = new Set(collapsedModelSections);
    if (next.has(sectionId)) next.delete(sectionId);
    else next.add(sectionId);
    collapsedModelSections = next;
  }

  function openPicker(): void {
    pickerPosition = null;
    pickerLayoutReady = false;
    pickerOpen = true;
    void tick().then(() => {
      pickerLayoutReady = true;
      positionPicker();
    });
  }

  function clamp(value: number, minimum: number, maximum: number): number {
    if (maximum < minimum) return minimum;
    return Math.min(Math.max(value, minimum), maximum);
  }

  function pickerBoundaryRect(): PickerRect {
    if (!pickerTrigger) {
      return { top: 0, right: window.innerWidth, bottom: window.innerHeight, left: 0 };
    }
    const boundaryElement = pickerTrigger.closest<HTMLElement>("[data-settings-content]")
      ?? pickerTrigger.closest<HTMLElement>("[data-settings-modal-panel]");
    if (!boundaryElement) {
      return { top: 0, right: window.innerWidth, bottom: window.innerHeight, left: 0 };
    }
    const boundary = boundaryElement.getBoundingClientRect();
    return {
      top: Math.max(0, boundary.top),
      right: Math.min(window.innerWidth, boundary.right),
      bottom: Math.min(window.innerHeight, boundary.bottom),
      left: Math.max(0, boundary.left),
    };
  }

  function positionPicker(): void {
    if (!controlled || !pickerTrigger || !pickerPanel) return;
    const trigger = pickerTrigger.getBoundingClientRect();
    const panel = pickerPanel.getBoundingClientRect();
    const boundary = pickerBoundaryRect();
    const topBound = boundary.top + pickerBoundaryInsetPx;
    const rightBound = boundary.right - pickerBoundaryInsetPx;
    const bottomBound = boundary.bottom - pickerBoundaryInsetPx;
    const leftBound = boundary.left + pickerBoundaryInsetPx;
    const aboveTop = trigger.top - pickerGapPx - panel.height;
    const belowTop = trigger.bottom + pickerGapPx;
    const aboveFits = aboveTop >= topBound;
    const belowFits = belowTop + panel.height <= bottomBound;
    const availableAbove = Math.max(0, trigger.top - pickerGapPx - topBound);
    const availableBelow = Math.max(0, bottomBound - belowTop);
    const preferredTop = belowFits || (!aboveFits && availableBelow >= availableAbove)
      ? belowTop
      : aboveTop;
    const maximumTop = Math.max(topBound, bottomBound - panel.height);
    const targetControlWidth = Math.max(trigger.width, panel.width);
    const maximumLeft = Math.max(leftBound, rightBound - panel.width);
    pickerPlacement = preferredTop === belowTop ? "below" : "above";
    pickerPosition = {
      left: clamp(trigger.left + targetControlWidth - panel.width, leftBound, maximumLeft),
      top: clamp(preferredTop, topBound, maximumTop),
    };
  }

  function setPickerView(nextView: PickerView): void {
    if (view === nextView) return;
    view = nextView;
    closeFlyout();
    void tick().then(() => {
      const target = nextView === "advanced" ? advancedHeading : advancedToggle;
      target?.focus({ preventScroll: true });
    });
  }

  function probeStatus(entry: (typeof providers)[number]): string {
    if (!entry.configuration.enabled) return t("chat.composer.providerDisabled");
    if (!entry.lastProbe) return t("chat.composer.providerNotChecked");
    if (entry.lastProbe.state === "healthy") return t("chat.status.idle");
    return entry.lastProbe.detail ?? t("chat.status.providerUnavailable");
  }

  async function toggleModelFavorite(entry: ProviderInstanceRead, modelId: string): Promise<void> {
    const favorites = entry.configuration.favoriteModelIds.includes(modelId)
      ? entry.configuration.favoriteModelIds.filter((id) => id !== modelId)
      : [...entry.configuration.favoriteModelIds, modelId];
    modelPickerError = null;
    try {
      await chat.updateModels(entry.configuration.instanceId, entry.configuration.visibleModelIds, favorites);
    } catch (error: unknown) {
      modelPickerError = error instanceof Error ? error.message : String(error);
    }
  }

  function openProviderSettings(): void {
    closePicker();
    settings.open("chat", { chatSubsection: "providers" });
  }

  function chooseModel(entry: (typeof providers)[number], modelId: string | null, providerManaged: boolean): void {
    const target = { providerInstanceId: entry.configuration.instanceId, modelId, providerManaged };
    if (value === undefined && chat.selectedThread && entry.configuration.instanceId !== chat.selectedThread.providerInstanceId) {
      pendingProviderModel = target;
      return;
    }
    applyProviderModelSelection(entry, modelId, providerManaged);
  }

  function applyProviderModelSelection(
    entry: (typeof providers)[number],
    modelId: string | null,
    providerManaged: boolean,
  ): void {
    const model = visibleProviderModels(entry, catalogSelection).find((candidate) => candidate.id === modelId);
    commitSelection({
      providerInstanceId: entry.configuration.instanceId,
      modelId,
      providerManaged,
      options: model ? defaultModelOptions(model.options) : [],
    });
    quickAnchorModelId = modelId;
  }

  function chooseQuickEffort(choice: QuickEffortChoice): void {
    const model = models.find((candidate) => candidate.id === choice.modelId);
    if (!model) return;
    const options = choice.modelId === selection.modelId
      ? selection.options.filter((entry) => entry.key !== choice.effortKey)
      : defaultModelOptions(model.options).filter((entry) => entry.key !== choice.effortKey);
    options.push({ key: choice.effortKey, value: { kind: "choice", value: choice.effortValue } });
    commitSelection({
      providerInstanceId,
      modelId: model.id,
      providerManaged: false,
      options,
    });
  }

  function commitSelection(next: ChatModelControlSelection): void {
    if (value !== undefined) {
      onChange?.(next);
      return;
    }
    if (next.providerInstanceId !== providerInstanceId) {
      chat.setComposerProvider(next.providerInstanceId);
    }
    chat.setComposerModel(composerModelSelection(
      next.modelId,
      next.providerManaged,
      next.options,
    ));
  }

  function cancelEffortClickReset(): void {
    if (effortClickResetTimer === null) return;
    clearTimeout(effortClickResetTimer);
    effortClickResetTimer = null;
  }

  function resetEffortPointerState(): void {
    effortPressing = false;
    effortDragging = false;
    effortPointerId = null;
    effortPointerOrigin = null;
    effortLastDragIndex = -1;
  }

  function updateEffortHandleHover(ladder: HTMLElement, clientX: number, clientY: number, selectedIndex = selectedQuickEffortIndex): void {
    const button = ladder.querySelectorAll<HTMLButtonElement>(".effort-options button")[selectedIndex];
    if (!button) {
      effortHandleHovered = false;
      return;
    }
    const rect = button.getBoundingClientRect();
    const radius = Math.min(rect.width, rect.height) / 2;
    effortHandleHovered = radius > 0
      && Math.hypot(clientX - (rect.left + rect.width / 2), clientY - (rect.top + rect.height / 2)) <= radius;
  }

  function chooseEffortAtPointer(ladder: HTMLElement, clientX: number): void {
    const buttons = [...ladder.querySelectorAll<HTMLButtonElement>(".effort-options button")];
    if (buttons.length === 0) return;
    let nearestIndex = 0;
    let nearestDistance = Number.POSITIVE_INFINITY;
    for (const [index, button] of buttons.entries()) {
      const rect = button.getBoundingClientRect();
      const distance = Math.abs(clientX - (rect.left + rect.width / 2));
      if (distance < nearestDistance) {
        nearestDistance = distance;
        nearestIndex = index;
      }
    }
    if (nearestIndex === effortLastDragIndex) return;
    effortLastDragIndex = nearestIndex;
    const choice = quickEffortChoices[nearestIndex];
    if (choice) chooseQuickEffort(choice);
  }

  function handleEffortPointerDown(event: PointerEvent): void {
    if (event.button !== 0 || event.isPrimary === false || !(event.currentTarget instanceof HTMLElement)) return;
    event.preventDefault();
    cancelEffortClickReset();
    effortPressing = true;
    effortDragging = false;
    effortPointerId = event.pointerId;
    effortPointerOrigin = { x: event.clientX, y: event.clientY };
    effortLastDragIndex = selectedQuickEffortIndex;
    suppressEffortPointerClick = true;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    chooseEffortAtPointer(event.currentTarget, event.clientX);
  }

  function handleEffortPointerMove(event: PointerEvent): void {
    if (!(event.currentTarget instanceof HTMLElement)) return;
    if (effortPressing && event.pointerId === effortPointerId) {
      if (!effortDragging && effortPointerOrigin && Math.hypot(event.clientX - effortPointerOrigin.x, event.clientY - effortPointerOrigin.y) >= effortDragThresholdPx) {
        effortDragging = true;
      }
      chooseEffortAtPointer(event.currentTarget, event.clientX);
    }
    updateEffortHandleHover(
      event.currentTarget,
      event.clientX,
      event.clientY,
      effortPressing ? effortLastDragIndex : selectedQuickEffortIndex,
    );
  }

  function finishEffortPointer(event: PointerEvent, selectFinalChoice: boolean): void {
    if (!effortPressing || event.pointerId !== effortPointerId || !(event.currentTarget instanceof HTMLElement)) return;
    if (selectFinalChoice) chooseEffortAtPointer(event.currentTarget, event.clientX);
    if (selectFinalChoice) updateEffortHandleHover(event.currentTarget, event.clientX, event.clientY, effortLastDragIndex);
    else effortHandleHovered = false;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    resetEffortPointerState();
    cancelEffortClickReset();
    effortClickResetTimer = setTimeout(() => {
      effortClickResetTimer = null;
      suppressEffortPointerClick = false;
    }, 0);
  }

  function handleEffortPointerLeave(): void {
    effortHandleHovered = false;
  }

  function handleEffortChoiceClick(choice: QuickEffortChoice, event: MouseEvent): void {
    if (event.detail > 0 && suppressEffortPointerClick) {
      event.preventDefault();
      return;
    }
    chooseQuickEffort(choice);
  }

  function openOption(definition: KnownModelOption, trigger?: HTMLElement): void {
    if (trigger) activeFlyoutTrigger = trigger;
    optionViewKey = definition.key;
    flyout = "option";
    scheduleFlyoutPosition();
  }

  function openFlyout(next: FlyoutView, trigger?: HTMLElement): void {
    if (trigger) activeFlyoutTrigger = trigger;
    if (next === "models" && flyout !== "models") {
      modelQuery = "";
      modelPickerError = null;
    }
    optionViewKey = null;
    flyout = next;
    scheduleFlyoutPosition();
  }

  function closeFlyout(): void {
    optionViewKey = null;
    flyout = null;
    flyoutPosition = null;
    activeFlyoutTrigger = null;
  }

  /** Whether the pointer is travelling from the open flyout's row toward the flyout, so rows it crosses do not replace it. */
  function pointerAimingAtFlyout(event: PointerEvent): boolean {
    if (!activeFlyoutTrigger || !flyoutPanel) return false;
    const row = activeFlyoutTrigger.getBoundingClientRect();
    const panel = flyoutPanel.getBoundingClientRect();
    if (panel.width === 0 || panel.height === 0) return false;
    return isPointerAimingAtSubmenu({
      origin: { x: row.left + row.width / 2, y: row.top + row.height / 2 },
      point: { x: event.clientX, y: event.clientY },
      submenu: panel,
      side: panel.left >= row.right ? "right" : "left",
      ...SUBMENU_AIM_TOLERANCES,
    });
  }

  function handleOptionPointer(definition: KnownModelOption, event: PointerEvent): void {
    if (!(event.currentTarget instanceof HTMLElement)) return;
    if (flyout === "option" && optionViewKey === definition.key) return;
    if (flyout && pointerAimingAtFlyout(event)) return;
    openOption(definition, event.currentTarget);
  }

  function handleNamedFlyoutPointer(next: FlyoutView, event: PointerEvent): void {
    if (!(event.currentTarget instanceof HTMLElement)) return;
    if (flyout === next) return;
    if (flyout && pointerAimingAtFlyout(event)) return;
    openFlyout(next, event.currentTarget);
  }

  function scheduleFlyoutPosition(): void {
    void tick().then(positionFlyout);
  }

  function positionFlyout(): void {
    if (!activeFlyoutTrigger || !flyoutPanel || !pickerPanel) return;
    const anchor = activeFlyoutTrigger.getBoundingClientRect();
    const panel = flyoutPanel.getBoundingClientRect();
    const rightPosition = anchor.right + flyoutGapPx;
    const leftPosition = anchor.left - panel.width - flyoutGapPx;
    const rightFits = rightPosition + panel.width <= window.innerWidth - flyoutViewportInsetPx;
    const leftFits = leftPosition >= flyoutViewportInsetPx;
    const maximumLeft = Math.max(flyoutViewportInsetPx, window.innerWidth - panel.width - flyoutViewportInsetPx);
    const viewportLeft = rightFits
      ? rightPosition
      : leftFits
        ? leftPosition
        : Math.min(Math.max(flyoutViewportInsetPx, rightPosition), maximumLeft);
    const maximumTop = Math.max(flyoutViewportInsetPx, window.innerHeight - panel.height - flyoutViewportInsetPx);
    const viewportTop = Math.min(Math.max(flyoutViewportInsetPx, anchor.top - 4), maximumTop);
    flyoutPosition = {
      left: Math.max(flyoutViewportInsetPx, viewportLeft),
      top: viewportTop,
    };
  }

  function updateOption(key: string, value: ModelOptionValue): void {
    const next = selection.options.filter((entry) => entry.key !== key);
    next.push({ key, value });
    commitSelection({
      providerInstanceId,
      modelId: selection.modelId,
      providerManaged: selection.providerManaged,
      options: next,
    });
  }

  function removeOption(key: string): void {
    commitSelection({
      providerInstanceId,
      modelId: selection.modelId,
      providerManaged: selection.providerManaged,
      options: selection.options.filter((entry) => entry.key !== key),
    });
  }

  function option(key: string): ModelOptionSelection | undefined {
    return selection.options.find((entry) => entry.key === key);
  }

  function choiceValue(key: string): string {
    const value = option(key)?.value;
    return value?.kind === "choice" ? value.value : "";
  }

  function booleanValue(key: string): boolean {
    const value = option(key)?.value;
    return value?.kind === "boolean" && value.value;
  }

  function multipleIncludes(key: string, candidate: string): boolean {
    const value = option(key)?.value;
    return value?.kind === "multiple_choice" && value.value.includes(candidate);
  }

  function toggleMultiple(key: string, value: string, checked: boolean): void {
    const current = option(key)?.value;
    const values = current?.kind === "multiple_choice" ? current.value : [];
    updateOption(key, {
      kind: "multiple_choice",
      value: checked
        ? [...new Set([...values, value])]
        : values.filter((entry) => entry !== value),
    });
  }

  function integerValue(key: string, fallback: number): number {
    const value = option(key)?.value;
    return value?.kind === "integer" ? value.value : fallback;
  }

  function textValue(key: string): string {
    const value = option(key)?.value;
    return value?.kind === "text" ? value.value : "";
  }

  function selectedOptionLabel(definition: KnownModelOption | null): string | null {
    if (!definition) return null;
    if (definition.kind === "choice") {
      const selected = definition.options.find((entry) => entry.value === choiceValue(definition.key));
      return humanizeOptionLabel(selected?.label ?? definition.label);
    }
    if (definition.kind === "boolean") return booleanValue(definition.key) ? t("chat.composer.enabled") : t("chat.composer.disabled");
    if (definition.kind === "integer_range") return formatNumber(localization.locale, integerValue(definition.key, definition.defaultValue ?? definition.minimum));
    if (definition.kind === "text") return textValue(definition.key) || definition.label;
    const value = option(definition.key)?.value;
    return value?.kind === "multiple_choice" ? formatNumber(localization.locale, value.value.length) : definition.label;
  }

  function selectedEffortValue(): string | null {
    if (!effortDefinition) return null;
    const value = option(effortDefinition.key)?.value;
    return value?.kind === "choice" ? value.value : null;
  }

  function isUltraSelected(): boolean {
    return selectedQuickEffortIndex >= 0 && selectedQuickEffortIndex === quickEffortChoices.length - 1;
  }

  function isFastSelected(): boolean {
    if (!speedDefinition) return false;
    if (speedDefinition.kind === "boolean") return booleanValue(speedDefinition.key);
    if (speedDefinition.kind !== "choice") return false;
    const selected = speedDefinition.options.find((entry) => entry.value === choiceValue(speedDefinition.key));
    return `${selected?.value ?? ""} ${selected?.label ?? ""}`.toLowerCase().includes("fast");
  }

  function setFastMode(enabled: boolean): void {
    if (!speedDefinition) return;
    if (speedDefinition.kind === "boolean") {
      updateOption(speedDefinition.key, { kind: "boolean", value: enabled });
      return;
    }
    if (speedDefinition.kind !== "choice") return;
    const match = speedDefinition.options.find((entry) => {
      const identity = `${entry.value} ${entry.label}`.toLowerCase();
      return enabled ? identity.includes("fast") : identity.includes("standard") || identity.includes("default");
    });
    if (match) updateOption(speedDefinition.key, { kind: "choice", value: match.value });
    else if (!enabled) removeOption(speedDefinition.key);
  }

  function humanizeOptionLabel(label: string): string {
    return compactModelOptionLabel(
      label,
      provider?.configuration.familyId ?? null,
      t("chat.composer.light"),
      t("chat.composer.extraHigh"),
    );
  }

  function displayModelName(model: ProviderModel | null): string {
    if (!model) return provider?.configuration.label ?? t("chat.hero.chooseProvider");
    return compactModelName(model.displayName, provider?.configuration.familyId ?? null);
  }

  function choiceDescription(
    definition: KnownModelOption,
    value: string,
    description: string | null,
  ): string | null {
    const providerDescription = description?.trim();
    if (providerDescription) return providerDescription;
    if (modelOptionRole(definition) !== "speed") return null;
    const normalized = value.trim().toLowerCase();
    if (["standard", "default", "normal"].includes(normalized)) {
      return t("chat.composer.standardSpeedDescription");
    }
    if (["fast", "priority"].includes(normalized)) {
      return t("chat.composer.fastSpeedDescription");
    }
    return null;
  }

  function modelMetadata(contextLimit: number | null, availability: string): string[] {
    const values: string[] = [];
    if (contextLimit !== null) values.push(t("chat.composer.modelContext", formatNumber(localization.locale, contextLimit)));
    if (availability === "stale") values.push(t("chat.composer.modelStale"));
    if (availability === "unavailable") values.push(t("chat.composer.modelUnavailable"));
    return values;
  }

  function handlePickerKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      closePicker();
      return;
    }
    trapTabFocus(event);
  }

  async function confirmProviderFork(): Promise<void> {
    if (!pendingProviderModel) return;
    const target = pendingProviderModel;
    pendingProviderModel = null;
    await chat.forkComposerWithProvider(target.providerInstanceId);
    const entry = providers.find((candidate) => candidate.configuration.instanceId === target.providerInstanceId);
    if (entry) applyProviderModelSelection(entry, target.modelId, target.providerManaged);
  }
</script>

<div bind:this={pickerRoot} class="model-control" class:controlled>
  <button bind:this={pickerTrigger} type="button" class="model-trigger" data-chat-model-trigger aria-expanded={pickerOpen} disabled={disabled || (value === undefined && chat.composer.loading)} onclick={togglePicker}>
    <span class="model-trigger-content">
      <span class="fast-indicator" class:active={isFastSelected()} aria-hidden="true">{#if chat.providerDiscoveryLoading && !provider}<LoaderCircle size={13} class="animate-spin" />{:else}<Zap size={13} fill="currentColor" />{/if}</span>
      <span class="model-name">{chat.providerDiscoveryLoading && !provider ? t("chat.composer.detectingProviders") : selection.providerManaged ? provider?.configuration.label ?? t("chat.composer.providerManagedModel") : displayModelName(selectedModel)}</span>
      {#if selectedOptionLabel(effortDefinition)}<span class="effort-name" class:ultra={isUltraSelected()}>{selectedOptionLabel(effortDefinition)}</span>{/if}
      <ChevronDown size={13} class={pickerOpen ? "model-chevron open" : "model-chevron"} />
    </span>
  </button>

  {#if pickerOpen}
    <div
      bind:this={pickerPanel}
      use:portal={controlled ? "body" : pickerRoot ?? "body"}
      use:keepComposerPopoverInViewport
      class="model-popover surface-floating w-floating"
      class:portaled={controlled}
      class:below={controlled && pickerPlacement === "below"}
      class:positioned={!controlled || pickerPosition !== null}
      style:left={controlled && pickerPosition !== null ? `${pickerPosition.left}px` : undefined}
      style:top={controlled && pickerPosition !== null ? `${pickerPosition.top}px` : undefined}
      role="dialog"
      aria-label={t("chat.hero.model")}
      tabindex="-1"
      data-app-floating-surface
      onkeydown={handlePickerKeydown}
    >
      <div class="picker-stage" style:height={pickerStageHeight === null ? undefined : `${pickerStageHeight}px`}>
        <div bind:this={overviewPanel} class="picker-view overview-view" class:active={view === "overview"} inert={view !== "overview"} aria-hidden={view !== "overview"}>
        {#if quickEffortChoices.length > 0}
          <div class="effort-ladder" class:fast={isFastSelected()} class:ultra={isUltraSelected()} class:holding={effortPressing} class:handle-hovered={effortHandleHovered} style={`--effort-track-height:${effortTrackHeightRem}rem`} role="group" aria-label={t("chat.composer.quickModelEffort")} onpointerdown={handleEffortPointerDown} onpointermove={handleEffortPointerMove} onpointerleave={handleEffortPointerLeave} onpointerup={(event) => finishEffortPointer(event, true)} onpointercancel={(event) => finishEffortPointer(event, false)} onlostpointercapture={(event) => finishEffortPointer(event, false)}>
            <span class="effort-fill" style={`width:${selectedQuickEffortIndex < 0 ? "0" : isUltraSelected() ? "100%" : modelEffortStopPosition(selectedQuickEffortIndex, quickEffortChoices.length, effortEndpointInsetRem)}`} aria-hidden="true">
              <span class="effort-particles calm"></span>
              <span class="effort-particles rapid"></span>
            </span>
            <span class="effort-options">
              {#each quickEffortChoices as choice, index}
                <button type="button" style={`left:${modelEffortStopPosition(index, quickEffortChoices.length, effortEndpointInsetRem)}`} aria-label={`${choice.modelName} ${choice.effortLabel}`} aria-pressed={index === selectedQuickEffortIndex} data-app-tooltip-disabled="true" data-app-tooltip-focus-disabled="true" onclick={(event) => handleEffortChoiceClick(choice, event)}><i></i></button>
              {/each}
              {#if selectedQuickEffortIndex >= 0}<span class="effort-knob" style={`left:${modelEffortStopPosition(selectedQuickEffortIndex, quickEffortChoices.length, effortEndpointInsetRem)}`} aria-hidden="true"></span>{/if}
            </span>
          </div>
        {:else}
          <div class="effort-ladder dummy" style={`--effort-track-height:${effortTrackHeightRem}rem`} role="group" aria-label={provider ? t("chat.composer.effortUnavailableForModel") : t("chat.composer.providerRequiredForModelOptions")} aria-disabled="true">
            <span class="effort-options">
              {#each dummyEffortStops as stop}
                <span class="effort-stop" style={`left:${modelEffortStopPosition(stop, dummyEffortStops.length, effortEndpointInsetRem)}`} aria-hidden="true"><i></i></span>
              {/each}
            </span>
          </div>
        {/if}
        <div class="effort-footer" class:holding={effortDragging}>
          <div class="quick-actions" inert={effortDragging} aria-hidden={effortDragging}>
            <button bind:this={advancedToggle} type="button" class="advanced-toggle" onclick={() => setPickerView("advanced")}><span>{t("chat.composer.advanced")}</span><span class="advanced-chevron" class:expanded={view === "advanced"}><ChevronRight size={15} /></span></button>
            {#if speedDefinition}<button type="button" class="fast-button" class:active={isFastSelected()} class:ultra={isUltraSelected()} title={isFastSelected() ? t("chat.composer.fastEnabled") : t("chat.composer.enableFast")} aria-label={isFastSelected() ? t("chat.composer.fastEnabled") : t("chat.composer.enableFast")} aria-pressed={isFastSelected()} onclick={() => setFastMode(!isFastSelected())}><Zap size={16} strokeWidth={1} /></button>
            {:else if !provider}<button type="button" class="fast-button dummy" title={t("chat.composer.providerRequiredForModelOptions")} aria-label={t("chat.composer.providerRequiredForModelOptions")} disabled><Zap size={16} strokeWidth={1} /></button>{/if}
          </div>
          <div class="effort-guidance" aria-hidden="true"><span>{t("chat.composer.faster")}</span><span>{t("chat.composer.smarter")}</span></div>
        </div>
        </div>
        <div bind:this={advancedPanel} class="picker-view advanced-view" class:active={view === "advanced"} inert={view !== "advanced"} aria-hidden={view !== "advanced"}>
          <button bind:this={advancedHeading} type="button" class="advanced-heading" onclick={() => setPickerView("overview")}><span>{t("chat.composer.advanced")}</span><span class="advanced-chevron" class:expanded={view === "advanced"}><ChevronRight size={15} /></span></button>
          <div class="advanced-list">
            <button type="button" class="menu-item" aria-expanded={flyout === "models"} onpointerenter={(event) => handleNamedFlyoutPointer("models", event)} onpointermove={(event) => handleNamedFlyoutPointer("models", event)} onfocus={(event) => openFlyout("models", event.currentTarget)} onclick={(event) => { openFlyout("models", event.currentTarget); void tick().then(() => modelSearch?.focus()); }}><span>{t("chat.hero.model")}</span><small>{selection.providerManaged ? t("chat.composer.providerManagedModel") : displayModelName(selectedModel)}</small><ChevronRight size={15} /></button>
            {#if effortDefinition}<button type="button" class="menu-item" aria-expanded={flyout === "option" && optionViewKey === effortDefinition.key} onpointerenter={(event) => handleOptionPointer(effortDefinition, event)} onpointermove={(event) => handleOptionPointer(effortDefinition, event)} onfocus={(event) => openOption(effortDefinition, event.currentTarget)} onclick={(event) => openOption(effortDefinition, event.currentTarget)}><span>{t("chat.composer.effort")}</span><small>{selectedOptionLabel(effortDefinition)}</small><ChevronRight size={15} /></button>
            {:else if !provider}<button type="button" class="menu-item" disabled title={t("chat.composer.providerRequiredForModelOptions")}><span>{t("chat.composer.effort")}</span><small></small><ChevronRight size={15} /></button>{/if}
            {#if speedDefinition}<button type="button" class="menu-item" aria-expanded={flyout === "option" && optionViewKey === speedDefinition.key} onpointerenter={(event) => handleOptionPointer(speedDefinition, event)} onpointermove={(event) => handleOptionPointer(speedDefinition, event)} onfocus={(event) => openOption(speedDefinition, event.currentTarget)} onclick={(event) => openOption(speedDefinition, event.currentTarget)}><span>{t("chat.composer.speed")}</span><small>{isFastSelected() ? t("chat.composer.fast") : t("chat.composer.standard")}</small><ChevronRight size={15} /></button>
            {:else if !provider}<button type="button" class="menu-item" disabled title={t("chat.composer.providerRequiredForModelOptions")}><span>{t("chat.composer.speed")}</span><small></small><ChevronRight size={15} /></button>{/if}
            {#each otherDefinitions as definition}<button type="button" class="menu-item" aria-expanded={flyout === "option" && optionViewKey === definition.key} onpointerenter={(event) => handleOptionPointer(definition, event)} onpointermove={(event) => handleOptionPointer(definition, event)} onfocus={(event) => openOption(definition, event.currentTarget)} onclick={(event) => openOption(definition, event.currentTarget)}><span>{definition.label}</span><small>{selectedOptionLabel(definition)}</small><ChevronRight size={15} /></button>{/each}
          </div>
        </div>
      </div>

        {#if view === "advanced" && flyout}
          <div bind:this={flyoutPanel} use:portal class={cn("model-flyout surface-floating", flyout === "models" ? "w-floating-lg" : "w-floating")} class:positioned={flyoutPosition !== null} class:model-picker-flyout={flyout === "models"} style:left={flyoutPosition === null ? undefined : `${flyoutPosition.left}px`} style:top={flyoutPosition === null ? undefined : `${flyoutPosition.top}px`} role="dialog" tabindex="-1" aria-label={flyout === "models" ? t("chat.hero.model") : optionViewDefinition?.label} data-app-floating-surface onkeydown={handlePickerKeydown}>
            {#if flyout === "models"}
              <div class="model-picker-shell">
                <div class="model-picker-main">
                  <div class="model-list-frame">
                  <div
                    bind:this={modelListElement}
                    class="selection-list model-list hide-scrollbar"
                    use:scrollEdgeFadeAction
                  >
                    <div class="model-list-content">
                    <div class="model-search-row">
                      <label class="model-search"><Search size={16} /><input class="field-bare" bind:this={modelSearch} bind:value={modelQuery} placeholder={t("chat.composer.modelSearch")} /></label>
                    </div>
                    <section class="model-company-section favorite-company-section" aria-labelledby="favorite-models-heading">
                      <button id="favorite-models-heading" type="button" class="model-company-heading" aria-expanded={!isModelSectionCollapsed("favorites")} aria-controls="favorite-models-content" onclick={() => toggleModelSection("favorites")}><Star size={14} fill="currentColor" /><span>{t("chat.composer.favorites")}</span><ChevronDown size={13} class={isModelSectionCollapsed("favorites") ? "collapsed" : undefined} /></button>
                      <div id="favorite-models-content" class="model-company-content" class:collapsed={isModelSectionCollapsed("favorites")} inert={isModelSectionCollapsed("favorites")} aria-hidden={isModelSectionCollapsed("favorites")}>
                        <div class="model-company-content-inner">
                          {#each favoriteModelEntries as favorite (`favorite:${favorite.provider.configuration.instanceId}:${favorite.model.id}`)}
                            {@const metadata = modelMetadata(favorite.model.contextLimit, favorite.model.availability)}
                            {@const company = modelCompany(favorite.provider.configuration.familyId, favorite.model)}
                            <div class="model-row">
                              <button type="button" class="model-choice" disabled={!isProviderAvailable(favorite.provider) || favorite.model.availability === "unavailable"} title={favorite.model.availability === "available" ? undefined : metadata.join(" · ")} onclick={() => chooseModel(favorite.provider, favorite.model.id, false)}>
                                <span class="favorite-model-label"><ChatProviderIcon familyId={company.iconFamilyId} label={company.name} monochrome size={14} /><strong>{formatModelDisplayName(favorite.model.displayName)}</strong></span>
                                {#if favorite.provider.configuration.instanceId === provider?.configuration.instanceId && selection.modelId === favorite.model.id}<Check size={15} />{/if}
                              </button>
                              <button type="button" class="model-favorite active" aria-label={`${t("chat.composer.favorite")}: ${formatModelDisplayName(favorite.model.displayName)}`} aria-pressed="true" onclick={() => void toggleModelFavorite(favorite.provider, favorite.model.id)}><Star size={16} fill="currentColor" /></button>
                            </div>
                          {/each}
                          {#if favoriteModelEntries.length === 0}<p>{modelQuery ? t("chat.composer.noFavoriteMatches") : t("chat.composer.noFavoriteModels")}</p>{/if}
                        </div>
                      </div>
                    </section>

                    {#each modelCompanySections as section (section.company.id)}
                      {@const sectionCollapsed = isModelSectionCollapsed(section.company.id)}
                      <section class="model-company-section" data-model-company={section.company.id} aria-labelledby={`model-company-${section.company.id}`}>
                        <button id={`model-company-${section.company.id}`} type="button" class="model-company-heading" aria-expanded={!sectionCollapsed} aria-controls={`model-company-${section.company.id}-content`} onclick={() => toggleModelSection(section.company.id)}><ChatProviderIcon familyId={section.company.iconFamilyId} label={section.company.name} monochrome size={14} /><span>{section.company.name}</span><ChevronDown size={13} class={sectionCollapsed ? "collapsed" : undefined} /></button>
                        <div id={`model-company-${section.company.id}-content`} class="model-company-content" class:collapsed={sectionCollapsed} inert={sectionCollapsed} aria-hidden={sectionCollapsed}>
                          <div class="model-company-content-inner">
                            {#each section.managedProviders as managedProvider (managedProvider.configuration.instanceId)}
                              <button type="button" class="menu-item" disabled={!isProviderAvailable(managedProvider)} title={probeStatus(managedProvider)} onclick={() => chooseModel(managedProvider, null, true)}>
                                <span><strong>{t("chat.composer.providerManagedModel")}</strong></span>
                                {#if managedProvider.configuration.instanceId === provider?.configuration.instanceId && selection.providerManaged}<Check size={14} />{/if}
                              </button>
                            {/each}
                            {#each section.models as entry (`${entry.provider.configuration.instanceId}:${entry.model.id}`)}
                              {@const metadata = modelMetadata(entry.model.contextLimit, entry.model.availability)}
                              <div class="model-row">
                                <button type="button" class="model-choice" disabled={!isProviderAvailable(entry.provider) || entry.model.availability === "unavailable"} title={entry.model.availability === "available" ? undefined : metadata.join(" · ")} onclick={() => chooseModel(entry.provider, entry.model.id, false)}>
                                  <span><strong>{formatModelDisplayName(entry.model.displayName)}</strong></span>
                                  {#if entry.provider.configuration.instanceId === provider?.configuration.instanceId && selection.modelId === entry.model.id}<Check size={15} />{/if}
                                </button>
                                <button type="button" class="model-favorite" class:active={entry.provider.configuration.favoriteModelIds.includes(entry.model.id)} aria-label={`${t("chat.composer.favorite")}: ${formatModelDisplayName(entry.model.displayName)}`} aria-pressed={entry.provider.configuration.favoriteModelIds.includes(entry.model.id)} onclick={() => void toggleModelFavorite(entry.provider, entry.model.id)}><Star size={16} fill={entry.provider.configuration.favoriteModelIds.includes(entry.model.id) ? "currentColor" : "none"} /></button>
                              </div>
                            {/each}
                            {#each section.setupFamilies as family (family.familyId)}
                              <button type="button" class="company-setup menu-item" onclick={openProviderSettings}><span><strong>{t("chat.composer.configureProvider")} {section.company.name}</strong></span><Plus size={14} /></button>
                            {/each}
                          </div>
                        </div>
                      </section>
                    {/each}
                    {#if modelCompanySections.length === 0}<p>{t("chat.composer.noModels")}</p>{/if}
                    {#if modelPickerError}<Notice class="model-picker-error" message={modelPickerError} />{/if}
                    </div>
                  </div>
                  <CalendarScrollbar scrollContainer={modelListElement} stickyTop={4} stickyBottom={4} wheelPassthrough />
                  </div>
                </div>
              </div>
            {:else if optionViewDefinition}
              <div class="selection-list option-list surface-floating-body" use:scrollEdgeFadeAction>
                {#if optionViewDefinition.kind === "choice"}
                  {#each optionViewDefinition.options as choice}
                    {@const description = choiceDescription(optionViewDefinition, choice.value, choice.description)}
                    <button type="button" class="menu-item" onclick={() => updateOption(optionViewDefinition.key, { kind: "choice", value: choice.value })}><span><strong>{humanizeOptionLabel(choice.label)}</strong>{#if description && modelOptionRole(optionViewDefinition) !== "effort"}<small>{description}</small>{/if}</span>{#if choiceValue(optionViewDefinition.key) === choice.value}<Check size={14} />{/if}</button>
                  {/each}
                {:else if optionViewDefinition.kind === "boolean"}
                  {#if modelOptionRole(optionViewDefinition) === "speed"}
                    <button type="button" class="menu-item" onclick={() => setFastMode(false)}><span><strong>{t("chat.composer.standard")}</strong><small>{t("chat.composer.standardSpeedDescription")}</small></span>{#if !booleanValue(optionViewDefinition.key)}<Check size={14} />{/if}</button>
                    <button type="button" class="menu-item" onclick={() => setFastMode(true)}><span><strong>{t("chat.composer.fast")}</strong><small>{optionViewDefinition.description?.trim() || t("chat.composer.fastSpeedDescription")}</small></span>{#if booleanValue(optionViewDefinition.key)}<Check size={14} />{/if}</button>
                  {:else}
                    <button type="button" class="menu-item" onclick={() => updateOption(optionViewDefinition.key, { kind: "boolean", value: !booleanValue(optionViewDefinition.key) })}><span><strong>{optionViewDefinition.label}</strong>{#if optionViewDefinition.description && modelOptionRole(optionViewDefinition) !== "effort"}<small>{optionViewDefinition.description}</small>{/if}</span>{#if booleanValue(optionViewDefinition.key)}<Check size={14} />{/if}</button>
                  {/if}
                {:else if optionViewDefinition.kind === "multiple_choice"}
                  {#each optionViewDefinition.options as choice}<label class="menu-item"><Checkbox checked={multipleIncludes(optionViewDefinition.key, choice.value)} onChange={(next) => toggleMultiple(optionViewDefinition.key, choice.value, next)} /><span><strong>{choice.label}</strong>{#if choice.description}<small>{choice.description}</small>{/if}</span></label>{/each}
                {:else if optionViewDefinition.kind === "integer_range"}
                  <label class="range-option"><span>{formatNumber(localization.locale, integerValue(optionViewDefinition.key, optionViewDefinition.defaultValue ?? optionViewDefinition.minimum))}</span><input type="range" min={optionViewDefinition.minimum} max={optionViewDefinition.maximum} step={optionViewDefinition.step} value={integerValue(optionViewDefinition.key, optionViewDefinition.defaultValue ?? optionViewDefinition.minimum)} oninput={(event) => updateOption(optionViewDefinition.key, { kind: "integer", value: event.currentTarget.valueAsNumber })} /></label>
                {:else if optionViewDefinition.kind === "text"}
                  <label class="text-option"><span>{optionViewDefinition.label}</span><input type="text" class="field" value={textValue(optionViewDefinition.key)} oninput={(event) => updateOption(optionViewDefinition.key, { kind: "text", value: event.currentTarget.value })} /></label>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
    </div>
  {/if}
</div>

{#if pendingProviderModel}
  <ChatProviderForkDialog
    bind:element={providerForkDialog}
    onCancel={() => { pendingProviderModel = null; }}
    onConfirm={() => void confirmProviderFork()}
  />
{/if}

<style>
  .model-control { position: relative; min-width: 0; flex: 0 1 auto; user-select: none; }
  .model-trigger { display: flex; width: auto; height: 2rem; max-width: 17rem; align-items: center; justify-content: center; border-radius: 999px; background: transparent; padding: 0.3rem 0.55rem; color: var(--foreground); font-size: calc(0.766667rem * var(--type-scale)); }
  .model-control.controlled .model-trigger { background: color-mix(in srgb, var(--accent) 52%, transparent); }
  .model-trigger-content { display: inline-flex; min-width: 0; max-width: 100%; align-items: center; gap: 0.3rem; }
  .model-trigger:hover, .model-trigger[aria-expanded="true"] { background: color-mix(in srgb, var(--accent) 52%, transparent); }
  .model-trigger:disabled { cursor: not-allowed; opacity: 0.5; }
  .model-trigger:focus-visible { outline: 1px solid color-mix(in srgb, var(--ring) 50%, transparent); outline-offset: 1px; }
  .model-trigger :global(svg) { flex: 0 0 auto; }
  .model-trigger :global(.model-chevron) { margin-left: 0.2rem; color: var(--muted-foreground); transition: transform 120ms ease; }
  .model-trigger :global(.model-chevron.open) { transform: rotate(180deg); }
  .fast-indicator { display: grid; width: 0; flex: 0 0 auto; place-items: center; overflow: hidden; opacity: 0; transform: scale(0.72); transition: width 180ms cubic-bezier(0.2, 0.8, 0.2, 1), opacity 140ms ease, transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1); }
  .fast-indicator.active { width: 0.9rem; opacity: 1; transform: scale(1); }
  .model-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .effort-name { flex: 0 0 auto; color: var(--primary); transition: color 260ms ease; }
  .model-control.controlled .effort-name:not(.ultra) { color: var(--foreground); }
  .effort-name.ultra { color: #7c3aed; }
  .model-popover { position: absolute; right: 0; bottom: calc(100% + 0.45rem); z-index: 45; overflow: visible; padding: 0.65rem 0.6rem 0.5rem; }
  .model-popover.portaled { position: fixed; right: auto; bottom: auto; z-index: 80; }
  .model-popover.portaled:not(.positioned) { visibility: hidden; }
  .model-popover.portaled.positioned { animation: model-popover-enter 180ms cubic-bezier(0.22, 0.75, 0.18, 1); }
  .model-popover.portaled.below { --model-popover-enter-y: -0.25rem; }
  .picker-stage { position: relative; overflow-x: visible; overflow-y: clip; transition: height 320ms cubic-bezier(0.22, 0.75, 0.18, 1); }
  .picker-view { width: 100%; opacity: 0; pointer-events: none; transition: opacity 190ms ease, transform 300ms cubic-bezier(0.22, 0.75, 0.18, 1); will-change: opacity, transform; }
  .picker-view:not(.active) { position: absolute; inset: 0 0 auto; }
  .overview-view { padding-top: 0.45rem; transform: translateY(-0.7rem) scale(0.99); }
  .advanced-view { transform: translateY(0.8rem) scale(0.99); }
  .picker-view.active { position: relative; opacity: 1; pointer-events: auto; transform: translateY(0) scale(1); transition-delay: 55ms, 0ms; }
  .effort-ladder { position: relative; height: var(--effort-track-height); overflow: visible; border: 1px solid color-mix(in srgb, var(--border) 88%, transparent); border-radius: 999px; background: #e4e5e9; cursor: default; touch-action: none; user-select: none; transition: border-color 260ms ease, background-color 260ms ease; }
  .effort-fill { position: absolute; inset-block: 0; left: 0; overflow: hidden; border-radius: inherit; background: #0c78d0; transition: width 240ms cubic-bezier(0.22, 0.75, 0.18, 1), background-color 280ms ease; }
  .effort-fill::before { position: absolute; content: ""; inset: 0; opacity: 0; background-image: linear-gradient(105deg, #6ea8ff 0%, #7457f5 48%, #a83cf2 100%); background-size: 180% 100%; animation: ultra-color-flow 3.6s ease-in-out infinite alternate; pointer-events: none; transition: opacity 300ms ease; }
  .effort-particles { position: absolute; inset: 0; opacity: 0; background-image: radial-gradient(circle at 23% 22%, rgb(255 255 255 / 0.78) 0 0.7px, transparent 1.45px), radial-gradient(circle at 68% 72%, rgb(255 255 255 / 0.58) 0 1px, transparent 1.85px), radial-gradient(circle at 37% 44%, rgb(255 255 255 / 0.88) 0 0.6px, transparent 1.3px), radial-gradient(circle at 79% 31%, rgb(255 255 255 / 0.64) 0 1.2px, transparent 2px), radial-gradient(circle at 14% 66%, rgb(255 255 255 / 0.7) 0 0.8px, transparent 1.55px), radial-gradient(circle at 53% 19%, rgb(255 255 255 / 0.54) 0 0.65px, transparent 1.35px), radial-gradient(circle at 86% 58%, rgb(255 255 255 / 0.82) 0 0.9px, transparent 1.7px), radial-gradient(circle at 42% 81%, rgb(255 255 255 / 0.62) 0 0.75px, transparent 1.45px); background-repeat: repeat-x; background-size: 83px 100%, 107px 100%, 131px 100%, 157px 100%, 191px 100%, 223px 100%, 269px 100%, 311px 100%; pointer-events: none; transition: opacity 240ms ease; }
  .effort-particles.calm { animation: galaxy-drift 7s linear infinite; }
  .effort-particles.rapid { animation: galaxy-stream 700ms linear infinite; }
  .effort-options { position: absolute; inset: 0; }
  .effort-options button { position: absolute; top: 50%; display: grid; width: 2.15rem; height: 2.15rem; place-items: center; transform: translate(-50%, -50%); border-radius: 999px; cursor: inherit; }
  .effort-options .effort-stop { position: absolute; top: 50%; display: grid; width: 2.15rem; height: 2.15rem; place-items: center; transform: translate(-50%, -50%); border-radius: 999px; pointer-events: none; }
  .effort-options button:focus-visible { outline: 1px solid color-mix(in srgb, var(--ring) 52%, transparent); outline-offset: -2px; }
  .effort-options :is(button, .effort-stop) i { width: 0.3rem; min-width: 0.3rem; max-width: 0.3rem; height: 0.3rem; min-height: 0.3rem; max-height: 0.3rem; flex: 0 0 0.3rem; border-radius: 999px; background: color-mix(in srgb, var(--muted-foreground) 58%, #e4e5e9); opacity: 1; transform: scale(1); transition: opacity 180ms ease 70ms, background-color 240ms ease, transform 180ms ease 70ms; }
  .effort-options button:hover i { transform: scale(1.4); transition-delay: 0ms; }
  .effort-options button[aria-pressed="true"] i, .effort-options button:has(~ button[aria-pressed="true"]) i { background: color-mix(in srgb, white 48%, #0c78d0); }
  .effort-knob { position: absolute; top: 50%; width: 2.15rem; height: 2.15rem; transform: translate(-50%, -50%) scale(1); border: 1px solid color-mix(in srgb, var(--foreground) 16%, transparent); border-radius: 999px; background: #ffffff; pointer-events: none; transition: left 240ms cubic-bezier(0.22, 0.75, 0.18, 1), transform 170ms cubic-bezier(0.2, 0.8, 0.2, 1), border-color 260ms ease, background-color 260ms ease; }
  .effort-options button[aria-pressed="true"]:focus-visible ~ .effort-knob, .effort-ladder:is(.holding, .handle-hovered) .effort-knob { transform: translate(-50%, -50%) scale(1.065); }
  .effort-ladder.ultra { border-color: color-mix(in srgb, #8b5cf6 30%, transparent); background: #ddd6fe; }
  .effort-ladder.ultra .effort-fill::before, .effort-ladder.ultra:not(.fast) .effort-particles.calm, .effort-ladder.fast .effort-particles.rapid { opacity: 1; }
  .effort-ladder.fast .effort-options button:has(~ button[aria-pressed="true"]) i, .effort-ladder.ultra .effort-options button:has(~ button[aria-pressed="true"]) i { opacity: 0; transform: scale(0.65); transition-delay: 0ms; }
  .effort-ladder.dummy { cursor: not-allowed; opacity: 0.48; }
  :global(.dark) .effort-name.ultra { color: #b794ff; }
  :global(.dark) .effort-ladder { border-color: rgb(255 255 255 / 0.09); background: #45464a; }
  :global(.dark) .effort-fill { background: #1681dc; }
  :global(.dark) .effort-options :is(button, .effort-stop) i { background: color-mix(in srgb, white 32%, #45464a); }
  :global(.dark) .effort-options button[aria-pressed="true"] i, :global(.dark) .effort-options button:has(~ button[aria-pressed="true"]) i { background: color-mix(in srgb, white 46%, #1681dc); }
  :global(.dark) .effort-knob { border-color: rgb(255 255 255 / 0.18); background: #f4f4f6; }
  :global(.dark) .effort-ladder.ultra { border-color: rgb(139 92 246 / 0.4); background: #4c3b78; }
  :global(.dark) .effort-fill::before { background-image: linear-gradient(105deg, #476cf4 0%, #7144e8 48%, #9f35d7 100%); }
  .effort-footer { display: grid; margin-top: 0.4rem; }
  .quick-actions, .effort-guidance { grid-area: 1 / 1; min-height: 2rem; }
  .quick-actions { display: flex; align-items: center; justify-content: space-between; gap: 0.25rem; opacity: 1; transform: translateY(0); transition: opacity 150ms ease, transform 190ms cubic-bezier(0.2, 0.8, 0.2, 1); }
  .effort-guidance { display: flex; align-items: center; justify-content: space-between; padding-inline: 0.15rem; color: var(--muted-foreground); opacity: 0; pointer-events: none; transform: translateY(0.28rem); transition: opacity 170ms ease, transform 210ms cubic-bezier(0.2, 0.8, 0.2, 1); }
  .effort-footer.holding .quick-actions { opacity: 0; pointer-events: none; transform: translateY(0.22rem); }
  .effort-footer.holding .effort-guidance { opacity: 1; transform: translateY(0); }
  .advanced-toggle, .advanced-heading { display: flex; min-height: 2rem; align-items: center; gap: 0.2rem; border-radius: var(--floating-item-radius); padding: 0.3rem 0.2rem; color: var(--muted-foreground); text-align: left; }
  .advanced-toggle { width: 100%; min-width: 0; flex: 1 1 auto; }
  .advanced-toggle:hover, .advanced-heading:hover { background: color-mix(in srgb, var(--accent) 65%, transparent); color: var(--foreground); }
  .advanced-heading { width: 100%; }
  .advanced-chevron { display: grid; width: 0.95rem; height: 0.95rem; flex: 0 0 0.95rem; place-items: center; transform: rotate(0deg); transition: transform 240ms cubic-bezier(0.22, 0.75, 0.18, 1); }
  .advanced-chevron.expanded { transform: rotate(90deg); }
  .fast-button { display: flex; width: 2rem; height: 2rem; flex: 0 0 auto; align-items: center; justify-content: center; margin-inline: 0.375rem -0.375rem; border-radius: 0.55rem; color: var(--muted-foreground); transition: color 240ms ease, background-color 240ms ease, transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1); }
  .fast-button :global(svg) { fill: transparent; transform: scale(0.94); transition: fill 200ms ease, transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1); }
  .fast-button.active :global(svg) { fill: currentColor; transform: scale(1); }
  .fast-button:hover { background: color-mix(in srgb, var(--accent) 65%, transparent); color: var(--foreground); }
  .fast-button.active { background: rgb(12 120 208 / 0.1); color: #0879d8; }
  .fast-button.active.ultra { background: rgb(124 58 237 / 0.1); color: #7c3aed; }
  .fast-button:active { transform: scale(0.92); }
  .fast-button.dummy:disabled { cursor: not-allowed; opacity: 0.48; }
  :global(.dark) .fast-button.active { background: rgb(22 129 220 / 0.16); color: #3b9aeb; }
  :global(.dark) .fast-button.active.ultra { background: rgb(167 139 250 / 0.15); color: #b794ff; }
  .advanced-list { padding-top: 0.3rem; }
  .advanced-list button { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, auto) 1rem; gap: 0.5rem; }
  .advanced-list button:disabled { cursor: not-allowed; opacity: 0.48; }
  .advanced-list small { overflow: hidden; max-width: 9rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); text-overflow: ellipsis; white-space: nowrap; }
  .model-flyout { position: fixed; z-index: 80; display: flex; max-height: min(28rem, 72vh); flex-direction: column; overflow: hidden; }
  .model-flyout:not(.positioned) { visibility: hidden; }
  .model-picker-shell { width: 100%; height: min(24rem, 72vh); min-height: min(18rem, 72vh); }
  .model-picker-main { display: flex; width: 100%; height: 100%; min-width: 0; min-height: 0; flex-direction: column; overflow: hidden; padding: 0.5rem 0.55rem; }
  .option-list { min-height: 0; flex: 1 1 auto; overflow-y: auto; }
  .selection-list > button, .model-company-content-inner > button { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 0.45rem; }
  .selection-list > button:disabled, .model-company-content-inner > button:disabled { opacity: 0.55; }
  .selection-list > button > span, .model-company-content-inner > button > span { min-width: 0; }
  .selection-list strong, .selection-list small { display: block; }
  .selection-list strong { font-weight: 500; }
  .selection-list small { overflow: hidden; margin-top: 0.1rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); text-overflow: ellipsis; white-space: nowrap; }
  .model-list-content > p, .model-company-content-inner > p { padding: 0.38rem 0.5rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .model-search-row { display: flex; flex: 0 0 auto; align-items: center; gap: 0.35rem; border-bottom: 1px solid var(--border); margin: 0 0.15rem 0.2rem; }
  .model-search { display: flex; min-width: 0; flex: 1; align-items: center; gap: 0.45rem; padding: 0.5rem 0.15rem; color: var(--muted-foreground); }
  .model-search:focus-within { color: var(--foreground); }
  .model-search input { user-select: text; color: var(--foreground); }
  .model-list-frame { --cal-scrollbar-thumb: color-mix(in srgb, var(--popover-foreground) 18%, var(--popover)); --cal-scrollbar-thumb-hover: var(--cal-scrollbar-thumb); position: relative; min-height: 0; flex: 1; margin-right: -0.55rem; }
  .model-list { height: 100%; min-height: 0; overflow-y: auto; padding: 0 0.55rem 0 0; }
  .model-company-section { padding: 0.38rem 0; }
  .model-company-section + .model-company-section { border-top: 1px solid color-mix(in srgb, var(--border) 72%, transparent); }
  .model-company-heading { display: grid; width: 100%; min-width: 0; min-height: 2rem; grid-template-columns: 0.875rem minmax(0, 1fr) 1rem; align-items: center; gap: 0.35rem; border-radius: var(--floating-item-radius); padding: 0.38rem 0.5rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 550; text-align: left; }
  .model-company-heading:hover { background: transparent; }
  .model-company-heading:focus-visible { outline: 1px solid color-mix(in srgb, var(--ring) 55%, transparent); outline-offset: -2px; }
  .model-company-heading :global(svg:last-child) { color: var(--muted-foreground); transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1); }
  .model-company-heading :global(svg:last-child.collapsed) { transform: rotate(-90deg); }
  .model-company-heading span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .model-company-content { display: grid; grid-template-rows: 1fr; opacity: 1; transition: grid-template-rows 190ms cubic-bezier(0.2, 0.8, 0.2, 1), opacity 140ms ease; }
  .model-company-content.collapsed { grid-template-rows: 0fr; opacity: 0; }
  .model-company-content-inner { min-height: 0; overflow: hidden; }
  .company-setup { color: var(--muted-foreground); }
  .model-row { display: grid; grid-template-columns: minmax(0, 1fr) 2rem; align-items: center; border-radius: var(--floating-item-radius); }
  .model-row:hover, .model-row:has(:focus-visible) { background: var(--accent); }
  .model-choice { display: grid; min-width: 0; min-height: var(--panel-row-height); grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 0.4rem; padding: 0.38rem 0.5rem; text-align: left; }
  .model-choice:focus-visible, .model-favorite:focus-visible { outline: 1px solid color-mix(in srgb, var(--ring) 55%, transparent); outline-offset: -2px; }
  .model-choice:disabled { opacity: 0.55; }
  .model-choice > span { min-width: 0; }
  .model-choice > .favorite-model-label { display: flex; align-items: center; gap: 0.45rem; }
  .favorite-model-label :global(svg) { color: var(--muted-foreground); }
  .model-favorite { display: grid; width: 2rem; height: 2rem; place-items: center; border-radius: var(--floating-item-radius); color: var(--muted-foreground); }
  .model-favorite:hover, .model-favorite:focus-visible, .model-favorite.active { color: var(--foreground); }
  .model-list-content > :global(.model-picker-error) { padding: 0.38rem 0.5rem; font-size: var(--panel-detail-font-size); }
  .option-list > label > span { min-width: 0; flex: 1; }
  .range-option, .text-option { display: flex; flex-direction: column; gap: 0.35rem; padding: 0.375rem 0.5rem; }
  .range-option input, .text-option input { width: 100%; }
  .text-option input { user-select: text; color: var(--foreground); }
  @keyframes ultra-color-flow { from { background-position: 0 0; } to { background-position: 100% 0; } }
  @keyframes galaxy-drift { from { background-position: 0 0, 0 0, 0 0, 0 0, 0 0, 0 0, 0 0, 0 0; } to { background-position: -83px 0, -107px 0, -131px 0, -157px 0, -191px 0, -223px 0, -269px 0, -311px 0; } }
  @keyframes galaxy-stream { from { background-position: 0 0, 0 0, 0 0, 0 0, 0 0, 0 0, 0 0, 0 0; } to { background-position: -83px 0, -107px 0, -131px 0, -157px 0, -191px 0, -223px 0, -269px 0, -311px 0; } }
  @keyframes model-popover-enter { from { opacity: 0; transform: translateY(var(--model-popover-enter-y, 0.25rem)); } to { opacity: 1; transform: translateY(0); } }
  @container chat-composer (max-width: 460px) { .model-trigger { max-width: 11rem; } .effort-name { display: none; } }
  @container chat-composer (max-width: 330px) { .model-trigger { max-width: 7.5rem; padding-inline: 0.45rem; } }
  @media (prefers-reduced-motion: reduce) { .picker-stage, .picker-view, .quick-actions, .effort-guidance, .model-company-content { transition-duration: 0.01ms; } .model-popover.portaled.positioned { animation: none; } .effort-fill::before, .effort-particles { animation: none; background-position: 50% 0; } }
</style>
