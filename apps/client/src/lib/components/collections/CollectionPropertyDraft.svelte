<script lang="ts" generics="T extends string">
  import { onMount } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { anchoredPanelContentHeight, anchoredPanelStyle } from "$lib/utils/anchored-panel";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import CollectionPanel from "./CollectionPanel.svelte";
  import CollectionPropertyCreator from "./CollectionPropertyCreator.svelte";
  import CollectionPropertyIconButton from "./CollectionPropertyIconButton.svelte";
  import { COLLECTION_ACTION_PANEL_WIDTH } from "./collection-panel-width";
  import { collectionPropertyDraftHeaderBox, type CollectionPropertyDraftPlacement } from "./property-draft";
  import type { CollectionPropertyTypeOption } from "./property-icons";

  /**
   * A draft column header for a new property: an icon button and name field drawn on the header row where the
   * property will appear, with the type list below it. Choosing a type creates the property; Escape or an outside
   * click discards the draft. A blank name lets the owner choose a default name.
   */
  let { anchor, placement, trigger = null, types, pending = false, maxLength, onCreate, onClose }: {
    /** The header cell the draft is drawn from. */
    anchor: HTMLElement;
    placement: CollectionPropertyDraftPlacement;
    /** The control that opened the draft: it toggles the draft instead of dismissing it, and gets focus back on Escape. */
    trigger?: HTMLElement | null;
    types: readonly CollectionPropertyTypeOption<T>[];
    pending?: boolean;
    maxLength?: number;
    /** Create the property and resolve whether it was created. False keeps the draft for an error the owner reports; a rejection shows its message here. */
    onCreate: (type: T, name: string, icon: string | null) => Promise<boolean>;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  const VIEWPORT_HEIGHT_FRACTION = 0.7;
  let root: HTMLDivElement | undefined = $state();
  let header: HTMLDivElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();
  let panel: HTMLDivElement | null = $state(null);
  let name = $state("");
  let icon = $state<string | null>(null);
  let creating = $state(false);
  let error = $state<string | null>(null);
  let headerStyle = $state("");
  let panelStyle = $state("");
  let closed = false;
  const busy = $derived(pending || creating);

  /** Follow the anchor while the table scrolls or the window resizes. */
  function place(): void {
    if (!anchor.isConnected) {
      close(false);
      return;
    }
    const box = collectionPropertyDraftHeaderBox({
      anchorRect: anchor.getBoundingClientRect(),
      placement,
      viewportWidth: window.innerWidth,
      preferredWidth: COLLECTION_ACTION_PANEL_WIDTH,
    });
    headerStyle = `left:${box.left}px; top:${box.top}px; width:${box.width}px; height:${box.height}px`;
    const content = panel?.querySelector<HTMLElement>("[data-collection-menu-content]");
    panelStyle = anchoredPanelStyle({
      triggerRect: { top: box.top, bottom: box.top + box.height, left: box.left, right: box.left + box.width },
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
      preferredWidth: COLLECTION_ACTION_PANEL_WIDTH,
      preferredMaxHeight: window.innerHeight * VIEWPORT_HEIGHT_FRACTION,
      contentHeight: panel && content ? anchoredPanelContentHeight(panel, [content]) : undefined,
    });
  }

  /** Remove the draft, returning focus to its trigger unless the person moved elsewhere. */
  function close(restoreFocus: boolean): void {
    if (closed) return;
    closed = true;
    if (restoreFocus && trigger?.isConnected) trigger.focus({ preventScroll: true });
    onClose();
  }

  async function create(type: T): Promise<void> {
    if (busy) return;
    creating = true;
    error = null;
    try {
      if (await onCreate(type, name.trim(), icon)) close(true);
    } catch (caught: unknown) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      creating = false;
    }
  }

  /** Whether an event belongs to the draft itself rather than a nested picker that owns its own keys. */
  function ownsKey(target: Element): boolean {
    const surface = target.closest("[data-app-floating-surface]");
    return surface !== null && (surface === header || surface === panel);
  }

  function nameKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    panel?.querySelector<HTMLButtonElement>("[data-property-type]:not(:disabled)")?.click();
  }

  onMount(() => {
    place();
    const outside = (event: Event) => {
      if (!(event.target instanceof Node) || root?.contains(event.target) || trigger?.contains(event.target)) return;
      close(false);
    };
    const keydown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !(event.target instanceof Element) || !ownsKey(event.target)) return;
      event.preventDefault();
      event.stopPropagation();
      close(true);
    };
    const scroll = (event: Event) => {
      if (!(event.target instanceof Node) || !root?.contains(event.target)) place();
    };
    const content = panel?.querySelector<HTMLElement>("[data-collection-menu-content]");
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    if (content) observer?.observe(content);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", scroll, true);
    window.addEventListener("keydown", keydown, true);
    // Focus moves in after the menu that opened the draft has returned focus to its own trigger.
    const ready = setTimeout(() => {
      if (closed) return;
      input?.focus({ preventScroll: true });
      window.addEventListener("pointerdown", outside, true);
      document.addEventListener("focusin", outside);
    });
    return () => {
      clearTimeout(ready);
      observer?.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", scroll, true);
      window.removeEventListener("keydown", keydown, true);
      window.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("focusin", outside);
    };
  });
</script>

<div bind:this={root} use:portal={anchor.closest<HTMLElement>("[data-floating-root]") ?? "body"} class="contents" data-floating-root data-app-shortcuts="ignore" data-collection-property-draft>
  <div bind:this={header} role="group" aria-label={t("collections.property.add")} data-app-floating-surface
    class="surface-floating fixed z-80 flex items-center gap-1.5 px-1.5" style={headerStyle}>
    <CollectionPropertyIconButton class="size-7" {icon} kind={null} editable={!busy} onChange={(next) => { icon = next; }} />
    <input bind:this={input} class="field-bare h-full" aria-label={t("collections.property.name")} placeholder={t("collections.property.namePlaceholder")}
      maxlength={maxLength} disabled={busy} bind:value={name} onkeydown={nameKeydown} />
  </div>
  <CollectionPanel bind:element={panel} label={t("collections.property.selectType")} class="fixed z-80" style={panelStyle}>
    <div use:scrollEdgeFadeAction class="min-h-0 overflow-x-hidden overflow-y-auto">
      <div data-collection-menu-content class="@container flow-root h-max p-1.5">
        <div data-collection-menu-body>
          <CollectionPropertyCreator {types} pending={busy} onCreate={(type) => { void create(type); }} />
          {#if error}<p role="alert" class="px-2 py-1 text-destructive">{error}</p>{/if}
        </div>
      </div>
    </div>
  </CollectionPanel>
</div>
