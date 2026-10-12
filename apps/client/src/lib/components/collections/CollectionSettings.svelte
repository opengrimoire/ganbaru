<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { anchoredPanelContentHeight, anchoredPanelStyle, anchoredPanelWidth } from "$lib/utils/anchored-panel";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import CollectionPanel from "./CollectionPanel.svelte";
  import { setCollectionSettingsNavigation, type CollectionSettingsPage } from "./collection-settings-context";

  let { label, anchor = null, preferredWidth = 320, showHeader = true, onClose, children }: {
    label: string;
    anchor?: HTMLElement | null;
    preferredWidth?: number;
    /** Shows the title and close button on the first page; drill-in pages always show a back header. */
    showHeader?: boolean;
    onClose: () => void;
    children: Snippet;
  } = $props();

  const { t } = getLocalization();
  const VIEWPORT_HEIGHT_FRACTION = 0.8;
  const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex="0"]';
  let panel: HTMLDivElement | null = $state(null);
  let pages: CollectionSettingsPage[] = $state([]);
  const activePage = $derived(pages.at(-1));

  setCollectionSettingsNavigation({
    navigate: (page) => {
      if (pages.some((current) => current.trigger === page.trigger)) return;
      pages = [...pages, page];
      void focusBody();
    },
    isActive: (trigger) => trigger !== undefined && pages.some((page) => page.trigger === trigger),
  });

  $effect(() => {
    if (!panel) return;
    const floating = floatPanel(panel);
    return () => floating.destroy();
  });

  /** Focus the visible page after its DOM and snippet have updated. */
  async function focusBody(): Promise<void> {
    await tick();
    const visible = panel?.querySelector<HTMLElement>("[data-collection-settings-page]:not([hidden])");
    (visible?.querySelector<HTMLElement>(FOCUSABLE) ?? panel)?.focus({ preventScroll: true });
  }

  /** Return one level without discarding the parent page's drafts. */
  function back(): void {
    const page = pages.at(-1);
    if (!page) return;
    pages = pages.slice(0, -1);
    void tick().then(() => {
      if (page.trigger.isConnected) page.trigger.focus({ preventScroll: true });
    });
  }

  /**
   * Drop pages whose row left the panel, such as the page of a property that was just deleted, together with every
   * page opened from them, so the panel never shows a detail page for something that no longer exists.
   */
  function pruneDetachedPages(): void {
    const firstDetached = pages.findIndex((page) => !page.trigger.isConnected);
    if (firstDetached < 0) return;
    pages = pages.slice(0, firstDetached);
    void focusBody();
  }

  /** Close explicitly and return focus to the settings control. */
  function close(): void {
    onClose();
    if (anchor?.isConnected) anchor.focus({ preventScroll: true });
  }

  /** Position the nonmodal panel within the viewport and its owning preview. */
  function floatPanel(node: HTMLDivElement) {
    const previousFocus = document.activeElement;
    const owner = anchor ?? (previousFocus instanceof HTMLElement ? previousFocus : null);
    const moved = portal(node, owner?.closest<HTMLElement>("[data-floating-root]") ?? "body");
    const content = node.querySelector<HTMLElement>("[data-collection-settings-content]");
    const header = () => node.querySelector<HTMLElement>("[data-collection-settings-header]");
    let disposed = false;
    let dismissed = false;
    let restoreFocus = true;
    let lastOwnedFocus: HTMLElement | null = null;
    let previousStyle = "";
    const place = () => {
      if (disposed) return;
      const rect = owner?.getBoundingClientRect() ?? {
        top: 8, bottom: 8, left: window.innerWidth - preferredWidth - 8, right: window.innerWidth - 8,
      };
      const input = {
        triggerRect: rect,
        viewportWidth: window.innerWidth,
        viewportHeight: window.innerHeight,
        preferredWidth,
        preferredMaxHeight: window.innerHeight * VIEWPORT_HEIGHT_FRACTION,
        horizontalAlign: "end" as const,
      };
      const width = `${Math.round(anchoredPanelWidth(input))}px`;
      if (node.style.width !== width) node.style.width = width;
      const measured = [header(), content].filter((element): element is HTMLElement => element !== null);
      const style = anchoredPanelStyle({ ...input, contentHeight: anchoredPanelContentHeight(node, measured) });
      if (style !== previousStyle) {
        node.style.cssText = style;
        previousStyle = style;
      }
    };
    /** A modal dialog opened from the panel, such as a delete confirmation, sits above it rather than outside it. */
    const inModal = (target: Node) => target instanceof Element && target.closest("[aria-modal='true']") !== null;
    const outside = (event: Event) => {
      if (!dismissed && event.target instanceof Node && !node.contains(event.target) && !owner?.contains(event.target) && !inModal(event.target)) {
        dismissed = true;
        restoreFocus = false;
        const focused = document.activeElement;
        if (focused instanceof HTMLElement && node.contains(focused)) focused.blur();
        onClose();
      }
    };
    const focusin = (event: FocusEvent) => {
      if (event.target instanceof HTMLElement && event.target.closest("[data-app-floating-surface]") === node) lastOwnedFocus = event.target;
      outside(event);
    };
    const keydown = (event: KeyboardEvent) => {
      if (dismissed) return;
      if (!(event.target instanceof Element)) return;
      const lostDisabledFocus = event.key === "Escape"
        && (event.target === document.body || event.target === document.documentElement)
        && (document.activeElement === document.body || document.activeElement === document.documentElement)
        && lastOwnedFocus?.isConnected && node.contains(lastOwnedFocus) && lastOwnedFocus.matches(":disabled")
        && !node.querySelector("[data-app-floating-surface]");
      if (event.target.closest("[data-app-floating-surface]") !== node && !lostDisabledFocus) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (pages.length) back();
        else close();
        return;
      }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) || !(event.target instanceof HTMLButtonElement)) return;
      if (!event.target.hasAttribute("data-collection-settings-row")) return;
      const visible = node.querySelector<HTMLElement>("[data-collection-settings-page]:not([hidden])");
      const rows = [...(visible?.querySelectorAll<HTMLButtonElement>("[data-collection-settings-row]:not(:disabled)") ?? [])];
      const index = rows.indexOf(event.target);
      if (index < 0 || !rows.length) return;
      event.preventDefault();
      const next = event.key === "Home" ? 0 : event.key === "End" ? rows.length - 1
        : event.key === "ArrowDown" ? (index + 1) % rows.length : (index + rows.length - 1) % rows.length;
      rows[next]?.focus({ preventScroll: true });
    };
    const scroll = (event: Event) => {
      if (!(event.target instanceof Node) || !node.contains(event.target)) place();
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    if (content) observer?.observe(content);
    const observeHeader = () => {
      const current = header();
      if (current) observer?.observe(current);
    };
    observeHeader();
    const changes = new MutationObserver(() => {
      pruneDetachedPages();
      if (content) for (const page of content.children) observer?.observe(page);
      observeHeader();
      place();
    });
    if (content) {
      for (const page of content.children) observer?.observe(page);
      changes.observe(content, { childList: true, subtree: true, attributes: true, attributeFilter: ["hidden"] });
    }
    changes.observe(node, { childList: true });
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("focusin", focusin);
    window.addEventListener("keydown", keydown, true);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", scroll, true);
    void tick().then(() => {
      if (disposed) return;
      place();
      void focusBody();
    });
    return {
      destroy() {
        disposed = true;
        observer?.disconnect();
        changes.disconnect();
        document.removeEventListener("pointerdown", outside, true);
        document.removeEventListener("focusin", focusin);
        window.removeEventListener("keydown", keydown, true);
        window.removeEventListener("resize", place);
        window.removeEventListener("scroll", scroll, true);
        if (restoreFocus && node.contains(document.activeElement) && owner?.isConnected) owner.focus({ preventScroll: true });
        moved.destroy();
      },
    };
  }
</script>

<CollectionPanel bind:element={panel} label={activePage?.label ?? label} class="fixed z-80">
  {#if showHeader || activePage}
    <div data-collection-settings-header class="flex shrink-0 items-center gap-1 border-b border-border px-2 py-1.5">
      {#if activePage}
        <button type="button" class="collection-settings-control flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("common.back")} onclick={back}><ArrowLeft class="size-3.5" /></button>
      {/if}
      <h3 class="min-w-0 flex-1 truncate px-1 font-medium">{activePage?.label ?? label}</h3>
      {#if showHeader}
        <button type="button" class="collection-settings-control flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("common.close")} onclick={close}><X class="size-3.5" /></button>
      {/if}
    </div>
  {/if}
  <div use:scrollEdgeFadeAction class="min-h-0 overflow-x-hidden overflow-y-auto">
    <div data-collection-settings-content class="@container flow-root h-max p-1.5">
      <div data-collection-settings-page hidden={pages.length > 0} inert={pages.length > 0}>
        {@render children()}
      </div>
      {#each pages as page, index (page.trigger)}
        <div data-collection-settings-page hidden={index !== pages.length - 1} inert={index !== pages.length - 1}>
          {@render page.children()}
        </div>
      {/each}
    </div>
  </div>
</CollectionPanel>
