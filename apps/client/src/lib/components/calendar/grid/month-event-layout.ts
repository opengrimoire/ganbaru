import type { CalendarEvent } from "$lib/calendar/types";
import { getMeetingIndicatorState } from "./event-indicators";

export const MONTH_EVENT_CHIP_HEIGHT_PX = 20;
export const MONTH_EVENT_ROW_GAP_PX = 2;

const MONTH_MEETING_ICON_SIZE_PX = 8;
const MONTH_MEETING_ICON_GAP_PX = 2;
const MONTH_MEETING_ICON_TEXT_GAP_PX = 4;
const DEFAULT_HORIZONTAL_GAP_PX = 2;
const DEFAULT_MIN_EVENT_WIDTH_PX = 28;
const DEFAULT_MIN_MORE_WIDTH_PX = 42;
const DEFAULT_HORIZONTAL_PADDING_PX = 8;
const DEFAULT_AVG_CHAR_WIDTH_PX = 5.8;
const DEFAULT_TEXT_SAFETY_PX = 3;

interface MonthDayLayoutBase {
  row: number;
  leftPx: number;
  topPx: number;
  widthPx: number;
  heightPx: number;
}

export interface MonthDayEventLayoutItem extends MonthDayLayoutBase {
  kind: "event";
  event: CalendarEvent;
}

export interface MonthDayMoreLayoutItem extends MonthDayLayoutBase {
  kind: "more";
  hiddenCount: number;
  label: string;
}

export type MonthDayLayoutItem = MonthDayEventLayoutItem | MonthDayMoreLayoutItem;

export interface MonthDayLayout {
  items: MonthDayLayoutItem[];
  hiddenCount: number;
  rowCount: number;
}

/** Width variant of the overflow chip label. `compact` drops words so it fits narrow cells. */
export type MonthMoreLabelVariant = "full" | "compact";

/** Formats the localized overflow chip label for a hidden event count. */
export type MonthMoreLabelFormatter = (hiddenCount: number, variant: MonthMoreLabelVariant) => string;

export interface MonthDayLayoutOptions {
  cellWidthPx: number;
  availableHeightPx: number;
  /** Localized overflow label, measured so the chip width matches the rendered text. */
  formatMoreLabel: MonthMoreLabelFormatter;
  /** Localized fallback shown for untitled events, measured so chip widths match the rendered text. */
  untitledLabel: string;
  chipHeightPx?: number;
  rowGapPx?: number;
  horizontalGapPx?: number;
  minEventWidthPx?: number;
  minMoreWidthPx?: number;
  horizontalPaddingPx?: number;
  avgCharWidthPx?: number;
  textSafetyPx?: number;
}

interface NormalizedMonthDayLayoutOptions {
  cellWidthPx: number;
  formatMoreLabel: MonthMoreLabelFormatter;
  untitledLabel: string;
  chipHeightPx: number;
  rowGapPx: number;
  horizontalGapPx: number;
  minEventWidthPx: number;
  minMoreWidthPx: number;
  horizontalPaddingPx: number;
  avgCharWidthPx: number;
  textSafetyPx: number;
}

interface MonthDayEventWidthEstimate {
  compactWidthPx: number;
  desiredWidthPx: number;
}

interface PackedMonthDayEventLayoutItem extends MonthDayLayoutBase {
  kind: "event";
  event: CalendarEvent;
  desiredWidthPx: number;
}

interface MonthDayMoreChipEstimate {
  widthPx: number;
  label: string;
}

function roundPx(value: number): number {
  return Math.round(value * 100) / 100;
}

function finitePositive(value: number | undefined, fallback: number): number {
  return Number.isFinite(value) && value !== undefined && value > 0 ? value : fallback;
}

function normalizeOptions(options: MonthDayLayoutOptions): NormalizedMonthDayLayoutOptions {
  const cellWidthPx = Math.max(0, Number.isFinite(options.cellWidthPx) ? options.cellWidthPx : 0);
  return {
    cellWidthPx,
    formatMoreLabel: options.formatMoreLabel,
    untitledLabel: options.untitledLabel,
    chipHeightPx: finitePositive(options.chipHeightPx, MONTH_EVENT_CHIP_HEIGHT_PX),
    rowGapPx: Math.max(0, finitePositive(options.rowGapPx, MONTH_EVENT_ROW_GAP_PX)),
    horizontalGapPx: Math.max(0, finitePositive(options.horizontalGapPx, DEFAULT_HORIZONTAL_GAP_PX)),
    minEventWidthPx: Math.max(0, finitePositive(options.minEventWidthPx, DEFAULT_MIN_EVENT_WIDTH_PX)),
    minMoreWidthPx: Math.max(0, finitePositive(options.minMoreWidthPx, DEFAULT_MIN_MORE_WIDTH_PX)),
    horizontalPaddingPx: Math.max(
      0,
      finitePositive(options.horizontalPaddingPx, DEFAULT_HORIZONTAL_PADDING_PX),
    ),
    avgCharWidthPx: finitePositive(options.avgCharWidthPx, DEFAULT_AVG_CHAR_WIDTH_PX),
    textSafetyPx: Math.max(0, finitePositive(options.textSafetyPx, DEFAULT_TEXT_SAFETY_PX)),
  };
}

function rowCountForHeight(availableHeightPx: number, chipHeightPx: number, rowGapPx: number): number {
  if (!Number.isFinite(availableHeightPx) || availableHeightPx < chipHeightPx) return 0;
  return Math.max(0, Math.floor((availableHeightPx + rowGapPx) / (chipHeightPx + rowGapPx)));
}

function visibleTitle(event: CalendarEvent, untitledLabel: string): string {
  const title = event.title.trim();
  return title.length > 0 ? title : untitledLabel;
}

function characterWidth(char: string, avgCharWidthPx: number): number {
  if (char === " ") return avgCharWidthPx * 0.55;
  if ("fijlrtI1|![](){}".includes(char)) return avgCharWidthPx * 0.62;
  if ("mwMW@#%&".includes(char)) return avgCharWidthPx * 1.45;
  if (".,:;'`\"".includes(char)) return avgCharWidthPx * 0.65;

  const codePoint = char.codePointAt(0) ?? 0;
  if (codePoint >= 48 && codePoint <= 57) return avgCharWidthPx * 0.95;
  if (codePoint >= 65 && codePoint <= 90) return avgCharWidthPx * 1.22;
  if (codePoint > 0x2e80) return avgCharWidthPx * 1.8;
  if (codePoint > 0x7f) return avgCharWidthPx * 1.05;
  return avgCharWidthPx;
}

function textWidth(text: string, options: NormalizedMonthDayLayoutOptions): number {
  let width = 0;
  for (const char of text) width += characterWidth(char, options.avgCharWidthPx);
  return width;
}

function meetingIndicatorWidth(event: CalendarEvent): number {
  const { iconCount } = getMeetingIndicatorState(event);
  if (iconCount === 0) return 0;
  return MONTH_MEETING_ICON_TEXT_GAP_PX
    + iconCount * MONTH_MEETING_ICON_SIZE_PX
    + (iconCount - 1) * MONTH_MEETING_ICON_GAP_PX;
}

function maxPackedEventWidth(options: NormalizedMonthDayLayoutOptions, minWidth: number): number {
  return Math.max(
    minWidth,
    (options.cellWidthPx - options.horizontalGapPx) / 2,
  );
}

function estimateEventWidths(
  event: CalendarEvent,
  options: NormalizedMonthDayLayoutOptions,
): MonthDayEventWidthEstimate {
  if (options.cellWidthPx <= 0) return { compactWidthPx: 0, desiredWidthPx: 0 };
  const minWidth = Math.min(options.cellWidthPx, options.minEventWidthPx);
  const maxPackedWidth = maxPackedEventWidth(options, minWidth);
  const naturalWidth = textWidth(visibleTitle(event, options.untitledLabel), options)
    + options.horizontalPaddingPx
    + meetingIndicatorWidth(event);
  const compactWidthPx = roundPx(
    Math.min(options.cellWidthPx, Math.max(minWidth, Math.min(naturalWidth, maxPackedWidth))),
  );
  const desiredWidthPx = roundPx(
    Math.min(
      options.cellWidthPx,
      Math.max(compactWidthPx, Math.min(naturalWidth + options.textSafetyPx, maxPackedWidth)),
    ),
  );
  return { compactWidthPx, desiredWidthPx };
}

function labelWidth(label: string, options: NormalizedMonthDayLayoutOptions): number {
  return textWidth(label, options)
    + options.horizontalPaddingPx
    + options.textSafetyPx;
}

function estimateMoreChip(
  hiddenCount: number,
  options: NormalizedMonthDayLayoutOptions,
): MonthDayMoreChipEstimate {
  if (options.cellWidthPx <= 0) return { widthPx: 0, label: options.formatMoreLabel(hiddenCount, "compact") };
  const minWidth = Math.min(options.cellWidthPx, options.minMoreWidthPx);
  const fullLabel = options.formatMoreLabel(hiddenCount, "full");
  const fullWidth = labelWidth(fullLabel, options);
  if (fullWidth <= options.cellWidthPx) {
    return {
      widthPx: roundPx(Math.min(options.cellWidthPx, Math.max(minWidth, fullWidth))),
      label: fullLabel,
    };
  }

  const compactLabel = options.formatMoreLabel(hiddenCount, "compact");
  const compactWidth = labelWidth(compactLabel, options);
  return {
    widthPx: roundPx(Math.min(options.cellWidthPx, Math.max(minWidth, compactWidth))),
    label: compactLabel,
  };
}

function topForRow(row: number, options: NormalizedMonthDayLayoutOptions): number {
  return row * (options.chipHeightPx + options.rowGapPx);
}

function rowWidth(
  items: readonly PackedMonthDayEventLayoutItem[],
  widthOf: (item: PackedMonthDayEventLayoutItem) => number,
  options: NormalizedMonthDayLayoutOptions,
): number {
  if (items.length === 0) return 0;
  let width = options.horizontalGapPx * (items.length - 1);
  for (const item of items) {
    width += widthOf(item);
  }
  return width;
}

function findLastItemIndexInRow(items: readonly PackedMonthDayEventLayoutItem[], row: number): number {
  for (let i = items.length - 1; i >= 0; i -= 1) {
    if (items[i].row === row) return i;
  }
  return -1;
}

function publicEventItem(item: PackedMonthDayEventLayoutItem): MonthDayEventLayoutItem {
  return {
    kind: "event",
    event: item.event,
    row: item.row,
    leftPx: item.leftPx,
    topPx: item.topPx,
    widthPx: item.widthPx,
    heightPx: item.heightPx,
  };
}

function fullWidthEventItems(
  events: readonly CalendarEvent[],
  options: NormalizedMonthDayLayoutOptions,
): MonthDayEventLayoutItem[] {
  return events.map((event, row) => ({
    kind: "event",
    event,
    row,
    leftPx: 0,
    topPx: topForRow(row, options),
    widthPx: roundPx(options.cellWidthPx),
    heightPx: options.chipHeightPx,
  }));
}

function allocateRowEventWidths(
  rowItems: PackedMonthDayEventLayoutItem[],
  rightBoundaryPx: number,
  options: NormalizedMonthDayLayoutOptions,
): void {
  if (rowItems.length === 0) return;

  const compactWidth = rowWidth(rowItems, (item) => item.widthPx, options);
  let spareWidth = Math.max(0, rightBoundaryPx - compactWidth);

  for (const item of rowItems) {
    if (spareWidth <= 0) break;
    const readableDeficit = Math.max(0, item.desiredWidthPx - item.widthPx);
    const addedWidth = Math.min(spareWidth, readableDeficit);
    item.widthPx += addedWidth;
    spareWidth -= addedWidth;
  }

  const lastItem = rowItems[rowItems.length - 1];
  if (lastItem && spareWidth > 0) {
    lastItem.widthPx += spareWidth;
  }

  let leftPx = 0;
  for (const item of rowItems) {
    item.leftPx = roundPx(leftPx);
    item.widthPx = roundPx(item.widthPx);
    leftPx += item.widthPx + options.horizontalGapPx;
  }
}

function arrangeLayoutItems(
  eventItems: readonly PackedMonthDayEventLayoutItem[],
  moreItem: MonthDayMoreLayoutItem | undefined,
  rowCount: number,
  options: NormalizedMonthDayLayoutOptions,
): MonthDayLayoutItem[] {
  const mutableEvents = eventItems.map((item) => ({ ...item }));
  const arrangedItems: MonthDayLayoutItem[] = [];

  for (let row = 0; row < rowCount; row += 1) {
    const rowEvents = mutableEvents.filter((item) => item.row === row);
    const rowMore = moreItem?.row === row ? { ...moreItem } : undefined;
    const rightBoundaryPx = rowMore && rowEvents.length > 0
      ? Math.max(0, rowMore.leftPx - options.horizontalGapPx)
      : options.cellWidthPx;

    allocateRowEventWidths(rowEvents, rightBoundaryPx, options);
    arrangedItems.push(...rowEvents.map(publicEventItem));
    if (rowMore) arrangedItems.push(rowMore);
  }

  return arrangedItems;
}

function appendMoreChip(
  eventItems: PackedMonthDayEventLayoutItem[],
  hiddenCount: number,
  rowCount: number,
  options: NormalizedMonthDayLayoutOptions,
): MonthDayLayout {
  const targetRow = Math.max(0, rowCount - 1);
  let hidden = hiddenCount;
  const items = [...eventItems];

  while (true) {
    const moreChip = estimateMoreChip(hidden, options);
    const targetRowItems = items.filter((item) => item.row === targetRow);
    const rowBoundaryPx = targetRowItems.length > 0
      ? Math.max(0, options.cellWidthPx - moreChip.widthPx - options.horizontalGapPx)
      : options.cellWidthPx;
    const compactWidthPx = rowWidth(targetRowItems, (item) => item.widthPx, options);
    const desiredWidthPx = rowWidth(targetRowItems, (item) => item.desiredWidthPx, options);

    if (
      targetRowItems.length === 0 ||
      (compactWidthPx <= rowBoundaryPx && desiredWidthPx <= rowBoundaryPx) ||
      items.length === 0
    ) {
      const hasRowEvent = targetRowItems.length > 0;
      const moreItem: MonthDayMoreLayoutItem = {
        kind: "more",
        hiddenCount: hidden,
        label: moreChip.label,
        row: targetRow,
        leftPx: roundPx(hasRowEvent ? options.cellWidthPx - moreChip.widthPx : 0),
        topPx: topForRow(targetRow, options),
        widthPx: moreChip.widthPx,
        heightPx: options.chipHeightPx,
      };
      return {
        items: arrangeLayoutItems(items, moreItem, rowCount, options),
        hiddenCount: hidden,
        rowCount,
      };
    }

    const removeIndex = findLastItemIndexInRow(items, targetRow);
    if (removeIndex < 0) {
      const moreItem: MonthDayMoreLayoutItem = {
        kind: "more",
        hiddenCount: hidden,
        label: moreChip.label,
        row: targetRow,
        leftPx: 0,
        topPx: topForRow(targetRow, options),
        widthPx: moreChip.widthPx,
        heightPx: options.chipHeightPx,
      };
      return {
        items: arrangeLayoutItems(items, moreItem, rowCount, options),
        hiddenCount: hidden,
        rowCount,
      };
    }

    items.splice(removeIndex, 1);
    hidden += 1;
  }
}

export function layoutMonthDayEvents(
  events: readonly CalendarEvent[],
  options: MonthDayLayoutOptions,
): MonthDayLayout {
  const normalized = normalizeOptions(options);
  const rowCount = rowCountForHeight(
    options.availableHeightPx,
    normalized.chipHeightPx,
    normalized.rowGapPx,
  );

  if (events.length === 0) {
    return { items: [], hiddenCount: 0, rowCount };
  }
  if (rowCount === 0 || normalized.cellWidthPx <= 0) {
    return { items: [], hiddenCount: events.length, rowCount };
  }
  if (events.length <= rowCount) {
    return {
      items: fullWidthEventItems(events, normalized),
      hiddenCount: 0,
      rowCount,
    };
  }

  const items: PackedMonthDayEventLayoutItem[] = [];
  let row = 0;
  let cursorX = 0;
  let packedCount = 0;

  for (const event of events) {
    const { compactWidthPx, desiredWidthPx } = estimateEventWidths(event, normalized);
    if (cursorX > 0 && cursorX + normalized.horizontalGapPx + compactWidthPx > normalized.cellWidthPx) {
      row += 1;
      cursorX = 0;
    }
    if (row >= rowCount) break;

    const leftPx = cursorX > 0 ? cursorX + normalized.horizontalGapPx : 0;
    items.push({
      kind: "event",
      event,
      row,
      leftPx: roundPx(leftPx),
      topPx: topForRow(row, normalized),
      widthPx: compactWidthPx,
      heightPx: normalized.chipHeightPx,
      desiredWidthPx,
    });
    cursorX = leftPx + compactWidthPx;
    packedCount += 1;
  }

  const hiddenCount = events.length - packedCount;
  if (hiddenCount <= 0) {
    return {
      items: arrangeLayoutItems(items, undefined, rowCount, normalized),
      hiddenCount: 0,
      rowCount,
    };
  }

  return appendMoreChip(items, hiddenCount, rowCount, normalized);
}
