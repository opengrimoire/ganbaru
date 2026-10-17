<script lang="ts">
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { onMount, type Component } from "svelte";
  import {
    readPairingStatus,
    type PairingStatus,
  } from "$lib/api/vault-handoff";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { mobileTopBarPanelGeometry } from "$lib/mobile/layout";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { getVaultOwnership } from "$lib/vault/ownership.svelte";

  interface HandoffPanelProps {
    platform: "desktop" | "android";
    presentation: "control";
    initialStatus?: PairingStatus | null;
    onActivated?: () => void;
    onStatusChange?: (status: PairingStatus) => void;
    onOpenDataSettings?: () => void;
  }

  interface MobileLinkingProps {
    initialStatus?: PairingStatus | null;
    bootstrapReplicaOnLink?: boolean;
    onComplete: () => void | Promise<void>;
  }

  let {
    platform,
    presentation,
    onOpened,
  }: {
    platform: "desktop" | "android";
    presentation: "desktop" | "mobile";
    onOpened?: () => void;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const settingsLauncher = getSettingsLauncher();
  const ownership = getVaultOwnership();
  let status = $state<PairingStatus | null>(null);
  let open = $state(false);
  let panelLoading = $state(false);
  let panelLoadError = $state<string | null>(null);
  let mobileLinkingLoading = $state(false);
  let mobileLinkingLoadError = $state<string | null>(null);
  let HandoffPanel = $state<Component<HandoffPanelProps> | null>(null);
  let MobileLinkingScreen = $state<Component<MobileLinkingProps> | null>(null);
  let triggerElement = $state<HTMLButtonElement | null>(null);
  let mobilePanelStyle = $state("");

  const mobileLinking = $derived(
    open && presentation === "mobile" && status?.linked !== true,
  );

  async function refreshStatus(): Promise<PairingStatus | null> {
    try {
      const next = await readPairingStatus();
      status = next;
      await ownership.refresh();
      if (open && presentation === "mobile") {
        if (next.linked) void loadPanel();
        else void loadMobileLinkingScreen();
      }
      return next;
    } catch (error) {
      console.warn("Failed to read linked-device status:", error);
      return null;
    }
  }

  async function loadPanel(): Promise<void> {
    if (HandoffPanel || panelLoading) return;
    panelLoading = true;
    panelLoadError = null;
    try {
      const module = await import("./VaultHandoffPanel.svelte");
      HandoffPanel = module.default;
    } catch (error) {
      panelLoadError = error instanceof Error ? error.message : t("vaultHandoff.unknownError");
      console.error("Failed to load linked-device controls:", error);
    } finally {
      panelLoading = false;
    }
  }

  async function loadMobileLinkingScreen(): Promise<void> {
    if (MobileLinkingScreen || mobileLinkingLoading) return;
    mobileLinkingLoading = true;
    mobileLinkingLoadError = null;
    try {
      const module = await import("$lib/components/mobile/MobileVaultHandoffOnboarding.svelte");
      MobileLinkingScreen = module.default;
    } catch (error) {
      mobileLinkingLoadError = error instanceof Error ? error.message : t("vaultHandoff.unknownError");
      console.error("Failed to load device linking:", error);
    } finally {
      mobileLinkingLoading = false;
    }
  }

  function rootPixelValue(property: string): number {
    const value = Number.parseFloat(
      getComputedStyle(document.documentElement).getPropertyValue(property),
    );
    return Number.isFinite(value) ? Math.max(0, value) : 0;
  }

  function updateMobilePanelPosition(): void {
    if (presentation !== "mobile" || !triggerElement) return;
    const triggerRect = triggerElement.getBoundingClientRect();
    const visualViewport = window.visualViewport;
    const viewportOffsetLeft = visualViewport?.offsetLeft ?? 0;
    const viewportOffsetTop = visualViewport?.offsetTop ?? 0;
    const viewportWidth = visualViewport?.width ?? window.innerWidth;
    const viewportHeight = visualViewport?.height ?? window.innerHeight;
    const safeAreaLeft = rootPixelValue("--safe-area-left");
    const safeAreaRight = rootPixelValue("--safe-area-right");
    const safeAreaTop = rootPixelValue("--safe-area-top");
    const safeAreaBottom = rootPixelValue("--safe-area-bottom");
    const geometry = mobileTopBarPanelGeometry({
      anchorLeft: triggerRect.left,
      anchorWidth: triggerRect.width,
      anchorBottom: triggerRect.bottom,
      desiredWidth: FLOATING_WIDTH.sm,
      desiredHeight: viewportHeight,
      viewportLeft: viewportOffsetLeft + safeAreaLeft,
      viewportWidth: Math.max(0, viewportWidth - safeAreaLeft - safeAreaRight),
      viewportTop: viewportOffsetTop + safeAreaTop,
      viewportHeight: Math.max(0, viewportHeight - safeAreaTop - safeAreaBottom),
    });
    mobilePanelStyle = [
      `left:${Math.round(geometry.left)}px`,
      `top:${Math.round(geometry.top)}px`,
      `width:${Math.round(geometry.width)}px`,
      `max-height:${Math.round(geometry.height)}px`,
    ].join(";");
  }

  function openPanel(): void {
    onOpened?.();
    updateMobilePanelPosition();
    open = true;
    if (presentation === "mobile" && status?.linked !== true) {
      void loadMobileLinkingScreen();
    } else {
      void loadPanel();
    }
    void refreshStatus();
  }

  function closePanel(): void {
    open = false;
  }

  function handleStatusChange(next: PairingStatus): void {
    status = next;
    if (open && presentation === "mobile") {
      if (next.linked) void loadPanel();
      else void loadMobileLinkingScreen();
    }
    void ownership.refresh();
  }

  function handleMobileLinkingComplete(): void {
    closePanel();
    void refreshStatus();
  }
  function handleActivated(): void {
    closePanel();
    void refreshStatus();
  }

  function openDataSettings(): void {
    closePanel();
    settingsLauncher.open("data");
  }

  onMount(() => {
    void refreshStatus();
    const timer = window.setInterval(() => {
      if (!open) void refreshStatus();
    }, 5_000);
    const handleVisibility = (): void => {
      if (document.visibilityState === "visible") void refreshStatus();
    };
    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  });

  $effect(() => {
    if (!open || presentation !== "mobile") return;
    updateMobilePanelPosition();
    const visualViewport = window.visualViewport;
    window.addEventListener("resize", updateMobilePanelPosition);
    visualViewport?.addEventListener("resize", updateMobilePanelPosition);
    visualViewport?.addEventListener("scroll", updateMobilePanelPosition);
    return () => {
      window.removeEventListener("resize", updateMobilePanelPosition);
      visualViewport?.removeEventListener("resize", updateMobilePanelPosition);
      visualViewport?.removeEventListener("scroll", updateMobilePanelPosition);
    };
  });

  $effect(() => {
    if (!mobileLinking) return;
    return mobileBackStack.activate({ handle: closePanel });
  });
</script>

<div class={cn("relative shrink-0", presentation === "mobile" && "h-full min-w-0")}>
  <button
    bind:this={triggerElement}
    type="button"
    data-linked-device-trigger
    class={cn(
      "group relative flex items-center justify-center text-muted-foreground transition-colors",
      presentation === "desktop"
        ? "size-8 rounded-lg hover:bg-sidebar-accent"
        : "h-full w-full min-w-0",
      open && (presentation === "desktop" ? "bg-sidebar-accent text-foreground" : "text-foreground"),
    )}
    title={t("vaultHandoff.heading")}
    aria-label={t("vaultHandoff.heading")}
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={() => { if (open) closePanel(); else openPanel(); }}
  >
    <span class={cn(
      "grid place-items-center rounded-full transition-colors",
      presentation === "mobile" ? "h-8 w-8 group-active:bg-accent/70" : "size-full",
      open && presentation === "mobile" && "bg-accent/70",
    )}>
      <RefreshCw
        size={presentation === "desktop" ? 14 : 19}
        strokeWidth={presentation === "desktop" ? 1.5 : 1.8}
        class={cn(presentation === "desktop" && "text-foreground/68 dark:text-white/76")}
        aria-hidden="true"
      />
    </span>
    {#if status?.linked && status.canWrite === false && !status.pendingTransfer}
      <span
        class={cn(
          "absolute size-1.5 rounded-full bg-warning",
          presentation === "desktop" ? "bottom-1 right-1" : "bottom-2 right-[calc(50%-0.8rem)]",
        )}
        aria-hidden="true"
      ></span>
    {/if}
  </button>

  {#if mobileLinking}
    <div
      class="fixed inset-0 z-80 bg-background text-foreground"
      role="dialog"
      aria-modal="true"
      aria-label={t("vaultHandoff.androidOnboardingTitle")}
    >
      {#if MobileLinkingScreen}
        <MobileLinkingScreen initialStatus={status} onComplete={handleMobileLinkingComplete} />
      {:else if mobileLinkingLoadError}
        <div
          class="mobile-viewport-height flex px-6"
          style="padding-top: var(--safe-area-top); padding-bottom: var(--safe-area-bottom);"
        >
          <Notice
            layout="fill"
            class="text-sm"
            message={mobileLinkingLoadError}
            retryLabel={t("vaultHandoff.retry")}
            onRetry={loadMobileLinkingScreen}
            onDismiss={closePanel}
          />
        </div>
      {:else}
        <div
          class="mobile-viewport-height grid place-items-center"
          style="padding-top: var(--safe-area-top); padding-bottom: var(--safe-area-bottom);"
          role="status"
          aria-label={t("common.loading")}
        >
          <svg viewBox="0 0 48 48" class="ganbaru-loading-ring size-12 text-foreground" aria-hidden="true">
            <circle class="ganbaru-loading-ring-stroke" cx="24" cy="24" r="18" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
          </svg>
        </div>
      {/if}
    </div>
  {:else if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-40"
      onclick={closePanel}
      onkeydown={(event) => { if (event.key === "Escape") closePanel(); }}
    ></div>
    <div
      role="dialog"
      aria-label={t("vaultHandoff.heading")}
      class={cn(
        "z-50 flex flex-col overflow-hidden",
        (HandoffPanel || panelLoadError) && "surface-floating",
        presentation === "desktop"
          ? "absolute right-0 top-9 max-h-[calc(100vh-3rem)] w-floating-sm"
          : "fixed",
      )}
      style={presentation === "mobile" ? mobilePanelStyle : undefined}
    >
      <div use:scrollEdgeFadeAction class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto py-1">
        {#if HandoffPanel}
          <HandoffPanel
            {platform}
            presentation="control"
            initialStatus={status}
            onStatusChange={handleStatusChange}
            onActivated={handleActivated}
            onOpenDataSettings={openDataSettings}
          />
        {:else if panelLoadError}
          <Notice
            class="px-3 py-2 text-panel-detail"
            message={panelLoadError}
            retryLabel={t("vaultHandoff.retry")}
            onRetry={loadPanel}
          />
        {:else}
          <Loading layout="fill" />
        {/if}
      </div>
    </div>
  {/if}
</div>
