<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import {
    deleteProjectIconAssetsIfUnreferenced,
    downloadProjectIconImageUrl,
    pickProjectIconImageFile,
    projectIconAssetUrl,
    saveProjectIconImageDataUrl,
    type ProjectIconAsset,
  } from "$lib/api/project-icons";
  import type {
    IconPickerAsset,
    IconPickerTriggerContext,
    IconPickerUploadAdapter,
  } from "./types";
  import type { EventColor } from "$lib/calendar/types";
  import { getEventColor } from "$lib/calendar/utils";
  import { contrastRatio } from "$lib/color/math";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    inspectManagedImageFile,
    MANAGED_ICON_IMAGE_MAX_BYTES,
    MANAGED_ICON_IMAGE_MAX_MEGABYTES,
    MANAGED_IMAGE_MAX_DIMENSION_PIXELS,
    MANAGED_IMAGE_MAX_MEGAPIXELS,
    MANAGED_IMAGE_FILE_ACCEPT,
    normalizeManagedImageDataUrl,
  } from "$lib/browser-file-policy";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import {
    PROJECT_EMOJI_ENTRIES,
    type ProjectEmojiCategoryId,
  } from "$lib/projects/icons/emoji-catalog.generated";
  import {
    cleanupProjectIconRecentValues,
    filterProjectEmojiEntries,
    filterProjectLucideIcons,
    projectIconEmojiRecentValues,
    projectIconLucideRecentValues,
    prependProjectIconRecentValue,
    projectEmojiSkinToneFromEmoji,
    iconPickerAnchoredPanelPlacement,
    iconPickerGroupVirtualWindow,
    iconPickerIsPrimaryLucideCategory,
    iconPickerLucideCategoryOptions,
    iconPickerLucideGroups,
    iconPickerLucideRecentPreviewValue,
    iconPickerMenuPlacement,
    iconPickerPanelPlacement,
    iconPickerPointPlacement,
    iconPickerPrimaryLucideCategoryOptions,
    iconPickerRandomEmojiIcon,
    iconPickerRandomLucideIcon,
    iconPickerVisibleEmojiCategories,
    iconPickerVisibleCustomEmojis,
    readProjectIconAskEveryTime,
    readProjectIconDefaultColor,
    readProjectIconRecentValues,
    stripProjectEmojiSkinTone,
    type ProjectEmojiSkinTone,
    type IconPickerAnchoredPanelPlacement,
    type IconPickerColor,
    type IconPickerPointPlacement,
    type IconPickerRect,
  } from "$lib/projects/icons/picker";
  import {
    parseProjectIcon,
    projectIconDisplayLabel,
    serializeProjectIcon,
    type ProjectIconValue,
  } from "$lib/projects/icons/values";
  import type {
    ProjectLucideCategory,
    ProjectLucideIconEntry,
    ProjectLucideIconNode,
  } from "$lib/projects/icons/lucide-catalog.generated";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { resolveAppTokens, resolveCalendarTokens } from "$lib/themes";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import {
    ensureConfigLoaded,
    getConfigKey,
    setConfigKey,
  } from "$lib/vault/config";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import IconPickerCategoryMenu from "./IconPickerCategoryMenu.svelte";
  import IconPickerColorChoicePanel from "./IconPickerColorChoicePanel.svelte";
  import IconPickerCustomEmojiPanel from "./IconPickerCustomEmojiPanel.svelte";
  import IconPickerEmojiTab from "./IconPickerEmojiTab.svelte";
  import IconPickerIconsTab from "./IconPickerIconsTab.svelte";
  import IconPickerUploadPanel from "./IconPickerUploadPanel.svelte";

  type IconPickerTab = "emoji" | "icons" | "upload";
  type PickerInteractionSource = "keyboard" | "pointer";
  type IconColorChoice = {
    slug: string;
    label: string;
    iconNode: readonly ProjectLucideIconNode[] | null;
    anchor: HTMLElement;
    placement: IconPickerPointPlacement;
  };

  let {
    value,
    onChange,
    ariaLabel,
    trigger,
    uploadAdapter,
    showIcons = true,
    showEmoji = true,
    showUpload = true,
    showRemove = true,
    showIconColor = true,
    nested = false,
    initiallyOpen = false,
    panelAlign = "end",
    panelAnchor = null,
    class: className = "",
  }: {
    value: string;
    onChange: (value: string) => void;
    ariaLabel: string;
    trigger?: Snippet<[IconPickerTriggerContext]>;
    uploadAdapter?: IconPickerUploadAdapter;
    showIcons?: boolean;
    showEmoji?: boolean;
    showUpload?: boolean;
    showRemove?: boolean;
    /** Offer an icon color; without it, icons keep the automatic color and the saved default color is ignored. */
    showIconColor?: boolean;
    /** Keep the panels inside the nearest floating root, so the menu or panel that hosts the picker treats them as its own. */
    nested?: boolean;
    initiallyOpen?: boolean;
    panelAlign?: "start" | "end";
    /** Stable panel anchor when the action that opened the picker is removed. */
    panelAnchor?: HTMLElement | null;
    class?: string;
  } = $props();

  const pickerId = $props.id();
  const panelId = `${pickerId}-panel`;
  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const projects = getProjects();
  const theme = getTheme();
  const nativeFilePickerAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "storage.native-file-picker",
  );
  const remoteImageUrlsAvailable = $derived(platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    uploadAdapter?.selectExternalUrl
      ? "notes.external-image-references"
      : "content.managed-image-downloads",
  ));
  const recentConfigKey = "projects.iconPicker.recent";
  const askEveryTimeConfigKey = "projects.iconPicker.askEveryTime";
  const defaultColorConfigKey = "projects.iconPicker.defaultColor";
  const panelWidth = 360;
  const panelPreferredHeight = 440;
  const uploadPanelPreferredHeight = 228;
  const uploadDraftPanelPreferredHeight = 276;
  const uploadWarningHeight = 72;
  const customPanelWidth = 330;
  const iconColorChoicePanelColumns = 4;
  const iconColorChoicePanelWidthRem = 8.25;
  const iconColorChoicePanelHeightRem = 18.05;
  const iconCategoryMenuWidth = FLOATING_WIDTH.sm;
  const iconCategoryMenuMaxHeight = 280;
  const gridCellSize = 36;
  const gridRowHeight = 36;
  const gridGroupHeaderHeight = 24;
  const gridGroupGapHeight = 12;

  let open = $state(false);
  let activeTab = $state<IconPickerTab>("icons");
  let pickerInteractionSource: PickerInteractionSource = "pointer";
  let triggerElement = $state<HTMLElement | undefined>();
  const portalTarget = $derived<HTMLElement | string>(
    nested ? (panelAnchor ?? triggerElement)?.closest<HTMLElement>("[data-floating-root]") ?? "body" : "body",
  );
  let uploadFileInput = $state<HTMLInputElement | undefined>();
  let customEmojiFileInput = $state<HTMLInputElement | undefined>();
  let panelElement = $state<HTMLElement | undefined>();
  let customPanelElement = $state<HTMLElement | undefined>();
  let customEmojiTriggerElement = $state<HTMLButtonElement | undefined>();
  let iconColorChoicePanelElement = $state<HTMLElement | undefined>();
  let iconCategoryMenuElement = $state<HTMLElement | undefined>();
  let iconCategoryMenuTriggerElement = $state<HTMLButtonElement | undefined>();
  let gridScrollElement = $state<HTMLElement | undefined>();
  let panelPlacement = $state({ left: 0, top: 0, width: panelWidth, height: panelPreferredHeight });
  let customPanelPlacement = $state<IconPickerAnchoredPanelPlacement>({ left: 0, top: 0, maxHeight: 360 });
  let iconCategoryMenuPlacement = $state<IconPickerAnchoredPanelPlacement>({
    left: 0,
    top: 0,
    maxHeight: iconCategoryMenuMaxHeight,
  });
  let emojiQuery = $state("");
  let iconQuery = $state("");
  let uploadUrl = $state("");
  let uploadDraft = $state<IconPickerAsset | null>(null);
  let uploadPreviewUrl = $state<string | null>(null);
  let uploadPreviewRequestId = 0;
  let uploadError = $state<string | null>(null);
  let uploading = $state(false);
  let iconColor = $state<IconPickerColor>("default");
  let emojiSkinTone = $state<ProjectEmojiSkinTone>("default");
  let emojiCategory = $state<ProjectEmojiCategoryId | "all">("all");
  let iconCategory = $state<ProjectLucideCategory | "all">("all");
  let lucideIcons = $state<readonly ProjectLucideIconEntry[]>([]);
  let lucideCategories = $state<readonly ProjectLucideCategory[]>([]);
  let lucideLoaded = $state(false);
  let lucideLoading = $state(false);
  let customEmojiPanelOpen = $state(false);
  let customEmojiName = $state("");
  let customEmojiDraft = $state<ProjectIconAsset | null>(null);
  let customEmojiError = $state<string | null>(null);
  let customEmojiSaving = $state(false);
  let skinTonePanelOpen = $state(false);
  let iconColorPanelOpen = $state(false);
  let iconCategoryMenuOpen = $state(false);
  let askIconColorEveryTime = $state(false);
  let iconColorChoice = $state<IconColorChoice | null>(null);
  let recentValues = $state<string[]>([]);
  let gridScrollTop = $state(0);
  let gridScrollable = $state(false);
  let gridCanScrollUp = $state(false);
  let gridCanScrollDown = $state(false);
  let gridViewportHeight = $state(260);
  let gridColumnCount = $state(8);
  let gridScrollStateFrame: number | null = null;

  const parsedValue = $derived(parseProjectIcon(value));
  const visibleTabs = $derived.by((): IconPickerTab[] => {
    const tabs: IconPickerTab[] = [];
    if (showIcons) tabs.push("icons");
    if (showEmoji || !tabs.length) tabs.push("emoji");
    if (showUpload) tabs.push("upload");
    return tabs;
  });
  const customEmojiIds = $derived(new Set(projects.customEmojis.map((emoji) => emoji.id)));
  $effect(() => {
    if (!visibleTabs.includes("emoji")) return;
    void projects.ensureCustomEmojis().catch((error) => {
      console.error("load icon picker custom emoji failed", error);
    });
  });
  const pickerLabel = $derived.by(() => {
    if (parsedValue.kind === "emoji") {
      const baseEmoji = stripProjectEmojiSkinTone(parsedValue.emoji);
      return PROJECT_EMOJI_ENTRIES.find((entry) => entry.emoji === baseEmoji)?.name
        ?? t("projects.iconPicker.emoji");
    }
    if (parsedValue.kind === "custom-emoji") {
      return projects.customEmojis.find((emoji) => emoji.id === parsedValue.id)?.name ?? projectIconDisplayLabel(parsedValue);
    }
    return projectIconDisplayLabel(parsedValue);
  });
  const emojiRecentValues = $derived(
    projectIconEmojiRecentValues(recentValues),
  );
  const lucideRecentValues = $derived(projectIconLucideRecentValues(recentValues));
  const visibleCustomEmojis = $derived(iconPickerVisibleCustomEmojis(projects.customEmojis, emojiQuery));
  const visibleEmojiCategories = $derived(
    iconPickerVisibleEmojiCategories(t("projects.iconPicker.symbolsAndFlags")),
  );
  const emojiGroups = $derived.by(() =>
    visibleEmojiCategories
      .filter((category) => emojiCategory === "all" || emojiCategory === category.id)
      .map((category) => ({
        category,
        entries: filterProjectEmojiEntries(PROJECT_EMOJI_ENTRIES, emojiQuery, category.id),
      }))
      .filter((group) => group.entries.length > 0)
  );
  const filteredEmojiEntries = $derived(
    filterProjectEmojiEntries(PROJECT_EMOJI_ENTRIES, emojiQuery, emojiCategory),
  );
  const filteredLucideEntries = $derived(
    filterProjectLucideIcons(lucideIcons, iconQuery, iconCategory),
  );
  const lucideGroups = $derived(iconPickerLucideGroups(filteredLucideEntries, lucideCategories));
  const lucideCategoryOptions = $derived(
    iconPickerLucideCategoryOptions(lucideCategories, lucideIcons),
  );
  const primaryLucideCategoryOptions = $derived(
    iconPickerPrimaryLucideCategoryOptions(lucideCategoryOptions),
  );
  const iconCategoryOverflowActive = $derived(
    iconCategory !== "all" && !iconPickerIsPrimaryLucideCategory(iconCategory),
  );
  const lucideRecentHeight = $derived.by(() => {
    if (lucideRecentValues.length === 0) return 0;
    return gridGroupHeaderHeight
      + (Math.ceil(lucideRecentValues.length / Math.max(1, gridColumnCount)) * gridRowHeight)
      + gridGroupGapHeight;
  });
  const lucideGroupVirtual = $derived(iconPickerGroupVirtualWindow({
    groups: lucideGroups,
    columnCount: gridColumnCount,
    viewportHeight: gridViewportHeight,
    scrollTop: Math.max(0, gridScrollTop - lucideRecentHeight),
    rowHeight: gridRowHeight,
    groupHeaderHeight: gridGroupHeaderHeight,
    groupGapHeight: gridGroupGapHeight,
  }));
  const calendarTokens = $derived(resolveCalendarTokens(theme.current));
  const appTokens = $derived(resolveAppTokens(theme.current));
  const pickerBg = $derived(calendarTokens["--cal-bg"]);
  const pickerText = $derived(calendarTokens["--cal-time-label"]);
  const automaticIconColor = $derived(appTokens["--foreground"]);
  const pickerSurfaceStyle = $derived(
    `background-color: ${pickerBg}; color: ${pickerText}; --icon-picker-bg: ${pickerBg}; --icon-picker-text: ${pickerText};`,
  );
  const panelStyle = $derived.by(() => {
    const baseStyle = `left: ${panelPlacement.left}px; top: ${panelPlacement.top}px; width: ${panelPlacement.width}px; ${pickerSurfaceStyle}`;
    if (activeTab === "upload") return `${baseStyle} max-height: ${panelPlacement.height}px;`;
    return `${baseStyle} height: ${panelPlacement.height}px;`;
  });
  const uploadBodyStyle = $derived(`max-height: ${Math.max(0, panelPlacement.height - 48)}px;`);
  const colorSelectionBorder = $derived(
    contrastRatio(pickerBg, "#000000") >= contrastRatio(pickerBg, "#ffffff")
      ? "#000000"
      : "#ffffff",
  );
  const customPanelStyle = $derived(
    `left: ${customPanelPlacement.left}px; top: ${customPanelPlacement.top}px; width: ${customPanelWidth}px; max-height: ${customPanelPlacement.maxHeight}px; ${pickerSurfaceStyle}`,
  );
  const iconCategoryMenuStyle = $derived(
    `left: ${iconCategoryMenuPlacement.left}px; top: ${iconCategoryMenuPlacement.top}px; width: ${iconCategoryMenuWidth}px; max-height: ${iconCategoryMenuPlacement.maxHeight}px; ${pickerSurfaceStyle}`,
  );

  function tabLabel(tab: IconPickerTab): string {
    if (tab === "icons") return t("projects.iconPicker.icons");
    if (tab === "upload") return t("projects.iconPicker.upload");
    return t("projects.iconPicker.emoji");
  }

  function iconColorLabel(color: IconPickerColor): string {
    return color === "default"
      ? t("projects.iconPicker.automaticColor")
      : t("projects.iconPicker.selectIconColor", color + 1);
  }

  function iconColorSwatch(color: EventColor): string {
    return getEventColor(color, theme.current).bg;
  }

  function iconColorStyle(color: EventColor): string {
    return `color: ${iconColorSwatch(color)};`;
  }

  function rootRemPx(): number {
    const fontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
    return Number.isFinite(fontSize) && fontSize > 0 ? fontSize : 16;
  }

  function iconColorChoicePanelSize(): { width: number; height: number } {
    const rem = rootRemPx();
    return {
      width: iconColorChoicePanelWidthRem * rem,
      height: iconColorChoicePanelHeightRem * rem,
    };
  }

  function lucideRecentPreviewValue(rawValue: string): string {
    return iconPickerLucideRecentPreviewValue({ rawValue, iconColor });
  }

  function iconColorChoiceStyle(choice: IconColorChoice): string {
    const { width } = iconColorChoicePanelSize();
    return `left: ${choice.placement.left}px; top: ${choice.placement.top}px; width: ${width}px; max-height: calc(100vh - 1rem); ${pickerSurfaceStyle}`;
  }

  function resetGridScroll(): void {
    gridScrollTop = 0;
    if (gridScrollElement) {
      gridScrollElement.scrollTop = 0;
      refreshGridScrollState();
    } else {
      requestGridScrollStateRefresh();
    }
  }

  function closeInlinePanels(): void {
    skinTonePanelOpen = false;
    iconColorPanelOpen = false;
    iconCategoryMenuOpen = false;
    iconColorChoice = null;
  }

  function handlePanelPointerDown(event: PointerEvent): void {
    pickerInteractionSource = "pointer";
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.closest("[data-icon-picker-inline-panel]")) return;
    closeInlinePanels();
  }

  async function loadPickerState(): Promise<void> {
    await ensureConfigLoaded();
    recentValues = cleanupProjectIconRecentValues(
      readProjectIconRecentValues(getConfigKey<unknown>(recentConfigKey, [])),
      customEmojiIds,
    );
    if (!showIconColor) return;
    askIconColorEveryTime = readProjectIconAskEveryTime(
      getConfigKey<unknown>(askEveryTimeConfigKey, false),
    );
    iconColor = readProjectIconDefaultColor(
      getConfigKey<unknown>(defaultColorConfigKey, "default"),
    );
  }

  function saveRecentValues(values: string[]): void {
    recentValues = cleanupProjectIconRecentValues(values, customEmojiIds);
    setConfigKey(recentConfigKey, recentValues);
  }

  function setAskIconColorEveryTime(enabled: boolean): void {
    askIconColorEveryTime = enabled;
    setConfigKey(askEveryTimeConfigKey, enabled);
    if (!enabled) iconColorChoice = null;
  }

  function openDefaultIconColorPanel(): void {
    iconColorChoice = null;
  }

  function updateGridMetrics(): void {
    const element = gridScrollElement;
    if (!element) return;
    gridViewportHeight = element.clientHeight;
    gridColumnCount = Math.max(1, Math.floor(element.clientWidth / gridCellSize));
    requestGridScrollStateRefresh();
  }

  function refreshGridScrollState(): void {
    gridScrollStateFrame = null;
    const element = gridScrollElement;
    if (!element) {
      gridScrollable = false;
      gridCanScrollUp = false;
      gridCanScrollDown = false;
      return;
    }
    const maxScrollTop = element.scrollHeight - element.clientHeight;
    gridScrollable = maxScrollTop > 1;
    gridCanScrollUp = element.scrollTop > 1;
    gridCanScrollDown = element.scrollTop < maxScrollTop - 1;
  }

  function requestGridScrollStateRefresh(): void {
    if (gridScrollStateFrame !== null) cancelAnimationFrame(gridScrollStateFrame);
    gridScrollStateFrame = requestAnimationFrame(refreshGridScrollState);
  }

  function preferredPanelHeight(): number {
    if (activeTab !== "upload") return panelPreferredHeight;
    const baseHeight = uploadDraft ? uploadDraftPanelPreferredHeight : uploadPanelPreferredHeight;
    return uploadError ? baseHeight + uploadWarningHeight : baseHeight;
  }

  function toPickerRect(rect: DOMRect): IconPickerRect {
    return {
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
      left: rect.left,
      width: rect.width,
      height: rect.height,
    };
  }

  function viewportBoundaryRect(): IconPickerRect {
    return {
      top: 0,
      right: window.innerWidth,
      bottom: window.innerHeight,
      left: 0,
      width: window.innerWidth,
      height: window.innerHeight,
    };
  }

  function panelBoundaryRect(trigger: HTMLElement): IconPickerRect {
    const viewport = viewportBoundaryRect();
    const settingsContent = trigger.closest<HTMLElement>("[data-settings-content]");
    if (!settingsContent) return viewport;
    const rect = settingsContent.getBoundingClientRect();
    const top = Math.max(viewport.top, rect.top);
    const bottom = Math.min(viewport.bottom, rect.bottom);
    return {
      top,
      right: viewport.right,
      bottom,
      left: viewport.left,
      width: viewport.width,
      height: Math.max(0, bottom - top),
    };
  }

  async function refreshPanelPlacement(): Promise<void> {
    await tick();
    placePanel();
  }

  function placePanel(): void {
    const trigger = panelAnchor ?? triggerElement;
    if (!trigger) return;
    panelPlacement = iconPickerPanelPlacement({
      triggerRect: toPickerRect(trigger.getBoundingClientRect()),
      boundaryRect: panelBoundaryRect(trigger),
      preferredWidth: panelWidth,
      preferredHeight: preferredPanelHeight(),
      align: panelAlign,
    });
  }

  function placeCustomPanel(): void {
    const trigger = customEmojiTriggerElement;
    if (!trigger) return;
    customPanelPlacement = iconPickerAnchoredPanelPlacement({
      anchorRect: toPickerRect(trigger.getBoundingClientRect()),
      viewportRect: viewportBoundaryRect(),
      panelWidth: customPanelWidth,
      preferredHeight: 360,
    });
  }

  function placeIconCategoryMenu(): void {
    const trigger = iconCategoryMenuTriggerElement;
    if (!trigger) return;
    iconCategoryMenuPlacement = iconPickerMenuPlacement({
      anchorRect: toPickerRect(trigger.getBoundingClientRect()),
      viewportRect: viewportBoundaryRect(),
      menuWidth: iconCategoryMenuWidth,
      menuMaxHeight: iconCategoryMenuMaxHeight,
    });
  }

  function toggleIconCategoryMenu(): void {
    iconCategoryMenuOpen = !iconCategoryMenuOpen;
    skinTonePanelOpen = false;
    iconColorPanelOpen = false;
    iconColorChoice = null;
    if (iconCategoryMenuOpen) {
      void tick().then(placeIconCategoryMenu);
    }
  }

  function selectIconCategory(category: ProjectLucideCategory | "all"): void {
    iconCategory = category;
    iconCategoryMenuOpen = false;
    resetGridScroll();
  }

  function placeIconColorChoice(anchor: HTMLElement): IconPickerPointPlacement {
    const { width, height } = iconColorChoicePanelSize();
    return iconPickerPointPlacement({
      anchorRect: toPickerRect(anchor.getBoundingClientRect()),
      viewportRect: viewportBoundaryRect(),
      panelWidth: width,
      panelHeight: height,
    });
  }

  function updateIconColorChoicePlacement(): void {
    const choice = iconColorChoice;
    if (!choice) return;
    if (!document.contains(choice.anchor)) {
      iconColorChoice = null;
      return;
    }
    iconColorChoice = {
      ...choice,
      placement: placeIconColorChoice(choice.anchor),
    };
  }

  async function loadLucideCatalog(): Promise<void> {
    if (lucideLoaded || lucideLoading) return;
    lucideLoading = true;
    try {
      const catalog = await import("$lib/projects/icons/lucide-catalog.generated");
      lucideCategories = catalog.PROJECT_LUCIDE_CATEGORIES;
      lucideIcons = catalog.PROJECT_LUCIDE_ICONS;
      lucideLoaded = true;
    } finally {
      lucideLoading = false;
    }
  }

  function focusActivePickerTab(): void {
    const tab = panelElement?.querySelector<HTMLButtonElement>(
      `[data-icon-picker-tab="${activeTab}"]`,
    );
    if (tab) tab.focus();
    else panelElement?.focus();
  }

  async function openPicker(): Promise<void> {
    if (!visibleTabs.includes(activeTab)) activeTab = visibleTabs[0] ?? "emoji";
    placePanel();
    open = true;
    if (parsedValue.kind === "emoji") emojiSkinTone = projectEmojiSkinToneFromEmoji(parsedValue.emoji);
    await tick();
    if (!open) return;
    placePanel();
    updateGridMetrics();
    focusActivePickerTab();
    await loadPickerState();
    if (!open) return;
    if (activeTab === "icons") await loadLucideCatalog();
    if (!open) return;
    await tick();
    if (!open) return;
    placePanel();
    updateGridMetrics();
  }

  function focusPickerTrigger(): void {
    const element = panelAnchor ?? triggerElement;
    if (!element) return;
    const selector = [
      "button:not([disabled])",
      "a[href]",
      "input:not([disabled])",
      "select:not([disabled])",
      "textarea:not([disabled])",
      '[tabindex]:not([tabindex="-1"])',
    ].join(",");
    const focusTarget = element.matches(selector)
      ? element
      : element.querySelector<HTMLElement>(selector);
    focusTarget?.focus();
  }

  function closePicker(source: PickerInteractionSource = "pointer"): void {
    open = false;
    customEmojiPanelOpen = false;
    skinTonePanelOpen = false;
    iconColorPanelOpen = false;
    iconCategoryMenuOpen = false;
    iconColorChoice = null;
    uploadError = null;
    customEmojiError = null;
    if (source === "keyboard") {
      void tick().then(focusPickerTrigger);
    }
  }

  async function setTab(tab: IconPickerTab): Promise<void> {
    activeTab = tab;
    skinTonePanelOpen = false;
    iconColorPanelOpen = false;
    iconCategoryMenuOpen = false;
    customEmojiPanelOpen = false;
    iconColorChoice = null;
    resetGridScroll();
    placePanel();
    if (tab === "icons") await loadLucideCatalog();
    await tick();
    placePanel();
    updateGridMetrics();
  }

  async function focusTab(index: number): Promise<void> {
    const tab = visibleTabs[index];
    if (!tab) return;
    await setTab(tab);
    await tick();
    panelElement?.querySelector<HTMLButtonElement>(`[data-icon-picker-tab="${tab}"]`)?.focus();
  }

  function handleTabKeydown(event: KeyboardEvent, index: number): void {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    let nextIndex = index;
    if (event.key === "ArrowLeft") nextIndex = (index - 1 + visibleTabs.length) % visibleTabs.length;
    else if (event.key === "ArrowRight") nextIndex = (index + 1) % visibleTabs.length;
    else if (event.key === "Home") nextIndex = 0;
    else if (event.key === "End") nextIndex = visibleTabs.length - 1;
    else return;
    event.preventDefault();
    event.stopPropagation();
    void focusTab(nextIndex);
  }

  function chooseIcon(icon: ProjectIconValue, closeAfterSelect = true): void {
    onChange(serializeProjectIcon(icon));
    const nextRecent = prependProjectIconRecentValue(recentValues, icon, customEmojiIds);
    saveRecentValues(nextRecent);
    if (closeAfterSelect) closePicker(pickerInteractionSource);
  }

  function openIconColorChoice(
    slug: string,
    label: string,
    iconNode: readonly ProjectLucideIconNode[] | null,
    target: EventTarget | null,
  ): void {
    const anchor = target instanceof HTMLElement ? target : null;
    if (!anchor) {
      chooseIcon({ kind: "lucide", slug, color: iconColor });
      return;
    }
    skinTonePanelOpen = false;
    iconColorPanelOpen = false;
    customEmojiPanelOpen = false;
    iconColorChoice = {
      slug,
      label,
      iconNode,
      anchor,
      placement: placeIconColorChoice(anchor),
    };
  }

  function chooseLucideIcon(
    slug: string,
    label: string,
    iconNode: readonly ProjectLucideIconNode[] | null,
    target: EventTarget | null,
  ): void {
    if (askIconColorEveryTime) {
      openIconColorChoice(slug, label, iconNode, target);
      return;
    }
    chooseIcon({ kind: "lucide", slug, color: iconColor });
  }

  function chooseRecent(rawValue: string, target: EventTarget | null = null): void {
    const icon = parseProjectIcon(rawValue);
    if (icon.kind === "lucide") {
      const entry = lucideIcons.find((candidate) => candidate.slug === icon.slug);
      chooseLucideIcon(icon.slug, entry?.label ?? projectIconDisplayLabel(icon), entry?.iconNode ?? null, target);
      return;
    }
    chooseIcon(icon);
  }

  function chooseRandomEmoji(): void {
    const icon = iconPickerRandomEmojiIcon(filteredEmojiEntries, emojiSkinTone);
    if (icon) chooseIcon(icon);
  }

  function chooseRandomIcon(): void {
    const icon = iconPickerRandomLucideIcon(filteredLucideEntries, { iconColor });
    if (icon) chooseIcon(icon);
  }

  function handleGridScroll(): void {
    gridScrollTop = gridScrollElement?.scrollTop ?? 0;
    updateGridMetrics();
    updateIconColorChoicePlacement();
    refreshGridScrollState();
  }

  async function fileToDataUrl(file: File): Promise<string> {
    const inspection = await inspectManagedImageFile(file, MANAGED_ICON_IMAGE_MAX_BYTES);
    if (!inspection.ok) {
      const { issue } = inspection;
      if (issue === "unsupported-type") {
        return Promise.reject(new Error(t("projects.iconPicker.uploadUnsupportedType")));
      }
      if (issue === "too-large") {
        return Promise.reject(new Error(t(
          "projects.iconPicker.uploadTooLarge",
          MANAGED_ICON_IMAGE_MAX_MEGABYTES,
        )));
      }
      if (issue === "invalid-image") {
        return Promise.reject(new Error(t("projects.iconPicker.uploadInvalidImage")));
      }
      if (issue === "dimensions-too-large") {
        return Promise.reject(new Error(t(
          "projects.iconPicker.uploadDimensionsTooLarge",
          MANAGED_IMAGE_MAX_DIMENSION_PIXELS,
        )));
      }
      if (issue === "too-many-pixels") {
        return Promise.reject(new Error(t(
          "projects.iconPicker.uploadPixelCountTooLarge",
          MANAGED_IMAGE_MAX_MEGAPIXELS,
        )));
      }
      return Promise.reject(new Error(t("projects.iconPicker.uploadFailed")));
    }
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(new Error(t("projects.iconPicker.uploadFailed")));
      reader.onload = () => {
        if (typeof reader.result === "string") {
          const dataUrl = normalizeManagedImageDataUrl(
            reader.result,
            inspection.metadata.mimeType,
          );
          if (dataUrl) {
            resolve(dataUrl);
            return;
          }
          reject(new Error(t("projects.iconPicker.uploadFailed")));
        } else {
          reject(new Error(t("projects.iconPicker.uploadFailed")));
        }
      };
      reader.readAsDataURL(file);
    });
  }

  async function saveFileAsProjectIcon(file: File): Promise<ProjectIconAsset> {
    const dataUrl = await fileToDataUrl(file);
    return saveProjectIconImageDataUrl(dataUrl);
  }

  async function saveFileAsUploadAsset(file: File): Promise<IconPickerAsset> {
    const dataUrl = await fileToDataUrl(file);
    return uploadAdapter
      ? uploadAdapter.saveImageDataUrl(dataUrl, file.name)
      : saveProjectIconImageDataUrl(dataUrl);
  }

  async function chooseUploadFile(): Promise<void> {
    if (!nativeFilePickerAvailable) {
      uploadFileInput?.click();
      return;
    }
    uploading = true;
    uploadError = null;
    try {
      const asset = uploadAdapter
        ? await uploadAdapter.pickImageFile()
        : await pickProjectIconImageFile();
      if (asset && uploadAdapter?.selectPickedAssetImmediately) {
        await uploadAdapter.selectAsset(asset);
        closePicker(pickerInteractionSource);
      } else {
        uploadDraft = asset;
      }
    } catch (error) {
      uploadError = error instanceof Error ? error.message : String(error);
    } finally {
      uploading = false;
      await refreshPanelPlacement();
    }
  }

  async function saveUploadFile(file: File): Promise<void> {
    uploading = true;
    uploadError = null;
    try {
      const asset = await saveFileAsUploadAsset(file);
      if (uploadAdapter?.selectPickedAssetImmediately) {
        await uploadAdapter.selectAsset(asset);
        closePicker(pickerInteractionSource);
      } else {
        uploadDraft = asset;
      }
    } catch (error) {
      uploadError = error instanceof Error ? error.message : String(error);
    } finally {
      uploading = false;
      await refreshPanelPlacement();
    }
  }

  async function handleUploadFileInput(event: Event): Promise<void> {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    const file = input.files?.[0];
    input.value = "";
    if (file) await saveUploadFile(file);
  }

  async function downloadUploadUrl(): Promise<void> {
    const url = uploadUrl.trim();
    if (!url) return;
    uploading = true;
    uploadError = null;
    try {
      if (uploadAdapter?.selectExternalUrl) {
        await uploadAdapter.selectExternalUrl(url);
        uploadUrl = "";
        closePicker(pickerInteractionSource);
      } else {
        uploadDraft = uploadAdapter?.downloadImageUrl
          ? await uploadAdapter.downloadImageUrl(url)
          : await downloadProjectIconImageUrl(url);
      }
    } catch (error) {
      uploadError = error instanceof Error ? error.message : String(error);
    } finally {
      uploading = false;
      await refreshPanelPlacement();
    }
  }

  async function handleUploadPaste(event: ClipboardEvent): Promise<void> {
    const file = event.clipboardData?.files[0];
    if (!file) return;
    event.preventDefault();
    await saveUploadFile(file);
  }

  async function selectUploadDraft(): Promise<void> {
    if (!uploadDraft) return;
    if (uploadAdapter) {
      await uploadAdapter.selectAsset(uploadDraft);
      closePicker(pickerInteractionSource);
    } else {
      chooseIcon({ kind: "asset", relativePath: uploadDraft.relativePath });
    }
    uploadDraft = null;
    uploadUrl = "";
  }

  async function chooseCustomEmojiFile(): Promise<void> {
    if (!nativeFilePickerAvailable) {
      customEmojiFileInput?.click();
      return;
    }
    customEmojiError = null;
    try {
      customEmojiDraft = await pickProjectIconImageFile();
    } catch (error) {
      customEmojiError = error instanceof Error ? error.message : String(error);
    }
  }

  async function saveCustomEmojiFile(file: File): Promise<void> {
    customEmojiError = null;
    try {
      customEmojiDraft = await saveFileAsProjectIcon(file);
    } catch (error) {
      customEmojiError = error instanceof Error ? error.message : String(error);
    }
  }

  async function handleCustomEmojiFileInput(event: Event): Promise<void> {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    const file = input.files?.[0];
    input.value = "";
    if (file) await saveCustomEmojiFile(file);
  }

  async function handleCustomEmojiPaste(event: ClipboardEvent): Promise<void> {
    const file = event.clipboardData?.files[0];
    if (!file) return;
    event.preventDefault();
    await saveCustomEmojiFile(file);
  }

  async function saveCustomEmoji(): Promise<void> {
    if (!customEmojiDraft) return;
    const name = customEmojiName.trim();
    if (!name) {
      customEmojiError = t("projects.iconPicker.nameRequired");
      return;
    }
    customEmojiSaving = true;
    customEmojiError = null;
    try {
      const customEmoji = await projects.addCustomEmoji(name, customEmojiDraft.relativePath);
      if (customEmoji) chooseIcon({ kind: "custom-emoji", id: customEmoji.id });
      customEmojiName = "";
      customEmojiDraft = null;
      customEmojiPanelOpen = false;
    } catch (error) {
      customEmojiError = error instanceof Error ? error.message : String(error);
    } finally {
      customEmojiSaving = false;
    }
  }

  function removeIcon(): void {
    onChange("none");
    closePicker(pickerInteractionSource);
  }

  function selectIconColor(color: IconPickerColor): void {
    iconColor = color;
    setConfigKey(defaultColorConfigKey, color === "default" ? undefined : color);
    if (parsedValue.kind === "lucide") {
      onChange(serializeProjectIcon({ ...parsedValue, color }));
    }
  }

  function selectColorChoice(color: IconPickerColor): void {
    const choice = iconColorChoice;
    if (!choice) return;
    chooseIcon({ kind: "lucide", slug: choice.slug, color });
  }

  async function discardUploadDraft(): Promise<void> {
    const draft = uploadDraft;
    uploadDraft = null;
    await tick();
    placePanel();
    if (!draft) return;
    if (uploadAdapter?.deleteAssetsIfUnreferenced) {
      await uploadAdapter.deleteAssetsIfUnreferenced([draft.relativePath]);
    } else if (!uploadAdapter) {
      await deleteProjectIconAssetsIfUnreferenced([draft.relativePath]);
    }
  }

  function togglePicker(): void {
    if (open) closePicker();
    else void openPicker();
  }

  onMount(() => {
    if (initiallyOpen) void openPicker();
  });

  $effect(() => {
    if (!open) return;
    return mobileBackStack.activate({ handle: closePicker });
  });

  $effect(() => {
    if (!visibleTabs.includes(activeTab)) activeTab = visibleTabs[0] ?? "emoji";
  });

  $effect(() => {
    const draft = uploadDraft;
    const requestId = ++uploadPreviewRequestId;
    uploadPreviewUrl = null;
    if (!draft) return;
    const request = uploadAdapter
      ? uploadAdapter.assetUrl(draft)
      : projectIconAssetUrl(draft.relativePath);
    void request
      .then((url) => {
        if (requestId === uploadPreviewRequestId) uploadPreviewUrl = url;
      })
      .catch(() => {
        if (requestId === uploadPreviewRequestId) uploadPreviewUrl = null;
      });
  });

  $effect(() => {
    if (!open) return;
    const resize = () => {
      placePanel();
      placeCustomPanel();
      placeIconCategoryMenu();
      updateIconColorChoicePlacement();
      updateGridMetrics();
    };
    const outside = (event: PointerEvent) => {
      pickerInteractionSource = "pointer";
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (triggerElement?.contains(target)) return;
      if (panelElement?.contains(target)) return;
      if (customPanelElement?.contains(target)) return;
      if (iconColorChoicePanelElement?.contains(target)) return;
      if (iconCategoryMenuElement?.contains(target)) return;
      closePicker();
    };
    const recordKeyboardInteraction = () => {
      pickerInteractionSource = "keyboard";
    };
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closePicker("keyboard");
      }
    };
    window.addEventListener("resize", resize);
    window.addEventListener("scroll", resize, true);
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", recordKeyboardInteraction, true);
    document.addEventListener("keydown", keydown);
    return () => {
      window.removeEventListener("resize", resize);
      window.removeEventListener("scroll", resize, true);
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("keydown", recordKeyboardInteraction, true);
      document.removeEventListener("keydown", keydown);
      if (gridScrollStateFrame !== null) {
        cancelAnimationFrame(gridScrollStateFrame);
        gridScrollStateFrame = null;
      }
    };
  });

  $effect(() => {
    if (!open) return;
    const element = gridScrollElement;
    if (!element) return;
    const observer = new ResizeObserver(updateGridMetrics);
    observer.observe(element);
    updateGridMetrics();
    return () => observer.disconnect();
  });

  $effect(() => {
    if (!open) return;
    activeTab;
    emojiQuery;
    iconQuery;
    emojiCategory;
    iconCategory;
    emojiRecentValues.length;
    visibleCustomEmojis.length;
    emojiGroups.length;
    filteredLucideEntries.length;
    lucideGroups.length;
    lucideRecentHeight;
    lucideGroupVirtual.beforeHeight;
    lucideGroupVirtual.afterHeight;
    gridColumnCount;
    void tick().then(() => {
      updateGridMetrics();
      requestGridScrollStateRefresh();
    });
  });

  $effect(() => {
    if (!customEmojiPanelOpen) return;
    void tick().then(placeCustomPanel);
  });
</script>

{#if trigger}
  <span bind:this={triggerElement} class="inline-flex">
    {@render trigger({ open, toggle: togglePicker, panelId })}
  </span>
{:else}
  <button
    bind:this={triggerElement}
    type="button"
    class={cn(
      "flex h-8 min-w-0 items-center justify-between gap-2 rounded-md border border-border bg-card px-2.5 text-left text-[0.8rem] font-medium text-foreground hover:bg-accent/60",
      className,
    )}
    aria-label={ariaLabel}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-controls={panelId}
    onclick={togglePicker}
  >
    <span class="flex min-w-0 items-center gap-2">
      <ProjectIcon name={value} size={15} strokeWidth={1.8} class="shrink-0" />
      <span class="truncate">{pickerLabel}</span>
    </span>
    <ChevronDown
      size={13}
      strokeWidth={2}
      class={cn("shrink-0 text-muted-foreground transition-transform", open && "rotate-180")}
    />
  </button>
{/if}

<input
  bind:this={uploadFileInput}
  class="sr-only"
  type="file"
  accept={MANAGED_IMAGE_FILE_ACCEPT}
  aria-hidden="true"
  tabindex="-1"
  onchange={(event) => { void handleUploadFileInput(event); }}
/>
<input
  bind:this={customEmojiFileInput}
  class="sr-only"
  type="file"
  accept={MANAGED_IMAGE_FILE_ACCEPT}
  aria-hidden="true"
  tabindex="-1"
  onchange={(event) => { void handleCustomEmojiFileInput(event); }}
/>

{#if open}
  <div
    id={panelId}
    bind:this={panelElement}
    use:portal={portalTarget}
    class="surface-floating fixed z-90 flex min-h-0 flex-col overflow-hidden"
    style={panelStyle}
    role="dialog"
    data-app-floating-surface
    aria-label={ariaLabel}
    tabindex="-1"
    onpaste={activeTab === "upload" ? handleUploadPaste : undefined}
    onpointerdown={handlePanelPointerDown}
  >
    <div class="flex h-12 shrink-0 items-center justify-between border-b border-border/70 px-3">
      <div class="flex min-w-0 items-center gap-3" role="tablist" aria-label={ariaLabel}>
        {#each visibleTabs as tab, index}
          <button
            type="button"
            id={`${pickerId}-tab-${tab}`}
            role="tab"
            aria-selected={activeTab === tab}
            aria-controls={`${pickerId}-tabpanel-${tab}`}
            tabindex={activeTab === tab ? 0 : -1}
            data-icon-picker-tab={tab}
            class={cn(
              "h-10 border-b-2 px-0.5 text-[0.866667rem] transition-colors",
              activeTab === tab
                ? "border-foreground text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground",
            )}
            onclick={() => void setTab(tab)}
            onkeydown={(event) => handleTabKeydown(event, index)}
          >
            {tabLabel(tab)}
          </button>
        {/each}
      </div>
      {#if showRemove}
        <button
          type="button"
          class="h-9 px-1 text-[0.866667rem] text-muted-foreground hover:text-foreground"
          onclick={removeIcon}
        >
          {t("projects.iconPicker.remove")}
        </button>
      {/if}
    </div>

    {#each visibleTabs as tab}
      <div
        id={`${pickerId}-tabpanel-${tab}`}
        role="tabpanel"
        aria-labelledby={`${pickerId}-tab-${tab}`}
        hidden={activeTab !== tab}
        class={cn(
          "min-h-0 flex-1 flex-col overflow-hidden",
          activeTab === tab ? "flex" : "hidden",
        )}
      >
        {#if activeTab === tab}
          {#if tab === "emoji"}
            <IconPickerEmojiTab
              bind:scrollElement={gridScrollElement}
              bind:customEmojiTriggerElement={customEmojiTriggerElement}
              bind:emojiQuery
              bind:emojiCategory
              bind:emojiSkinTone
              bind:skinTonePanelOpen
              bind:iconColorPanelOpen
              bind:customEmojiPanelOpen
              allowCustomEmojiCreate={showUpload}
              {gridScrollable}
              {gridCanScrollUp}
              {gridCanScrollDown}
              {gridColumnCount}
              {emojiRecentValues}
              {visibleCustomEmojis}
              {emojiGroups}
              {visibleEmojiCategories}
              onScroll={handleGridScroll}
              onChooseRandom={chooseRandomEmoji}
              onChooseRecent={chooseRecent}
              onChooseIcon={chooseIcon}
              onResetGridScroll={resetGridScroll}
            />
          {:else if tab === "icons"}
            <IconPickerIconsTab
              bind:scrollElement={gridScrollElement}
              bind:iconCategoryMenuTriggerElement={iconCategoryMenuTriggerElement}
              bind:iconQuery
              bind:iconColor
              bind:iconColorPanelOpen
              bind:skinTonePanelOpen
              {askIconColorEveryTime}
              showColorControl={showIconColor}
              {iconCategory}
              {iconCategoryMenuOpen}
              {iconCategoryOverflowActive}
              {colorSelectionBorder}
              {gridScrollable}
              {gridCanScrollUp}
              {gridCanScrollDown}
              {gridColumnCount}
              {lucideRecentValues}
              {lucideGroupVirtual}
              {lucideLoading}
              {primaryLucideCategoryOptions}
              {automaticIconColor}
              {iconColorLabel}
              {iconColorSwatch}
              {iconColorStyle}
              {lucideRecentPreviewValue}
              onScroll={handleGridScroll}
              onChooseRandom={chooseRandomIcon}
              onChooseRecent={chooseRecent}
              onChooseLucideIcon={chooseLucideIcon}
              onSelectIconColor={selectIconColor}
              onDefaultIconColorPanelOpen={openDefaultIconColorPanel}
              onAskIconColorEveryTimeChange={setAskIconColorEveryTime}
              onSelectIconCategory={selectIconCategory}
              onToggleIconCategoryMenu={toggleIconCategoryMenu}
            />
          {:else}
            <IconPickerUploadPanel
              {uploadDraft}
              {uploadPreviewUrl}
              {uploadError}
              {uploading}
              {uploadBodyStyle}
              bind:uploadUrl
              onChooseFile={chooseUploadFile}
              onDiscardDraft={discardUploadDraft}
              onSelectDraft={selectUploadDraft}
              onDownloadUrl={downloadUploadUrl}
              remoteUrlAvailable={remoteImageUrlsAvailable}
            />
          {/if}
        {/if}
      </div>
    {/each}
  </div>

  {#if iconCategoryMenuOpen}
    <IconPickerCategoryMenu
      bind:rootElement={iconCategoryMenuElement}
      {portalTarget}
      style={iconCategoryMenuStyle}
      options={lucideCategoryOptions}
      selectedCategory={iconCategory}
      ariaLabel={t("projects.iconPicker.moreCategories")}
      onSelect={selectIconCategory}
    />
  {/if}

  {#if iconColorChoice}
    <IconPickerColorChoicePanel
      bind:rootElement={iconColorChoicePanelElement}
      {portalTarget}
      style={iconColorChoiceStyle(iconColorChoice)}
      label={iconColorChoice.label}
      slug={iconColorChoice.slug}
      iconNode={iconColorChoice.iconNode}
      {iconColorLabel}
      {iconColorStyle}
      {automaticIconColor}
      automaticLabel={t("projects.iconPicker.automaticColor")}
      columns={iconColorChoicePanelColumns}
      onSelect={selectColorChoice}
    />
  {/if}

  {#if customEmojiPanelOpen}
    <IconPickerCustomEmojiPanel
      bind:rootElement={customPanelElement}
      {portalTarget}
      style={customPanelStyle}
      {customEmojiDraft}
      bind:customEmojiName
      {customEmojiError}
      {customEmojiSaving}
      onChooseFile={chooseCustomEmojiFile}
      onPaste={handleCustomEmojiPaste}
      onSave={saveCustomEmoji}
      onClose={() => {
        customEmojiPanelOpen = false;
      }}
    />
  {/if}
{/if}
