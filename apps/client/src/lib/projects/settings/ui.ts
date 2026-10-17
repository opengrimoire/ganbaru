import type { EventColor } from "$lib/calendar/types";
import { EVENT_COLOR_OPTIONS } from "$lib/calendar/utils";
import { cn } from "$lib/utils";

export type ProjectSettingsIconButtonTone = "neutral" | "danger";
export interface ProjectSettingsScrollInput {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
  scrollTopBoundary: number;
  scrollBottomBoundary: number;
  rowTop: number;
  rowBottom: number;
  fadeInset: number;
  breathingRoom?: number;
}

export function projectSettingsIconButtonClass(
  tone: ProjectSettingsIconButtonTone = "neutral",
): string {
  return cn(
    "flex h-7 w-7 shrink-0 items-center justify-center rounded-md disabled:cursor-not-allowed disabled:opacity-40",
    tone === "danger"
      ? "text-destructive hover:bg-destructive/10"
      : "text-muted-foreground hover:bg-accent hover:text-foreground",
  );
}

export function nextProjectSettingsPaletteColor(
  color: EventColor,
  fallbackColor: EventColor,
): EventColor {
  const index = EVENT_COLOR_OPTIONS.indexOf(color);
  if (index < 0) return fallbackColor;
  return EVENT_COLOR_OPTIONS[(index + 1) % EVENT_COLOR_OPTIONS.length] ?? fallbackColor;
}

export function nextUnusedProjectSettingsColor({
  preferredColor,
  usedColors,
  fallbackColor,
  random = Math.random,
}: {
  preferredColor: EventColor;
  usedColors: ReadonlySet<EventColor>;
  fallbackColor: EventColor;
  random?: () => number;
}): EventColor {
  if (usedColors.size >= EVENT_COLOR_OPTIONS.length) {
    const index = Math.floor(random() * EVENT_COLOR_OPTIONS.length);
    return EVENT_COLOR_OPTIONS[index] ?? fallbackColor;
  }
  const preferredIndex = Math.max(0, EVENT_COLOR_OPTIONS.indexOf(preferredColor));
  for (let offset = 0; offset < EVENT_COLOR_OPTIONS.length; offset += 1) {
    const color = EVENT_COLOR_OPTIONS[(preferredIndex + offset) % EVENT_COLOR_OPTIONS.length];
    if (color !== undefined && !usedColors.has(color)) return color;
  }
  return fallbackColor;
}

export function projectSettingsCssLengthToPixels(
  value: string,
  context: HTMLElement,
  fallback: number,
): number {
  const trimmed = value.trim();
  const parsed = Number.parseFloat(trimmed);
  if (!Number.isFinite(parsed)) return fallback;
  if (trimmed.endsWith("rem")) {
    const rootFontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
    return parsed * (Number.isFinite(rootFontSize) ? rootFontSize : 16);
  }
  if (trimmed.endsWith("em")) {
    const contextFontSize = Number.parseFloat(getComputedStyle(context).fontSize);
    return parsed * (Number.isFinite(contextFontSize) ? contextFontSize : 16);
  }
  return parsed;
}

export function projectSettingsScrollFadeInset(scrollElement: HTMLElement): number {
  return projectSettingsCssLengthToPixels(
    getComputedStyle(scrollElement).getPropertyValue("--scroll-fade-size"),
    scrollElement,
    32,
  );
}

export function projectSettingsScrollTargetTop(input: ProjectSettingsScrollInput): number {
  const breathingRoom = input.breathingRoom ?? 4;
  const maxScrollTop = Math.max(0, input.scrollHeight - input.clientHeight);
  const safeTop = input.scrollTopBoundary
    + (input.scrollTop > 1 ? input.fadeInset : 0)
    + breathingRoom;
  const safeBottom = input.scrollBottomBoundary
    - (input.scrollTop < maxScrollTop - 1 ? input.fadeInset : 0)
    - breathingRoom;
  if (input.rowTop >= safeTop && input.rowBottom <= safeBottom) return input.scrollTop;
  const scrollDelta = input.rowTop < safeTop
    ? input.rowTop - safeTop
    : input.rowBottom - safeBottom;
  return Math.min(
    Math.max(0, input.scrollTop + scrollDelta),
    maxScrollTop,
  );
}

export function scrollProjectSettingsRowIntoView(
  scrollElement: HTMLElement | undefined,
  rowElement: HTMLElement | undefined,
): void {
  if (!scrollElement || !rowElement) return;
  const scrollRect = scrollElement.getBoundingClientRect();
  const rowRect = rowElement.getBoundingClientRect();
  const nextScrollTop = projectSettingsScrollTargetTop({
    scrollTop: scrollElement.scrollTop,
    scrollHeight: scrollElement.scrollHeight,
    clientHeight: scrollElement.clientHeight,
    scrollTopBoundary: scrollRect.top,
    scrollBottomBoundary: scrollRect.bottom,
    rowTop: rowRect.top,
    rowBottom: rowRect.bottom,
    fadeInset: projectSettingsScrollFadeInset(scrollElement),
  });
  if (Math.abs(nextScrollTop - scrollElement.scrollTop) <= 1) return;
  scrollElement.scrollTo({ top: nextScrollTop, behavior: "auto" });
}
