<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    calculateTooltipContentWidth,
    calculateTooltipPosition,
    deriveTooltipPalette,
    isPointInHorizontalScrollbar,
    tooltipSurfaceColorFor,
    type TooltipPalette,
    type TooltipPaletteTokens,
    type TooltipPlacement,
    type TooltipRect,
  } from "./tooltip";

  const { t } = getLocalization();

  const tooltipId = "app-global-tooltip";
  const hoverDelayMs = 350;
  const maxSurfaceLookupDepth = 12;
  const maxTooltipWidthRem = 18;
  const tooltipViewportMarginPx = 8;
  const targetSelector = [
    "[data-app-tooltip]",
    "[data-tooltip]",
    "button[aria-label]",
    "a[aria-label]",
    "[role='button'][aria-label]",
    "[role='menuitem'][aria-label]",
  ].join(",");

  let tooltipEl = $state<HTMLDivElement | null>(null);
  let anchor: HTMLElement | null = null;
  let anchorTextOverride: string | undefined;
  let anchorRectOverride: TooltipRect | undefined;
  let text = $state("");
  let visible = $state(false);
  let ready = $state(false);
  let placement = $state<TooltipPlacement>("top");
  let tooltipStyle = $state("left: -9999px; top: -9999px;");
  let tooltipPaletteStyle = $state("");
  let tooltipWidthStyle = $state("");
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let suppressedPointerTarget: HTMLElement | null = null;
  const migratingTitleElements = new WeakSet<HTMLElement>();
  let measureCanvas: HTMLCanvasElement | undefined;

  function isInteractiveElement(element: HTMLElement): boolean {
    return element.matches("button, a, [role='button'], [role='menuitem']");
  }

  function syncGeneratedAriaLabel(element: HTMLElement, value: string): void {
    if (!isInteractiveElement(element)) return;
    if (element.dataset.appTooltipAria === "true") {
      element.setAttribute("aria-label", value);
      return;
    }
    if (element.hasAttribute("aria-label") || element.hasAttribute("aria-labelledby")) return;
    if (element.textContent?.trim()) return;

    element.dataset.appTooltipAria = "true";
    element.setAttribute("aria-label", value);
  }

  function clearGeneratedAriaLabel(element: HTMLElement): void {
    if (element.dataset.appTooltipAria !== "true") return;
    delete element.dataset.appTooltipAria;
    element.removeAttribute("aria-label");
  }

  function migrateTitleAttribute(element: Element): void {
    if (!(element instanceof HTMLElement)) return;
    const rawTitle = element.getAttribute("title");
    if (rawTitle === null) return;

    const tooltip = rawTitle.trim();
    migratingTitleElements.add(element);
    element.removeAttribute("title");

    if (tooltip.length > 0) {
      element.dataset.appTooltip = tooltip;
      element.dataset.appTooltipSource = "title";
      syncGeneratedAriaLabel(element, tooltip);
    } else if (element.dataset.appTooltipSource === "title") {
      delete element.dataset.appTooltip;
      delete element.dataset.appTooltipSource;
      clearGeneratedAriaLabel(element);
    }

    if (anchor === element) {
      text = tooltip;
      if (tooltip.length === 0) hideTooltip();
      else void positionTooltip();
    }
  }

  function migrateTitleAttributes(root: ParentNode): void {
    if (root instanceof Element) migrateTitleAttribute(root);
    for (const element of root.querySelectorAll("[title]")) {
      migrateTitleAttribute(element);
    }
  }

  function removeMigratedTitle(element: HTMLElement): void {
    if (migratingTitleElements.delete(element)) return;
    if (element.dataset.appTooltipSource !== "title") return;

    delete element.dataset.appTooltip;
    delete element.dataset.appTooltipSource;
    clearGeneratedAriaLabel(element);
    if (anchor === element) hideTooltip();
  }

  function tooltipTextFor(element: HTMLElement): string {
    const explicitTooltip = element.dataset.appTooltip?.trim()
      || element.dataset.tooltip?.trim()
      || "";
    if (explicitTooltip.length > 0) return explicitTooltip;
    if (!isInteractiveElement(element) || element.textContent?.trim()) return "";
    return element.getAttribute("aria-label")?.trim() || "";
  }

  function activeTooltipText(element: HTMLElement): string {
    return anchorTextOverride ?? tooltipTextFor(element);
  }

  function findTooltipTarget(target: EventTarget | null): HTMLElement | null {
    if (!(target instanceof Element)) return null;
    const element = target.closest<HTMLElement>(targetSelector);
    if (!(element instanceof HTMLElement)) return null;
    if (element.dataset.appTooltipDisabled === "true") return null;
    if (!document.documentElement.contains(element)) return null;
    return tooltipTextFor(element).length > 0 ? element : null;
  }

  function isKeyboardFocusIntent(): boolean {
    return document.documentElement.dataset.focusIntent === "keyboard";
  }

  function isSuppressedPointerTarget(element: HTMLElement): boolean {
    return suppressedPointerTarget === element;
  }

  function clearSuppressionIfPointerLeft(event: PointerEvent): void {
    if (!suppressedPointerTarget || !(event.target instanceof Node)) return;
    if (!suppressedPointerTarget.contains(event.target)) return;

    const relatedTarget = event.relatedTarget;
    if (relatedTarget instanceof Node && suppressedPointerTarget.contains(relatedTarget)) return;
    suppressedPointerTarget = null;
  }

  function readCssVar(style: CSSStyleDeclaration, name: string): string | undefined {
    const value = style.getPropertyValue(name).trim();
    return value.length > 0 ? value : undefined;
  }

  function readTooltipPaletteTokens(): TooltipPaletteTokens {
    const rootStyle = getComputedStyle(document.documentElement);
    return {
      background: readCssVar(rootStyle, "--background"),
      foreground: readCssVar(rootStyle, "--foreground"),
      popover: readCssVar(rootStyle, "--popover"),
      popoverForeground: readCssVar(rootStyle, "--popover-foreground"),
    };
  }

  function paletteStyle(palette: TooltipPalette): string {
    return [
      `--app-tooltip-bg: ${palette.background}`,
      `--app-tooltip-fg: ${palette.foreground}`,
      `--app-tooltip-border: ${palette.border}`,
      `--app-tooltip-shadow: ${palette.shadow}`,
    ].join("; ");
  }

  function hiddenTooltipStyle(): string {
    return [`left: -9999px`, `top: -9999px`, tooltipWidthStyle, tooltipPaletteStyle].filter(Boolean).join("; ");
  }

  function refreshTooltipPalette(element: HTMLElement): void {
    const tokens = readTooltipPaletteTokens();
    tooltipPaletteStyle = paletteStyle(
      deriveTooltipPalette(
        tooltipSurfaceColorFor(element, tokens.background, maxSurfaceLookupDepth),
        tokens,
      ),
    );
  }

  function tooltipHorizontalChromePx(element: HTMLElement): number {
    const style = window.getComputedStyle(element);
    return Number.parseFloat(style.paddingLeft)
      + Number.parseFloat(style.paddingRight)
      + Number.parseFloat(style.borderLeftWidth)
      + Number.parseFloat(style.borderRightWidth);
  }

  function tooltipTextWidth(textToMeasure: string, font: string): number {
    measureCanvas ??= document.createElement("canvas");
    const context = measureCanvas.getContext("2d");
    if (!context) return textToMeasure.length * 8;
    context.font = font;
    return context.measureText(textToMeasure).width;
  }

  function calculateTooltipWidthStyle(element: HTMLElement): string {
    const rootFontSize = Number.parseFloat(window.getComputedStyle(document.documentElement).fontSize) || 16;
    const cssMaxWidth = maxTooltipWidthRem * rootFontSize;
    const viewportMaxWidth = Math.max(0, window.innerWidth - tooltipViewportMarginPx * 2);
    const maxOuterWidth = Math.min(cssMaxWidth, viewportMaxWidth);
    const maxContentWidth = Math.max(0, maxOuterWidth - tooltipHorizontalChromePx(element));
    const font = window.getComputedStyle(element).font;
    const naturalWidth = tooltipTextWidth(text, font);
    if (naturalWidth <= maxContentWidth) return "";

    const width = calculateTooltipContentWidth(text, {
      maxContentWidth,
      minContentWidth: 96,
      maxLines: 2,
      measureText: (value) => tooltipTextWidth(value, font),
    });
    return width > 0 ? `width: ${width}px` : "";
  }

  async function positionTooltip(): Promise<void> {
    if (!anchor || !tooltipEl || !visible) return;
    if (!document.documentElement.contains(anchor)) {
      hideTooltip();
      return;
    }

    const nextWidthStyle = calculateTooltipWidthStyle(tooltipEl);
    tooltipWidthStyle = nextWidthStyle;
    if (nextWidthStyle) {
      tooltipStyle = hiddenTooltipStyle();
      await tick();
      if (!anchor || !tooltipEl || !visible) return;
    }

    const position = calculateTooltipPosition(
      anchorRectOverride ?? anchor.getBoundingClientRect(),
      tooltipEl.getBoundingClientRect(),
      { width: window.innerWidth, height: window.innerHeight },
    );
    placement = position.placement;
    tooltipStyle = [
      `left: ${position.left}px`,
      `top: ${position.top}px`,
      tooltipWidthStyle,
      `--app-tooltip-arrow-left: ${position.arrowLeft}px`,
      tooltipPaletteStyle,
    ].join("; ");
    ready = true;
  }

  function clearShowTimer(): void {
    if (showTimer === undefined) return;
    clearTimeout(showTimer);
    showTimer = undefined;
  }

  function showTooltipFor(
    element: HTMLElement,
    delayMs: number,
    textOverride?: string,
    rectOverride?: TooltipRect,
  ): void {
    const normalizedOverride = textOverride?.trim() || undefined;
    const nextText = normalizedOverride ?? tooltipTextFor(element);
    if (nextText.length === 0) return;
    if (
      anchor === element
      && anchorTextOverride === normalizedOverride
      && (visible || showTimer !== undefined)
    ) {
      anchorRectOverride = rectOverride;
      return;
    }

    clearShowTimer();
    anchor = element;
    anchorTextOverride = normalizedOverride;
    anchorRectOverride = rectOverride;
    text = nextText;
    ready = false;
    tooltipWidthStyle = "";
    refreshTooltipPalette(element);
    tooltipStyle = hiddenTooltipStyle();

    showTimer = setTimeout(() => {
      showTimer = undefined;
      if (!anchor) return;
      text = activeTooltipText(anchor);
      if (text.length === 0) {
        hideTooltip();
        return;
      }
      visible = true;
      void tick().then(positionTooltip);
    }, delayMs);
  }

  function hideTooltip(): void {
    clearShowTimer();
    anchor = null;
    anchorTextOverride = undefined;
    anchorRectOverride = undefined;
    visible = false;
    ready = false;
    text = "";
    tooltipWidthStyle = "";
    tooltipStyle = hiddenTooltipStyle();
  }

  function isStillInsideAnchor(event: PointerEvent | FocusEvent): boolean {
    if (!anchor || !(event.target instanceof Node)) return false;
    if (!anchor.contains(event.target)) return false;
    const relatedTarget = event.relatedTarget;
    return relatedTarget instanceof Node && anchor.contains(relatedTarget);
  }

  function horizontalScrollbarAtPointer(
    event: PointerEvent,
  ): { element: HTMLElement; rect: TooltipRect } | null {
    if (!(event.target instanceof Element)) return null;

    let element: HTMLElement | null = event.target instanceof HTMLElement
      ? event.target
      : event.target.parentElement;
    while (element && element !== document.body) {
      if (element.scrollWidth - element.clientWidth > 2) {
        const style = window.getComputedStyle(element);
        if (style.overflowX !== "auto" && style.overflowX !== "scroll") {
          element = element.parentElement;
          continue;
        }
        const rect = element.getBoundingClientRect();
        const borderBottomWidth = Number.parseFloat(style.borderBottomWidth) || 0;
        if (isPointInHorizontalScrollbar({
          rect,
          clientLeft: element.clientLeft,
          clientTop: element.clientTop,
          clientWidth: element.clientWidth,
          clientHeight: element.clientHeight,
          scrollWidth: element.scrollWidth,
          borderBottomWidth,
        }, { x: event.clientX, y: event.clientY })) {
          const left = rect.left + element.clientLeft;
          const top = rect.top + element.clientTop + element.clientHeight;
          const right = left + element.clientWidth;
          const bottom = rect.bottom - borderBottomWidth;
          return {
            element,
            rect: {
              left,
              top,
              right,
              bottom,
              width: right - left,
              height: bottom - top,
            },
          };
        }
      }
      element = element.parentElement;
    }
    return null;
  }

  onMount(() => {
    migrateTitleAttributes(document);

    const observer = new MutationObserver((records) => {
      for (const record of records) {
        if (record.type === "attributes" && record.attributeName === "title") {
          if (!(record.target instanceof HTMLElement)) continue;
          if (record.target.hasAttribute("title")) migrateTitleAttribute(record.target);
          else removeMigratedTitle(record.target);
          continue;
        }

        for (const node of record.addedNodes) {
          if (node instanceof Element) migrateTitleAttributes(node);
        }
      }
    });

    observer.observe(document.body, {
      attributes: true,
      attributeFilter: ["title"],
      childList: true,
      subtree: true,
    });

    const handlePointerOver = (event: PointerEvent) => {
      const target = findTooltipTarget(event.target);
      if (!target) return;
      if (isSuppressedPointerTarget(target)) return;
      showTooltipFor(target, hoverDelayMs);
    };

    const handlePointerOut = (event: PointerEvent) => {
      clearSuppressionIfPointerLeft(event);
      if (isStillInsideAnchor(event)) return;
      hideTooltip();
    };

    const handlePointerMove = (event: PointerEvent) => {
      // Disabled surfaces also opt out of scrollbar tooltip layout checks.
      if (event.target instanceof Element && event.target.closest('[data-app-tooltip-disabled="true"]')) {
        if (anchorTextOverride !== undefined) hideTooltip();
        return;
      }
      const scrollbar = horizontalScrollbarAtPointer(event);
      if (scrollbar) {
        showTooltipFor(
          scrollbar.element,
          hoverDelayMs,
          t("common.horizontalScrollTooltip"),
          scrollbar.rect,
        );
        return;
      }
      if (anchorTextOverride !== undefined) hideTooltip();
    };

    const handleFocusIn = (event: FocusEvent) => {
      if (!isKeyboardFocusIntent()) return;
      const target = findTooltipTarget(event.target);
      if (!target) return;
      if (target.dataset.appTooltipFocusDisabled === "true") return;
      if (isSuppressedPointerTarget(target)) return;
      showTooltipFor(target, 0);
    };

    const handleFocusOut = (event: FocusEvent) => {
      if (isStillInsideAnchor(event)) return;
      hideTooltip();
    };

    const handlePointerDown = (event: PointerEvent) => {
      const target = findTooltipTarget(event.target);
      if (target?.dataset.appTooltipKeepOnClick === "true") return;
      suppressedPointerTarget = target;
      hideTooltip();
    };

    const refreshPosition = () => {
      void positionTooltip();
    };

    const refreshPaletteAndPosition = () => {
      if (!anchor) return;
      refreshTooltipPalette(anchor);
      void positionTooltip();
    };

    const themeObserver = new MutationObserver(refreshPaletteAndPosition);
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class", "style"],
    });

    document.addEventListener("pointerover", handlePointerOver, true);
    document.addEventListener("pointerout", handlePointerOut, true);
    document.addEventListener("pointermove", handlePointerMove, true);
    document.addEventListener("focusin", handleFocusIn, true);
    document.addEventListener("focusout", handleFocusOut, true);
    document.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("resize", refreshPosition);
    window.addEventListener("scroll", hideTooltip, true);

    return () => {
      clearShowTimer();
      observer.disconnect();
      themeObserver.disconnect();
      document.removeEventListener("pointerover", handlePointerOver, true);
      document.removeEventListener("pointerout", handlePointerOut, true);
      document.removeEventListener("pointermove", handlePointerMove, true);
      document.removeEventListener("focusin", handleFocusIn, true);
      document.removeEventListener("focusout", handleFocusOut, true);
      document.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("resize", refreshPosition);
      window.removeEventListener("scroll", hideTooltip, true);
    };
  });
</script>

{#if visible && text}
  <div
    id={tooltipId}
    bind:this={tooltipEl}
    class:ready
    class="app-tooltip"
    data-placement={placement}
    role="tooltip"
    style={tooltipStyle}
  >
    {text}
  </div>
{/if}

<style>
  .app-tooltip {
    position: fixed;
    z-index: 2147483000;
    max-width: min(18rem, calc(100vw - 16px));
    padding: 0.42rem 0.65rem;
    border-radius: 0.5rem;
    border: 1px solid var(--app-tooltip-border, var(--border));
    background: var(--app-tooltip-bg, var(--popover));
    color: var(--app-tooltip-fg, var(--popover-foreground));
    box-shadow: var(--app-tooltip-shadow, 0 8px 28px rgba(0, 0, 0, 0.22));
    font-size: calc(0.8rem * var(--type-scale));
    font-weight: 500;
    letter-spacing: 0;
    line-height: 1.3;
    white-space: pre-line;
    overflow-wrap: anywhere;
    opacity: 0;
    pointer-events: none;
  }

  .app-tooltip.ready {
    opacity: 1;
  }

  .app-tooltip::after {
    position: absolute;
    left: var(--app-tooltip-arrow-left, 50%);
    width: 10px;
    height: 10px;
    border-radius: 1px;
    background: inherit;
    content: "";
    transform: translateX(-50%) rotate(45deg);
  }

  .app-tooltip[data-placement="top"]::after {
    bottom: -4px;
  }

  .app-tooltip[data-placement="bottom"]::after {
    top: -4px;
  }
</style>
