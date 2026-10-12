<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import X from "@lucide/svelte/icons/x";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import { getThemeEditor } from "$lib/stores/theme-editor.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { hasOnlyShortcutModifier } from "$lib/keyboard-shortcuts";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import {
    loadSettingsDetail,
    retrySettingsDetail,
    type LoadedSettingsDetail,
  } from "$lib/components/settings/detail-registry";
  import {
    loadMobileThemeEditor,
    retryMobileThemeEditor,
  } from "$lib/components/themes/editor/mobile-editor-loader";
  import type { MobileThemeEditorComponent } from "$lib/components/themes/editor/mobile-editor-loader-contracts";
  import type {
    DistractionsLimitEditorTarget,
    DistractionsSettingsTab,
    NotesTransferOperation,
    ContactsSettingsTab,
    ChatProviderSetupTarget,
    ChatSettingsSubsection,
    SectionId,
    SettingsDetailKind,
    SettingsDraftKind,
  } from "$lib/settings/types";
  import { settingsSectionsForShell } from "./section-catalog";
  import SettingsSectionRenderer from "$lib/components/settings/SettingsSectionRenderer.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  type SettingsDetailView =
    | { kind: "distractions-limit"; target: DistractionsLimitEditorTarget }
    | { kind: "notes-transfer"; operation: NotesTransferOperation }
    | { kind: "chat-provider"; target: ChatProviderSetupTarget };

  let {
    onClose,
    initialSection,
    initialDistractionsTab,
    initialContactsTab,
    initialChatSubsection,
    initialChatTeammateId,
    initialChatChannelId,
    initialChatCreateTeammate,
    presentation,
  }: {
    onClose: () => void;
    initialSection?: SectionId;
    initialDistractionsTab?: DistractionsSettingsTab;
    initialContactsTab?: ContactsSettingsTab;
    initialChatSubsection?: ChatSettingsSubsection;
    initialChatTeammateId?: string;
    initialChatChannelId?: string;
    initialChatCreateTeammate?: boolean;
    presentation?: "dialog" | "mobile";
  } = $props();

  const themeEditor = getThemeEditor();
  const viewport = getViewport();
  const mobileBackStack = getMobileBackStack();
  const { t } = getLocalization();
  const mobilePresentation = $derived(
    (presentation ?? (BUILD_PLATFORM_PROFILE.shell === "mobile" ? "mobile" : "dialog"))
      === "mobile",
  );

  // Close Settings while a theme is open in the floating editor so the modal
  // backdrop does not block interaction with the app.
  $effect(() => {
    if (!mobilePresentation && themeEditor.editingId) requestSettingsClose();
  });

  const sections = $derived(settingsSectionsForShell(mobilePresentation ? "mobile" : "desktop"));

  const initialActiveSection = untrack(() => initialSection ?? "appearance");
  const initialActiveChatSubsection = untrack(() => initialChatSubsection ?? "teammates");
  const initialMobileSectionOpen = untrack(() => initialSection !== undefined);
  let activeSection = $state<SectionId>(initialActiveSection);
  let mobileSectionOpen = $state(initialMobileSectionOpen);
  let activeChatSubsection = $state<ChatSettingsSubsection>(initialActiveChatSubsection);
  let openDraft = $state<SettingsDraftKind | null>(null);
  let pendingDraftNavigation = $state<(() => void) | null>(null);
  let detailView = $state<SettingsDetailView | null>(null);
  let detailLoadState = $state<LazyComponentLoadState<
    SettingsDetailKind,
    LoadedSettingsDetail
  > | null>(null);
  const activeDetailLoadState = $derived(
    detailView && detailLoadState?.key === detailView.kind ? detailLoadState : null,
  );
  let detailScrollEl: HTMLElement | undefined = $state();
  let modalPanel: HTMLElement | undefined = $state();
  let detailScrollbarInsetTop = $state(0);
  let detailScrollbarInsetBottom = $state(0);
  let settingsScrollEl: HTMLElement | undefined = $state();
  let MobileThemeEditor = $state<MobileThemeEditorComponent | null>(null);
  let mobileThemeEditorLoading = $state(false);
  let mobileThemeEditorLoadError = $state("");
  let mobileThemeEditorLoadGeneration = 0;
  const useTopNav = $derived(!mobilePresentation && viewport.below("compact"));
  const useIconRail = $derived(!useTopNav && viewport.below("regular"));
  const settingsScrollbarInset = $derived(useTopNav ? 12 : useIconRail ? 16 : 24);
  const settingsContentPaddingClass = $derived(useTopNav ? "px-3 py-4" : useIconRail ? "px-5 py-5" : "p-8");
  const chatTeammatesUsesInternalScroll = $derived(
    !mobilePresentation && activeSection === "chat" && activeChatSubsection === "teammates",
  );

  $effect(() => {
    if (!mobilePresentation || !mobileSectionOpen) return;
    return mobileBackStack.activate({
      handle: detailView ? closeDetailView : requestMobileSectionClose,
    });
  });

  $effect(() => {
    const shouldPrepare = mobilePresentation
      && ((mobileSectionOpen && activeSection === "appearance") || themeEditor.editingId);
    if (!shouldPrepare || MobileThemeEditor || mobileThemeEditorLoading) return;
    void prepareMobileThemeEditor();
  });

  async function prepareMobileThemeEditor(retry = false): Promise<void> {
    if (MobileThemeEditor || mobileThemeEditorLoading) return;
    const generation = ++mobileThemeEditorLoadGeneration;
    mobileThemeEditorLoading = true;
    mobileThemeEditorLoadError = "";
    try {
      const component = await (retry
        ? retryMobileThemeEditor()
        : loadMobileThemeEditor());
      if (generation === mobileThemeEditorLoadGeneration) {
        MobileThemeEditor = component;
      }
    } catch (error) {
      if (generation !== mobileThemeEditorLoadGeneration) return;
      mobileThemeEditorLoadError = error instanceof Error ? error.message : String(error);
      console.error("Failed to load the mobile theme editor", error);
    } finally {
      if (generation === mobileThemeEditorLoadGeneration) {
        mobileThemeEditorLoading = false;
      }
    }
  }

  /** Retries loading the mobile theme editor and rejects when it fails again. */
  async function retryMobileThemeEditorLoad(): Promise<void> {
    await prepareMobileThemeEditor(true);
    if (mobileThemeEditorLoadError) throw new Error(mobileThemeEditorLoadError);
  }

  async function cancelUnloadedThemeEditor(): Promise<void> {
    mobileThemeEditorLoadError = "";
    await themeEditor.cancel();
  }

  function requestSettingsNavigation(navigate: () => void): void {
    if (!openDraft) {
      navigate();
      return;
    }
    pendingDraftNavigation = navigate;
  }

  function requestSettingsClose(): void {
    requestSettingsNavigation(onClose);
  }

  function cancelDraftNavigation(): void {
    pendingDraftNavigation = null;
  }

  function discardDraftAndContinue(): void {
    const navigate = pendingDraftNavigation;
    pendingDraftNavigation = null;
    openDraft = null;
    navigate?.();
  }

  function updateDraftState(draft: SettingsDraftKind | null): void {
    openDraft = draft;
    if (!draft) pendingDraftNavigation = null;
  }

  /** Loads a Settings detail component and resolves once the load state settles; failures land in the load state. */
  function requestSettingsDetail(kind: SettingsDetailKind, retry = false): Promise<void> {
    if (!retry && detailLoadState?.key === kind) return Promise.resolve();
    const loadingState = beginLazyComponentLoad(detailLoadState, kind);
    detailLoadState = loadingState;
    const request = retry ? retrySettingsDetail(kind) : loadSettingsDetail(kind);
    return request
      .then((component) => {
        if (!detailLoadState) return;
        const nextState = resolveLazyComponentLoad(
          detailLoadState,
          kind,
          loadingState.requestId,
          component,
        );
        if (nextState !== detailLoadState) detailLoadState = nextState;
      })
      .catch((error: unknown) => {
        if (!detailLoadState) return;
        const nextState = rejectLazyComponentLoad(
          detailLoadState,
          kind,
          loadingState.requestId,
          error,
        );
        if (nextState === detailLoadState) return;
        detailLoadState = nextState;
        console.error(`Failed to load ${kind} Settings detail:`, error);
      });
  }

  /** Retries a failed Settings detail load and rejects when it fails again, so the notice shows the outcome. */
  async function retrySettingsDetailLoad(kind: SettingsDetailKind): Promise<void> {
    await requestSettingsDetail(kind, true);
    if (detailLoadState?.key === kind && detailLoadState.status === "failed") {
      throw new Error(`The ${kind} Settings detail could not be loaded`);
    }
  }

  function activeSectionLabel(): string {
    const section = sections.find((candidate) => candidate.id === activeSection);
    return section ? t(section.labelKey) : t("settings.title");
  }

  function focusShortcutsSearch(): void {
    const input = document.querySelector<HTMLInputElement>(
      "[data-shortcuts-search-input]",
    );
    input?.focus();
    input?.select();
  }

  function scrollSettingsToTop(): void {
    queueMicrotask(() => {
      settingsScrollEl?.scrollTo({ top: 0 });
    });
  }

  function focusableElements(container: HTMLElement): HTMLElement[] {
    return [...container.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex='-1'])",
    )].filter((element) => !element.hidden && element.getClientRects().length > 0);
  }

  function trapModalFocus(event: KeyboardEvent): void {
    if (event.key !== "Tab" || !modalPanel) return;
    const focusable = focusableElements(modalPanel);
    if (focusable.length === 0) {
      event.preventDefault();
      modalPanel.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function selectSection(section: SectionId): void {
    if (mobilePresentation && !mobileSectionOpen) {
      activeSection = section;
      mobileSectionOpen = true;
      scrollSettingsToTop();
      return;
    }
    if (section === activeSection && !detailView) return;
    requestSettingsNavigation(() => {
      activeSection = section;
      detailView = null;
      detailLoadState = null;
      detailScrollEl = undefined;
      detailScrollbarInsetTop = 0;
      detailScrollbarInsetBottom = 0;
      scrollSettingsToTop();
    });
  }

  function requestMobileSectionClose(): void {
    requestSettingsNavigation(closeMobileSection);
  }

  function closeMobileSection(): void {
    detailView = null;
    detailLoadState = null;
    mobileSectionOpen = false;
    scrollSettingsToTop();
  }

  function openDistractionsLimitEditor(target: DistractionsLimitEditorTarget): void {
    activeSection = "distractions";
    detailView = { kind: "distractions-limit", target };
    detailScrollEl = undefined;
    detailScrollbarInsetTop = 0;
    detailScrollbarInsetBottom = 0;
    void requestSettingsDetail("distractions-limit");
    scrollSettingsToTop();
  }

  function openNotesTransferPanel(operation: NotesTransferOperation): void {
    activeSection = "notes";
    detailView = { kind: "notes-transfer", operation };
    detailScrollEl = undefined;
    detailScrollbarInsetTop = 0;
    detailScrollbarInsetBottom = 0;
    void requestSettingsDetail("notes-transfer");
    scrollSettingsToTop();
  }

  function openChatProviderSetup(target: ChatProviderSetupTarget): void {
    activeSection = "chat";
    activeChatSubsection = "providers";
    detailView = { kind: "chat-provider", target };
    detailScrollEl = undefined;
    detailScrollbarInsetTop = 0;
    detailScrollbarInsetBottom = 0;
    void requestSettingsDetail("chat-provider");
    scrollSettingsToTop();
  }

  function closeDetailView(): void {
    detailView = null;
    detailLoadState = null;
    detailScrollEl = undefined;
    detailScrollbarInsetTop = 0;
    detailScrollbarInsetBottom = 0;
    scrollSettingsToTop();
  }

  onMount(() => {
    const returnFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    queueMicrotask(() => {
      const first = modalPanel ? focusableElements(modalPanel)[0] : undefined;
      (first ?? modalPanel)?.focus();
    });
    function handleKeydown(e: KeyboardEvent) {
      if (pendingDraftNavigation) return;
      trapModalFocus(e);
      if (!mobilePresentation && hasOnlyShortcutModifier(e) && e.key === ",") {
        e.preventDefault();
        e.stopPropagation();
        requestSettingsClose();
        return;
      }
      if (!mobilePresentation && activeSection === "shortcuts" && hasOnlyShortcutModifier(e) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        e.stopPropagation();
        focusShortcutsSearch();
        return;
      }
      if (!mobilePresentation && e.key === "F1") {
        e.preventDefault();
        e.stopPropagation();
        selectSection("shortcuts");
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        if (detailView) {
          closeDetailView();
          return;
        }
        if (mobilePresentation && mobileSectionOpen) {
          requestMobileSectionClose();
          return;
        }
        requestSettingsClose();
        return;
      }
      // Keep the modal from leaking shortcuts to underlying panels.
      e.stopPropagation();
    }
    window.addEventListener("keydown", handleKeydown, true);
    return () => {
      window.removeEventListener("keydown", handleKeydown, true);
      queueMicrotask(() => {
        if (returnFocus?.isConnected) returnFocus.focus();
      });
    };
  });
</script>

{#if mobilePresentation}
  <div
    bind:this={modalPanel}
    data-settings-modal-panel
    data-settings-section={activeSection}
    role="dialog"
    aria-modal="true"
    aria-label={t("settings.title")}
    tabindex="-1"
    class="fixed inset-0 z-80 flex flex-col bg-background text-foreground outline-none"
    style="padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);"
  >
    {#if mobileSectionOpen}
      <header class="flex min-h-14 shrink-0 items-center gap-1 border-b border-border px-1">
        <button
          type="button"
          onclick={detailView ? closeDetailView : requestMobileSectionClose}
          aria-label={detailView
            ? t("mobile.settings.backToSection")
            : t("mobile.settings.backToCategories")}
          class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
        >
          <ArrowLeft size={22} aria-hidden="true" />
        </button>
        <h2 class="min-w-0 flex-1 truncate px-2 text-base font-semibold">
          {activeSectionLabel()}
        </h2>
        <button
          type="button"
          onclick={requestSettingsClose}
          aria-label={t("settings.close")}
          class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
        >
          <X size={21} aria-hidden="true" />
        </button>
      </header>

      <section
        bind:this={settingsScrollEl}
        data-settings-content
        use:scrollEdgeFadeAction
        class={cn(
          "mobile-settings-content min-h-0 flex-1",
          detailView ? "overflow-hidden" : "overflow-y-auto px-4 py-5",
        )}
      >
        {#if detailView}
          {#if activeDetailLoadState?.status === "ready"}
            {@const loadedDetail = activeDetailLoadState.component}
            {#if loadedDetail.kind === "distractions-limit" && detailView.kind === "distractions-limit"}
              {@const DetailComponent = loadedDetail.component}
              <DetailComponent
                target={detailView.target}
                onDone={closeDetailView}
                onCancel={closeDetailView}
                compactLayout
              />
            {/if}
          {:else if activeDetailLoadState?.status === "failed"}
            {@const failedDetailKind = activeDetailLoadState.key}
            <div class="h-full text-sm">
              <Notice
                layout="fill"
                message={t("common.viewLoadFailed", activeSectionLabel())}
                onRetry={() => retrySettingsDetailLoad(failedDetailKind)}
              />
            </div>
          {:else}
            <Loading layout="fill" class="h-full" />
          {/if}
        {:else}
          <div class="mx-auto w-full max-w-xl">
            <SettingsSectionRenderer
              {activeSection}
              {initialDistractionsTab}
              {initialContactsTab}
              {activeChatSubsection}
              {initialChatTeammateId}
              {initialChatChannelId}
              {initialChatCreateTeammate}
              onOpenDistractionsLimitEditor={openDistractionsLimitEditor}
              onOpenNotesTransferPanel={openNotesTransferPanel}
              onOpenChatProviderSetup={openChatProviderSetup}
              onChatSubsectionChange={(subsection: ChatSettingsSubsection) => {
                activeChatSubsection = subsection;
                scrollSettingsToTop();
              }}
              onRequestNavigation={requestSettingsNavigation}
              onDraftStateChange={updateDraftState}
            />
          </div>
        {/if}
      </section>
    {:else}
      <header class="flex min-h-14 shrink-0 items-center gap-2 border-b border-border px-2">
        <h2 class="min-w-0 flex-1 truncate px-2 text-lg font-semibold">
          {t("settings.title")}
        </h2>
        <button
          type="button"
          onclick={requestSettingsClose}
          aria-label={t("settings.close")}
          class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
        >
          <X size={22} aria-hidden="true" />
        </button>
      </header>

      <nav
        aria-label={t("mobile.settings.categoriesLabel")}
        use:scrollEdgeFadeAction
        class="min-h-0 flex-1 overflow-y-auto px-3 py-3"
      >
        <div class="mx-auto flex w-full max-w-xl flex-col gap-1">
          {#each sections as section}
            {@const Icon = section.icon}
            <button
              type="button"
              onclick={() => selectSection(section.id)}
              class="flex min-h-14 w-full items-center gap-3 rounded-xl px-3 text-left text-[0.95rem] font-medium active:bg-accent"
            >
              <span class="flex size-9 shrink-0 items-center justify-center">
                <Icon size={19} strokeWidth={1.75} aria-hidden="true" />
              </span>
              <span class="min-w-0 flex-1">{t(section.labelKey)}</span>
              <ChevronRight size={19} class="shrink-0 text-muted-foreground" aria-hidden="true" />
            </button>
          {/each}
        </div>
      </nav>
    {/if}
  </div>
{:else}
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class={cn(
    "fixed inset-0 z-70 flex",
    useTopNav ? "items-stretch justify-center p-1" : "items-center justify-center p-2",
  )}
  onclick={(e) => {
    e.stopPropagation();
    requestSettingsClose();
  }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <div
    bind:this={modalPanel}
    data-settings-modal-panel
    role="dialog"
    aria-modal="true"
    aria-label={t("settings.title")}
    tabindex="-1"
    data-settings-section={activeSection}
    class={cn(
      "surface-dialog relative z-10 flex overflow-hidden",
      useTopNav
        ? "h-[calc(100dvh-0.5rem)] w-full flex-col"
        : "h-[80vh]",
      !useTopNav && useIconRail ? "w-[min(760px,94vw)]" : "",
      !useTopNav && !useIconRail ? "w-[min(900px,90vw)]" : "",
    )}
    onclick={(e) => e.stopPropagation()}
  >
    {#if useTopNav}
      <header class="flex shrink-0 items-center gap-2 border-b border-border/70 bg-background/40 px-2 py-2 dark:bg-black/20">
        <nav class="flex min-w-0 flex-1 gap-1 overflow-x-auto rounded-md bg-card/60 p-0.5 dark:bg-background/60">
          {#each sections as section}
            {@const Icon = section.icon}
            <button
              onclick={() => {
                selectSection(section.id);
              }}
              class={cn(
                "flex h-8 shrink-0 items-center gap-1.5 rounded px-2.5 text-[0.8rem] font-medium",
                activeSection === section.id
                  ? "bg-accent text-accent-foreground"
                  : "text-foreground hover:bg-accent/60",
              )}
            >
              <Icon size={14} strokeWidth={1.75} class="shrink-0" />
              <span>{t(section.labelKey)}</span>
            </button>
          {/each}
        </nav>
        <button
          type="button"
          onclick={requestSettingsClose}
          aria-label={t("settings.close")}
          data-app-tooltip-disabled="true"
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <X size={15} strokeWidth={2} />
        </button>
      </header>
    {:else}
      <aside
        class={cn(
          "flex shrink-0 flex-col gap-3 bg-background/40 dark:bg-black/20",
          useIconRail ? "w-14 px-1 py-3" : "w-58 py-4 pl-2 pr-0",
        )}
      >
        <div class={cn("flex items-center", useIconRail ? "justify-center" : "justify-between")}>
          {#if !useIconRail}
            <span class="flex h-7 min-w-0 items-center gap-2.5 px-3 text-[0.866667rem] font-semibold text-muted-foreground">
              <SettingsIcon size={15} strokeWidth={1.75} class="shrink-0" />
              <span class="truncate">{t("settings.title")}</span>
            </span>
          {/if}
          <button
            type="button"
            onclick={requestSettingsClose}
            aria-label={t("settings.close")}
            data-app-tooltip-disabled="true"
            class="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <X size={14} strokeWidth={2} />
          </button>
        </div>
        <nav class="flex flex-col">
          {#each sections as section}
            {@const Icon = section.icon}
            <button
              onclick={() => {
                selectSection(section.id);
              }}
              aria-label={t(section.labelKey)}
              data-app-tooltip-disabled="true"
              class={cn(
                "flex items-center rounded-md text-left text-[0.866667rem] font-medium",
                useIconRail ? "h-9 justify-center px-0" : "gap-2.5 px-3 py-1.5",
                activeSection === section.id
                  ? "bg-accent text-accent-foreground"
                  : "text-foreground hover:bg-accent/60",
              )}
            >
              <Icon size={15} strokeWidth={1.75} class="shrink-0" />
              {#if !useIconRail}
                <span>{t(section.labelKey)}</span>
              {/if}
            </button>
          {/each}
        </nav>
      </aside>
    {/if}

    <div class="relative min-h-0 flex-1">
      <section
        bind:this={settingsScrollEl}
        data-settings-content
        class={cn(
          "h-full min-h-0 bg-background/40 dark:bg-black/20",
          detailView || activeSection === "shortcuts" || chatTeammatesUsesInternalScroll
            ? "overflow-hidden"
            : "hide-scrollbar overflow-y-auto",
          detailView ? "p-0" : settingsContentPaddingClass,
        )}
      >
        {#if detailView}
          {#if activeDetailLoadState?.status === "ready"}
            {@const loadedDetail = activeDetailLoadState.component}
            {#if loadedDetail.kind === "distractions-limit" && detailView.kind === "distractions-limit"}
              {@const DetailComponent = loadedDetail.component}
              <DetailComponent
                target={detailView.target}
                onDone={closeDetailView}
                onCancel={closeDetailView}
                compactLayout={useTopNav}
                iconRailLayout={useIconRail}
                onScrollContainerChange={(scrollContainer: HTMLElement | undefined) => {
                  detailScrollEl = scrollContainer;
                }}
                onScrollbarInsetsChange={(insets: { top: number; bottom: number }) => {
                  detailScrollbarInsetTop = insets.top;
                  detailScrollbarInsetBottom = insets.bottom;
                }}
              />
            {:else if loadedDetail.kind === "notes-transfer" && detailView.kind === "notes-transfer"}
              {@const DetailComponent = loadedDetail.component}
              <DetailComponent
                operation={detailView.operation}
                onCancel={closeDetailView}
                compactLayout={useTopNav}
                iconRailLayout={useIconRail}
                onScrollContainerChange={(scrollContainer: HTMLElement | undefined) => {
                  detailScrollEl = scrollContainer;
                }}
                onScrollbarInsetsChange={(insets: { top: number; bottom: number }) => {
                  detailScrollbarInsetTop = insets.top;
                  detailScrollbarInsetBottom = insets.bottom;
                }}
              />
            {:else if loadedDetail.kind === "chat-provider" && detailView.kind === "chat-provider"}
              {@const DetailComponent = loadedDetail.component}
              <DetailComponent
                target={detailView.target}
                onCancel={closeDetailView}
                compactLayout={useTopNav}
                iconRailLayout={useIconRail}
                onScrollContainerChange={(scrollContainer: HTMLElement | undefined) => {
                  detailScrollEl = scrollContainer;
                }}
                onScrollbarInsetsChange={(insets: { top: number; bottom: number }) => {
                  detailScrollbarInsetTop = insets.top;
                  detailScrollbarInsetBottom = insets.bottom;
                }}
              />
            {/if}
          {:else if activeDetailLoadState?.status === "failed"}
            {@const failedDetailKind = activeDetailLoadState.key}
            <div class="h-full text-sm">
              <Notice
                layout="fill"
                message={t("common.viewLoadFailed", activeSectionLabel())}
                onRetry={() => retrySettingsDetailLoad(failedDetailKind)}
              />
            </div>
          {:else}
            <Loading layout="fill" class="h-full" />
          {/if}
        {:else}
          <SettingsSectionRenderer
            {activeSection}
            {initialDistractionsTab}
            {initialContactsTab}
            {activeChatSubsection}
            {initialChatTeammateId}
            {initialChatChannelId}
            {initialChatCreateTeammate}
            onOpenDistractionsLimitEditor={openDistractionsLimitEditor}
            onOpenNotesTransferPanel={openNotesTransferPanel}
            onOpenChatProviderSetup={openChatProviderSetup}
            onChatSubsectionChange={(subsection: ChatSettingsSubsection) => {
              activeChatSubsection = subsection;
              scrollSettingsToTop();
            }}
            onRequestNavigation={requestSettingsNavigation}
            onDraftStateChange={updateDraftState}
          />
        {/if}
      </section>
      {#if detailView}
        <CalendarScrollbar
          scrollContainer={detailScrollEl}
          stickyTop={detailScrollbarInsetTop}
          stickyBottom={detailScrollbarInsetBottom}
          wheelPassthrough
        />
      {:else if activeSection !== "shortcuts" && !chatTeammatesUsesInternalScroll}
        <CalendarScrollbar
          scrollContainer={settingsScrollEl}
          stickyTop={settingsScrollbarInset}
          stickyBottom={settingsScrollbarInset}
          wheelPassthrough
        />
      {/if}
    </div>
  </div>
</div>

{#if pendingDraftNavigation}
  {#if openDraft === "profile"}
    <ConfirmDialog
      title={t("settings.profileIdentity.discardChangesTitle")}
      message={t("settings.profileIdentity.discardChangesMessage")}
      confirmLabel={t("settings.profileIdentity.discardChanges")}
      cancelLabel={t("settings.profileIdentity.keepEditing")}
      onConfirm={discardDraftAndContinue}
      onCancel={cancelDraftNavigation}
    />
  {:else}
    <ConfirmDialog
      title={t("settings.chat.teammates.discardDraftTitle")}
      message={t("settings.chat.teammates.discardDraftMessage")}
      confirmLabel={t("settings.chat.teammates.discardDraft")}
      cancelLabel={t("settings.chat.teammates.keepEditing")}
      onConfirm={discardDraftAndContinue}
      onCancel={cancelDraftNavigation}
    />
  {/if}
{/if}

{/if}

{#if mobilePresentation && themeEditor.editingId}
  {#if MobileThemeEditor}
    {@const Editor = MobileThemeEditor}
    <Editor />
  {:else}
    <div
      class="fixed z-85 flex items-center justify-center bg-background text-foreground"
      style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
      role="dialog"
      aria-modal="true"
      aria-label={t("settings.theme.editor.dialogLabel")}
    >
      <div class="flex w-full max-w-sm flex-col items-center gap-3 text-center">
        {#if mobileThemeEditorLoadError}
          <Notice
            class="text-sm"
            title={t("common.viewLoadFailed", t("settings.theme.editor.dialogLabel"))}
            message={mobileThemeEditorLoadError}
            onRetry={retryMobileThemeEditorLoad}
            onDismiss={() => void cancelUnloadedThemeEditor()}
          />
        {:else}
          <Loading layout="fill" />
          <button
            type="button"
            class="min-h-12 w-full rounded-xl px-4 text-sm font-medium active:bg-accent"
            onclick={() => void cancelUnloadedThemeEditor()}
          >
            {t("common.cancel")}
          </button>
        {/if}
      </div>
    </div>
  {/if}
{/if}
