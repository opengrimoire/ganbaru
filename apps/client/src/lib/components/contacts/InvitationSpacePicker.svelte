<script lang="ts">
  import { onMount, tick } from "svelte";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Search from "@lucide/svelte/icons/search";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
  import {
    projectPickerBridgeFrameStyle,
    projectPickerMenuAimRect,
    projectPickerPanelFrameStyle,
    projectPickerPointerPoint,
    projectPickerSubpanelAimOrigin,
    projectPickerSubpanelGeometry,
    projectPickerSubpanelSide,
  } from "$lib/projects/picker-panels";
  import { PROJECT_NAVIGATOR_PANEL_WIDTH } from "$lib/projects/toolbar";
  import { isPointerAimingAtSubmenu, SUBMENU_AIM_TOLERANCES, type MenuAimPoint } from "$lib/utils/menu-aim";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    ancestorsOf,
    childrenOf,
    spaceSelectionState,
    toggleSpaceSelection,
    type InvitationSpaceNode,
  } from "./space-selection";

  /**
   * Cascading picker for the spaces an invitation covers: groups open their projects and projects open their
   * channels, with a checkbox at every level. It mirrors the channel access picker in Settings so both flows feel
   * the same. The picker is its own keyboard layer above the invitation dialog.
   */
  let {
    anchor,
    tree,
    selected,
    onSelectionChange,
    onClose,
  }: {
    anchor: HTMLElement;
    tree: readonly InvitationSpaceNode[];
    selected: ReadonlySet<string>;
    onSelectionChange: (next: ReadonlySet<string>) => void;
    onClose: () => void;
  } = $props();

  interface SearchResult {
    readonly node: InvitationSpaceNode;
    readonly path: string;
  }

  const { t } = getLocalization();
  const PANEL_GAP = 4;
  const PANEL_WIDTH = PROJECT_NAVIGATOR_PANEL_WIDTH;
  const PANEL_FALLBACK_ROW_HEIGHT = 32;
  const PANEL_FALLBACK_LIST_PADDING = 12;
  const VIEWPORT_MARGIN = 8;
  const COMPACT_MAX_WIDTH = 760;

  let query = $state("");
  let compact = $state(false);
  let layerElement = $state<HTMLDivElement>();
  let mainPanelElement = $state<HTMLDivElement>();
  let mainScrollElement = $state<HTMLElement>();
  let mainScrollContentElement = $state<HTMLElement>();
  let mainSearchElement = $state<HTMLElement>();
  let compactBackElement = $state<HTMLButtonElement>();
  let searchInputElement = $state<HTMLInputElement>();
  let firstPanelElement = $state<HTMLDivElement>();
  let firstScrollElement = $state<HTMLElement>();
  let firstAnchorElement = $state<HTMLElement | null>(null);
  let secondPanelElement = $state<HTMLDivElement>();
  let secondScrollElement = $state<HTMLElement>();
  let secondAnchorElement = $state<HTMLElement | null>(null);
  let firstBridgeStyle = $state("");
  let firstPanelStyle = $state("");
  let secondBridgeStyle = $state("");
  let secondPanelStyle = $state("");
  let mainPanelStyle = $state("visibility:hidden");
  /** Ids of the opened nodes: the root node whose children fill the first subpanel, then the one for the second. */
  let openPath = $state<string[]>([]);

  const rootNodes = $derived(childrenOf(tree, null));
  const firstOpen = $derived(nodeById(openPath[0]));
  const secondOpen = $derived(nodeById(openPath[1]));
  const firstNodes = $derived(firstOpen ? childrenOf(tree, firstOpen.id) : []);
  const secondNodes = $derived(secondOpen ? childrenOf(tree, secondOpen.id) : []);
  const compactNodes = $derived(secondOpen ? secondNodes : firstOpen ? firstNodes : rootNodes);
  const normalizedQuery = $derived(query.trim().toLocaleLowerCase());
  const searchResults = $derived.by((): SearchResult[] => {
    if (!normalizedQuery) return [];
    return tree.flatMap((node) => {
      const ancestors = ancestorsOf(tree, node.id).reverse().map((ancestor) => ancestor.name);
      const searchable = [...ancestors, node.name].join(" ").toLocaleLowerCase();
      if (!searchable.includes(normalizedQuery)) return [];
      return [{ node, path: ancestors.length > 0 ? ancestors.join(" / ") : t(`contacts.space.${node.kind}`) }];
    });
  });

  function nodeById(id: string | undefined): InvitationSpaceNode | null {
    return id ? tree.find((node) => node.id === id) ?? null : null;
  }

  function hasChildren(node: InvitationSpaceNode): boolean {
    return childrenOf(tree, node.id).length > 0;
  }

  function nodeLabel(node: InvitationSpaceNode): string {
    return node.kind === "channel" ? `#${node.name}` : node.name;
  }

  function toggle(node: InvitationSpaceNode): void {
    const state = spaceSelectionState(tree, selected, node.id);
    onSelectionChange(toggleSpaceSelection(tree, selected, node.id, state !== "all"));
  }

  function viewportBounds() {
    return {
      left: VIEWPORT_MARGIN,
      right: window.innerWidth - VIEWPORT_MARGIN,
      top: VIEWPORT_MARGIN,
      bottom: window.innerHeight - VIEWPORT_MARGIN,
    };
  }

  function outerHeight(element: HTMLElement | undefined): number {
    if (!element) return 0;
    const style = getComputedStyle(element);
    return element.offsetHeight + (Number.parseFloat(style.marginTop) || 0) + (Number.parseFloat(style.marginBottom) || 0);
  }

  function verticalPadding(element: HTMLElement): number {
    const style = getComputedStyle(element);
    return (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.paddingBottom) || 0);
  }

  function rowHeight(): number {
    return mainScrollContentElement?.querySelector<HTMLElement>(".menu-item")?.offsetHeight || PANEL_FALLBACK_ROW_HEIGHT;
  }

  function listPadding(): number {
    return mainScrollElement ? verticalPadding(mainScrollElement) : PANEL_FALLBACK_LIST_PADDING;
  }

  function updateMainPanelGeometry(): void {
    if (!mainPanelElement || !mainScrollElement || !mainScrollContentElement) return;
    const trigger = anchor.getBoundingClientRect();
    const bounds = viewportBounds();
    const usableWidth = Math.max(0, bounds.right - bounds.left);
    const width = Math.min(compact ? FLOATING_WIDTH.lg : PANEL_WIDTH, usableWidth);
    const maxHeight = Math.max(0, bounds.bottom - bounds.top);
    const naturalHeight = outerHeight(mainSearchElement)
      + outerHeight(compactBackElement)
      + mainScrollContentElement.scrollHeight
      + verticalPadding(mainScrollElement);
    const height = Math.min(naturalHeight, maxHeight);
    const left = Math.min(Math.max(bounds.left, trigger.left), Math.max(bounds.left, bounds.right - width));
    const belowTop = trigger.bottom + PANEL_GAP;
    const top = belowTop + height <= bounds.bottom ? belowTop : Math.max(bounds.top, trigger.top - PANEL_GAP - height);
    mainPanelStyle = [
      "visibility:visible",
      `left:${Math.round(left)}px`,
      `top:${Math.round(top)}px`,
      `width:${Math.round(width)}px`,
      `height:${Math.round(height)}px`,
      `max-height:${Math.round(maxHeight)}px`,
    ].join(";");
  }

  function pointerAimingAtPanel(point: MenuAimPoint, row: HTMLElement | null, panel: HTMLElement | undefined): boolean {
    if (!row || !panel) return false;
    const rowRect = row.getBoundingClientRect();
    const panelRect = panel.getBoundingClientRect();
    return isPointerAimingAtSubmenu({
      origin: projectPickerSubpanelAimOrigin(rowRect),
      point,
      submenu: projectPickerMenuAimRect(panelRect),
      side: projectPickerSubpanelSide(rowRect, panelRect),
      ...SUBMENU_AIM_TOLERANCES,
    });
  }

  function updateFirstPanelGeometry(): void {
    if (compact || !firstAnchorElement || !mainPanelElement || !firstOpen) return;
    const geometry = projectPickerSubpanelGeometry({
      anchorRect: firstAnchorElement.getBoundingClientRect(),
      panelRect: mainPanelElement.getBoundingClientRect(),
      bounds: viewportBounds(),
      gap: PANEL_GAP,
      footerHeight: 0,
      projectCount: Math.max(1, firstNodes.length),
      visibleRows: null,
      listPadding: listPadding(),
      rowHeight: rowHeight(),
    });
    firstBridgeStyle = projectPickerBridgeFrameStyle(geometry.bridge);
    firstPanelStyle = projectPickerPanelFrameStyle(geometry.panel);
  }

  function updateSecondPanelGeometry(): void {
    if (compact || !secondAnchorElement || !firstPanelElement || !secondOpen) return;
    const geometry = projectPickerSubpanelGeometry({
      anchorRect: secondAnchorElement.getBoundingClientRect(),
      panelRect: firstPanelElement.getBoundingClientRect(),
      bounds: viewportBounds(),
      gap: PANEL_GAP,
      footerHeight: 0,
      projectCount: Math.max(1, secondNodes.length),
      visibleRows: null,
      listPadding: listPadding(),
      rowHeight: rowHeight(),
    });
    secondBridgeStyle = projectPickerBridgeFrameStyle(geometry.bridge);
    secondPanelStyle = projectPickerPanelFrameStyle(geometry.panel);
  }

  function focusFirstName(panel: HTMLElement | undefined): void {
    panel?.querySelector<HTMLButtonElement>(".space-picker-name")?.focus();
  }

  function openNode(node: InvitationSpaceNode, level: 0 | 1, target: EventTarget | null, moveFocus = false): void {
    const anchorElement = target instanceof HTMLElement ? target : null;
    if (level === 0) {
      openPath = [node.id];
      firstAnchorElement = anchorElement;
      secondAnchorElement = null;
      secondPanelStyle = "";
    } else {
      openPath = [openPath[0] ?? node.parentId ?? node.id, node.id];
      secondAnchorElement = anchorElement;
    }
    void tick().then(() => {
      if (level === 0) updateFirstPanelGeometry();
      else updateSecondPanelGeometry();
      if (moveFocus) focusFirstName(compact ? mainPanelElement : level === 0 ? firstPanelElement : secondPanelElement);
    });
  }

  function handleRowPointer(node: InvitationSpaceNode, level: 0 | 1, event: PointerEvent): void {
    if (compact || openPath[level] === node.id) return;
    const currentAnchor = level === 0 ? firstAnchorElement : secondAnchorElement;
    const currentPanel = level === 0 ? firstPanelElement : secondPanelElement;
    if (openPath[level] && pointerAimingAtPanel(projectPickerPointerPoint(event), currentAnchor, currentPanel)) return;
    openNode(node, level, event.currentTarget);
  }

  function backCompact(): void {
    openPath = openPath.slice(0, -1);
    void tick().then(() => focusFirstName(mainPanelElement));
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && layerElement) {
      trapModalTabKey(layerElement, event);
      return;
    }
    if (event.key !== "Escape") return;
    event.preventDefault();
    onClose();
  }

  function handleResize(): void {
    compact = window.innerWidth < COMPACT_MAX_WIDTH;
    requestAnimationFrame(() => {
      updateMainPanelGeometry();
      updateFirstPanelGeometry();
      updateSecondPanelGeometry();
    });
  }

  $effect(() => {
    void searchResults.length;
    void compactNodes.length;
    requestAnimationFrame(updateMainPanelGeometry);
  });

  $effect(() => {
    if (!normalizedQuery) return;
    openPath = [];
    firstAnchorElement = null;
    secondAnchorElement = null;
  });

  onMount(() => {
    compact = window.innerWidth < COMPACT_MAX_WIDTH;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    void tick().then(() => {
      updateMainPanelGeometry();
      searchInputElement?.focus();
    });
    return deactivateKeyboard;
  });
</script>

{#snippet row(node: InvitationSpaceNode, level: 0 | 1 | null)}
  {@const state = spaceSelectionState(tree, selected, node.id)}
  {@const expandable = level !== null && hasChildren(node)}
  <div class="space-picker-row menu-item" data-highlighted={level !== null && openPath[level] === node.id ? "" : undefined}>
    <Checkbox checked={state === "all"} indeterminate={state === "some"} label={nodeLabel(node)} onChange={() => toggle(node)} />
    {#if expandable && level !== null}
      <button
        type="button"
        class="space-picker-name"
        onpointerenter={(event) => handleRowPointer(node, level, event)}
        onpointermove={(event) => handleRowPointer(node, level, event)}
        onfocus={(event) => { if (!compact) openNode(node, level, event.currentTarget); }}
        onclick={(event) => openNode(node, level, event.currentTarget, true)}
      ><span>{nodeLabel(node)}</span><ChevronRight size={13} /></button>
    {:else}
      <button type="button" class="space-picker-name" onclick={() => toggle(node)}><span>{nodeLabel(node)}</span></button>
    {/if}
  </div>
{/snippet}

<svelte:window onresize={handleResize} />

<!-- The layer counts as one floating surface so hosts that close on outside presses ignore clicks inside the picker and on its dismiss area. -->
<div bind:this={layerElement} use:portal class="space-picker-layer" role="dialog" aria-modal="true" aria-label={t("contacts.invite.spacePickerLabel")} data-app-floating-surface>
  <button type="button" class="space-picker-dismiss" aria-label={t("common.close")} data-app-tooltip-disabled="true" data-app-tooltip-focus-disabled="true" onclick={onClose}></button>

  <div bind:this={mainPanelElement} class="space-picker-panel main-panel surface-floating" style={mainPanelStyle}>
    <label bind:this={mainSearchElement} class="space-picker-search field">
      <Search size={13} />
      <input class="field-bare" bind:this={searchInputElement} bind:value={query} aria-label={t("contacts.invite.searchSpaces")} placeholder={t("contacts.invite.searchSpaces")} />
    </label>

    {#if compact && !normalizedQuery && openPath.length > 0}
      <button bind:this={compactBackElement} type="button" class="compact-back" onclick={backCompact}>
        <ChevronLeft size={13} />
        <span>{secondOpen?.name ?? firstOpen?.name ?? ""}</span>
      </button>
    {/if}

    <div class="space-picker-scroll-frame">
      <div bind:this={mainScrollElement} class="space-picker-list surface-floating-body hide-scrollbar" use:scrollEdgeFadeAction>
        <div bind:this={mainScrollContentElement}>
          {#if normalizedQuery}
            {#each searchResults as result (result.node.id)}
              {@const state = spaceSelectionState(tree, selected, result.node.id)}
              <div class="space-picker-row search-result-row menu-item">
                <Checkbox checked={state === "all"} indeterminate={state === "some"} label={nodeLabel(result.node)} onChange={() => toggle(result.node)} />
                <button type="button" class="space-picker-name search-result" onclick={() => toggle(result.node)}>
                  <strong>{nodeLabel(result.node)}</strong>
                  <small>{result.path}</small>
                </button>
              </div>
            {:else}
              <p class="space-picker-empty">{t("contacts.invite.noSpaceResults")}</p>
            {/each}
          {:else if compact}
            {#each compactNodes as node (node.id)}
              {@render row(node, secondOpen ? null : firstOpen ? 1 : 0)}
            {:else}
              <p class="space-picker-empty">{t("contacts.invite.noSpaceResults")}</p>
            {/each}
          {:else}
            {#each rootNodes as node (node.id)}
              {@render row(node, 0)}
            {:else}
              <p class="space-picker-empty">{t("contacts.invite.noSpaceResults")}</p>
            {/each}
          {/if}
        </div>
      </div>
      <CalendarScrollbar scrollContainer={mainScrollElement} stickyTop={4} stickyBottom={4} wheelPassthrough />
    </div>
  </div>

  {#if !compact && !normalizedQuery && firstOpen && firstAnchorElement}
    <div aria-hidden="true" class="space-picker-bridge" style={firstBridgeStyle}></div>
    <div bind:this={firstPanelElement} class="space-picker-panel subpanel surface-floating" style={firstPanelStyle}>
      <div class="space-picker-scroll-frame">
        <div bind:this={firstScrollElement} class="space-picker-list surface-floating-body hide-scrollbar" use:scrollEdgeFadeAction>
          {#each firstNodes as node (node.id)}
            {@render row(node, 1)}
          {/each}
        </div>
        <CalendarScrollbar scrollContainer={firstScrollElement} stickyTop={4} stickyBottom={4} wheelPassthrough />
      </div>
    </div>
  {/if}

  {#if !compact && !normalizedQuery && secondOpen && secondAnchorElement && firstPanelElement}
    <div aria-hidden="true" class="space-picker-bridge" style={secondBridgeStyle}></div>
    <div bind:this={secondPanelElement} class="space-picker-panel subpanel surface-floating" style={secondPanelStyle}>
      <div class="space-picker-scroll-frame">
        <div bind:this={secondScrollElement} class="space-picker-list surface-floating-body hide-scrollbar" use:scrollEdgeFadeAction>
          {#each secondNodes as node (node.id)}
            {@render row(node, null)}
          {/each}
        </div>
        <CalendarScrollbar scrollContainer={secondScrollElement} stickyTop={4} stickyBottom={4} wheelPassthrough />
      </div>
    </div>
  {/if}
</div>

<style>
  .space-picker-layer { position: fixed; z-index: 120; inset: 0; pointer-events: none; }
  .space-picker-dismiss { position: fixed; inset: 0; pointer-events: auto; cursor: default; }
  .space-picker-panel { position: fixed; z-index: 2; display: flex; min-height: 0; flex-direction: column; overflow: hidden; pointer-events: auto; }
  .main-panel { height: auto; }
  .subpanel { z-index: 4; }
  .space-picker-bridge { position: fixed; z-index: 3; background: transparent; pointer-events: auto; }
  .space-picker-search { display: flex; min-height: 2rem; flex: 0 0 auto; align-items: center; gap: 0.45rem; margin: var(--floating-padding) var(--floating-padding) 0; color: var(--muted-foreground); }
  .space-picker-search input { color: var(--popover-foreground); }
  .space-picker-scroll-frame { position: relative; min-height: 0; flex: 1; }
  .space-picker-list { height: 100%; overflow-y: auto; overscroll-behavior: contain; }
  .space-picker-name { display: flex; min-width: 0; flex: 1; height: 100%; align-items: center; justify-content: space-between; gap: 0.5rem; overflow: hidden; color: inherit; text-align: left; }
  .space-picker-name > span, .space-picker-name strong, .space-picker-name small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .space-picker-name > span { flex: 1; font-weight: 500; }
  .space-picker-name :global(svg) { flex: 0 0 auto; color: var(--muted-foreground); }
  .search-result-row { min-height: 2.55rem; }
  .space-picker-name.search-result { display: grid; justify-content: stretch; align-content: center; }
  .space-picker-name strong { font-weight: 550; }
  .space-picker-name small { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .space-picker-empty { padding: 0.65rem 0.75rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .compact-back { display: flex; min-height: 2rem; flex: 0 0 auto; align-items: center; gap: 0.45rem; margin: 0.1rem 0.4rem 0; border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent); padding: 0 0.25rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 600; }
  @media (pointer: coarse) {
    .compact-back { min-height: 2.75rem; }
  }
</style>
