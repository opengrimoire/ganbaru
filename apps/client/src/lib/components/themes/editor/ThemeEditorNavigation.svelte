<script lang="ts">
  import { onDestroy } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn, isEditableKeyboardTarget } from "$lib/utils";
  import type { ThemeNavTarget } from "./model";

  let {
    items,
    scrollViewport,
    scrollContent,
  }: {
    items: ReadonlyArray<{ target: ThemeNavTarget; label: string }>;
    scrollViewport: HTMLDivElement | undefined;
    scrollContent: HTMLDivElement | undefined;
  } = $props();

  const { t } = getLocalization();
  const panelScrollKeys = new Set([
    "ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End",
  ]);
  let nav: HTMLElement | undefined = $state();
  let activeSection = $state<ThemeNavTarget>("general");
  let lockedSection = $state<ThemeNavTarget | undefined>();
  let scrollFrame: number | undefined;
  let navScrollFrame: number | undefined;
  let sectionLockTimer: ReturnType<typeof setTimeout> | undefined;
  let canScrollLeft = $state(false);
  let canScrollRight = $state(false);

  function selector(target: ThemeNavTarget): string {
    return `[data-theme-nav-target="${target}"]`;
  }

  function sectionElements(): Array<{ target: ThemeNavTarget; element: HTMLElement }> {
    if (!scrollViewport) return [];
    return items.flatMap((item) => {
      const element = scrollViewport?.querySelector<HTMLElement>(selector(item.target));
      return element ? [{ target: item.target, element }] : [];
    });
  }

  function updateActiveSection(): void {
    const viewport = scrollViewport;
    if (!viewport || viewport.clientHeight <= 0 || viewport.scrollHeight <= 0) return;
    if (lockedSection) {
      activeSection = lockedSection;
      return;
    }
    const sections = sectionElements();
    if (sections.length === 0) return;
    if (viewport.scrollTop + viewport.clientHeight >= viewport.scrollHeight - 2) {
      activeSection = sections.at(-1)?.target ?? activeSection;
      return;
    }
    const threshold = viewport.getBoundingClientRect().top + 8;
    let next = sections[0].target;
    for (const section of sections) {
      if (section.element.getBoundingClientRect().top > threshold) break;
      next = section.target;
    }
    activeSection = next;
  }

  function releaseSectionLock(): void {
    if (!lockedSection) return;
    if (sectionLockTimer) clearTimeout(sectionLockTimer);
    sectionLockTimer = setTimeout(() => {
      lockedSection = undefined;
      sectionLockTimer = undefined;
      updateActiveSection();
    }, 160);
  }

  function syncOverflow(): void {
    if (!nav) {
      canScrollLeft = false;
      canScrollRight = false;
      return;
    }
    const maximum = Math.max(0, nav.scrollWidth - nav.clientWidth);
    canScrollLeft = nav.scrollLeft > 1;
    canScrollRight = nav.scrollLeft < maximum - 1;
  }

  function scrollNavTargetIntoView(
    target: ThemeNavTarget,
    behavior: ScrollBehavior,
  ): void {
    const button = nav?.querySelector<HTMLButtonElement>(
      `[data-theme-nav-button="${target}"]`,
    );
    if (!nav || !button) return;
    const navRect = nav.getBoundingClientRect();
    const buttonRect = button.getBoundingClientRect();
    const leftOverflow = buttonRect.left - navRect.left - 8;
    const rightOverflow = buttonRect.right - navRect.right + 8;
    let nextLeft = nav.scrollLeft;
    if (leftOverflow < 0) nextLeft += leftOverflow;
    else if (rightOverflow > 0) nextLeft += rightOverflow;
    else {
      syncOverflow();
      return;
    }
    nav.scrollTo({
      left: Math.min(Math.max(0, nextLeft), Math.max(0, nav.scrollWidth - nav.clientWidth)),
      behavior,
    });
    syncOverflow();
  }

  function scrollToSection(target: ThemeNavTarget): void {
    const element = scrollViewport?.querySelector<HTMLElement>(selector(target))
      ?? document.querySelector<HTMLElement>(selector(target));
    if (!element) return;
    lockedSection = target;
    activeSection = target;
    releaseSectionLock();
    if (!scrollViewport) {
      element.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }
    const rootTop = scrollViewport.getBoundingClientRect().top;
    const targetTop = element.getBoundingClientRect().top;
    scrollViewport.scrollTo({
      top: scrollViewport.scrollTop + targetTop - rootTop,
      behavior: "smooth",
    });
  }

  function handleWheel(event: WheelEvent): void {
    if (!nav) return;
    const maximum = Math.max(0, nav.scrollWidth - nav.clientWidth);
    if (maximum <= 0) return;
    const rawDelta = Math.abs(event.deltaX) > Math.abs(event.deltaY)
      ? event.deltaX
      : event.deltaY;
    if (rawDelta === 0) return;
    const scale = event.deltaMode === WheelEvent.DOM_DELTA_LINE
      ? 16
      : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
        ? nav.clientWidth
        : 1;
    const nextLeft = Math.min(Math.max(0, nav.scrollLeft + rawDelta * scale), maximum);
    if (nextLeft === nav.scrollLeft) return;
    event.preventDefault();
    nav.scrollTo({ left: nextLeft, behavior: "auto" });
    syncOverflow();
  }

  export function queueActiveSectionUpdate(): void {
    if (lockedSection) {
      activeSection = lockedSection;
      releaseSectionLock();
      return;
    }
    if (scrollFrame !== undefined) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = undefined;
      updateActiveSection();
    });
  }

  export function focusViewportFromPointer(event: PointerEvent): void {
    if (!scrollViewport || isEditableKeyboardTarget(event.target)) return;
    if (
      event.target instanceof Element
      && event.target.closest("button, a[href], [role='button'], [role='combobox'], [role='listbox']")
    ) return;
    scrollViewport.focus({ preventScroll: true });
  }

  export function keepPanelScrollKey(event: KeyboardEvent): void {
    if (!panelScrollKeys.has(event.key) || isEditableKeyboardTarget(event.target)) return;
    event.stopPropagation();
  }

  $effect(() => {
    const viewport = scrollViewport;
    if (!viewport) return;
    const observer = new ResizeObserver(updateActiveSection);
    observer.observe(viewport);
    if (scrollContent) observer.observe(scrollContent);
    return () => observer.disconnect();
  });

  $effect(() => {
    if (!nav) return;
    const element = nav;
    syncOverflow();
    const onScroll = () => syncOverflow();
    const observer = new ResizeObserver(() => {
      syncOverflow();
      scrollNavTargetIntoView(activeSection, "auto");
    });
    element.addEventListener("scroll", onScroll, { passive: true });
    observer.observe(element);
    return () => {
      element.removeEventListener("scroll", onScroll);
      observer.disconnect();
    };
  });

  $effect(() => {
    const target = activeSection;
    if (!nav) return;
    if (navScrollFrame !== undefined) cancelAnimationFrame(navScrollFrame);
    navScrollFrame = requestAnimationFrame(() => {
      navScrollFrame = undefined;
      scrollNavTargetIntoView(target, "smooth");
    });
  });

  onDestroy(() => {
    if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame);
    if (navScrollFrame !== undefined) cancelAnimationFrame(navScrollFrame);
    if (sectionLockTimer) clearTimeout(sectionLockTimer);
  });
</script>

<div
  class="theme-editor-nav-shell relative h-9 overflow-hidden rounded-lg border border-border bg-card text-[0.733333rem] dark:bg-background"
  data-can-scroll-left={canScrollLeft}
  data-can-scroll-right={canScrollRight}
  onwheel={handleWheel}
>
  <nav
    bind:this={nav}
    class="theme-editor-nav grid h-full grid-cols-5 items-center gap-1 overflow-hidden px-1"
    aria-label={t("settings.theme.editor.sectionsLabel")}
  >
    {#each items as item}
      <button
        type="button"
        data-theme-nav-button={item.target}
        onclick={() => scrollToSection(item.target)}
        aria-current={activeSection === item.target ? "location" : undefined}
        class={cn(
          "flex h-7 min-w-0 items-center justify-center rounded-md px-2 text-center font-medium transition-colors",
          activeSection === item.target
            ? "bg-accent text-foreground"
            : "text-muted-foreground",
        )}
      >
        <span class="min-w-0 truncate uppercase">{item.label}</span>
      </button>
    {/each}
  </nav>
</div>

<style>
  .theme-editor-nav { scrollbar-width: none; }
  .theme-editor-nav::-webkit-scrollbar { display: none; }
  .theme-editor-nav-shell::before,
  .theme-editor-nav-shell::after {
    content: "";
    position: absolute;
    top: 1px;
    bottom: 1px;
    z-index: 2;
    width: 1.25rem;
    opacity: 0;
    pointer-events: none;
    transition: opacity 120ms ease-out;
  }
  .theme-editor-nav-shell::before {
    left: 0;
    background: linear-gradient(to right, var(--card), transparent);
  }
  .theme-editor-nav-shell::after {
    right: 0;
    background: linear-gradient(to left, var(--card), transparent);
  }
  :global(.dark) .theme-editor-nav-shell::before {
    background: linear-gradient(to right, var(--background), transparent);
  }
  :global(.dark) .theme-editor-nav-shell::after {
    background: linear-gradient(to left, var(--background), transparent);
  }
  @container theme-editor (max-width: 620px) {
    .theme-editor-nav {
      display: flex;
      overflow-x: auto;
      overflow-y: hidden;
      grid-template-columns: none;
      justify-content: flex-start;
    }
    .theme-editor-nav-shell[data-can-scroll-left="true"]::before,
    .theme-editor-nav-shell[data-can-scroll-right="true"]::after { opacity: 1; }
    .theme-editor-nav button {
      flex: 0 0 auto;
      min-width: max-content;
    }
  }
</style>
