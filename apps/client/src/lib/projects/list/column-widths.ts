import type {
  ProjectCoreTaskListColumn,
  ProjectCustomField,
  ProjectPriority,
  ProjectStatus,
  ProjectTask,
  ProjectTaskListColumn,
  ProjectViewPreference,
} from "$lib/projects/types";
import { PROJECT_TASK_LIST_COLUMNS } from "$lib/projects/types";
import { customFieldIdFromTaskListColumn } from "$lib/projects/tasks/list-columns";
import {
  projectCustomFieldHasCellIcon,
  projectCustomFieldUsesOptions,
} from "$lib/projects/custom-fields";

export const TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY = "list-column-widths";
export type ProjectTaskListResizableColumn = "name" | ProjectTaskListColumn;
export type ProjectTaskListColumnWidths = Partial<Record<ProjectTaskListResizableColumn, number>>;
export interface ProjectTaskListColumnResizeGesture {
  column: ProjectTaskListResizableColumn;
  pointerId: number;
  startClientX: number;
  startWidthRem: number;
  rootFontSizePx: number;
  widthsAtStart: ProjectTaskListColumnWidths;
  draftWidths: ProjectTaskListColumnWidths;
  moved: boolean;
}

/** Text styles list cells render content in: plain cell text, a regular badge, and a medium-weight status badge. */
export type ProjectTaskListTextStyle = "cell" | "badge" | "badgeStrong";

export interface ProjectTaskListGridInput {
  columns: readonly ProjectTaskListColumn[];
  columnWidths?: ProjectTaskListColumnWidths;
  tasks?: readonly ProjectTask[];
  statuses?: readonly ProjectStatus[];
  customFields?: readonly ProjectCustomField[];
  nameLabel?: string;
  sectionLabels?: readonly string[];
  groupLabels?: readonly string[];
  columnLabel?: (column: ProjectTaskListColumn) => string;
  /** Whether column headers lead with a property icon. */
  headerIcons?: boolean;
  /**
   * Width of one line of text in a cell style, in rem, measured in the rendered font so every language, font family,
   * and font scale fits. Without it widths come from a character count estimate.
   */
  measureText?: (text: string, style: ProjectTaskListTextStyle) => number | undefined;
  priorityLabel?: (priority: ProjectPriority) => string;
  /** Every priority a task can take, so picking another priority never needs a wider column. */
  priorityLabels?: readonly string[];
  estimateLabel?: (minutes: number) => string;
  /** The label a date cell shows in its column's date and time formats. */
  dateLabel?: (column: ProjectTaskListColumn, date: string, time?: string) => string;
  /** The placeholder an empty start or due date cell shows. */
  emptyDateLabel?: (column: "start" | "due") => string;
  /** The placeholder an empty custom field cell shows. */
  emptyValueLabel?: string;
  /** Today as `YYYY-MM-DD`; a sample date in each month of its year keeps date columns wide enough for any month. */
  today?: string;
  /** What a custom field cell shows for a task, such as a formatted number or date or the joined option names. */
  customFieldCellLabel?: (task: ProjectTask, field: ProjectCustomField) => string | undefined;
  /** Every option of a choice field, so picking another option never needs a wider column. */
  customFieldOptionNames?: (field: ProjectCustomField) => readonly string[];
  /** The dependency badges a task shows. */
  dependencyLabels?: (task: ProjectTask) => readonly string[];
  scheduledLabel?: (taskId: string) => string | null;
  /** Automatic widths from `projectTaskListAutoColumnWidths`, so resizing one column never refits every cell. */
  autoColumnWidths?: ProjectTaskListColumnWidths;
}

/**
 * One thing a cell can show: text in a style plus the fixed chrome around it, such as icons, gaps, and badge padding.
 * Several texts sit side by side, as dependency badges do.
 */
interface ProjectTaskListCellContent {
  texts: readonly string[];
  style: ProjectTaskListTextStyle;
  chromeRem: number;
}

interface ProjectTaskListColumnWidthBounds {
  /** Narrowest manual width. */
  min: number;
  /** Narrowest automatic width. */
  autoMin: number;
  /** Widest automatic width that content alone can ask for; headers can ask for more, up to their own cap. */
  contentMax: number;
}

const PROJECT_LIST_SELECTION_TRACK_REM = 1.5;
const PROJECT_LIST_EXPAND_TRACK_REM = 1.75;
const PROJECT_LIST_ADD_COLUMN_TRACK_REM = 2.25;
/** The root font size the pixel sizes of cell icons and avatars convert from. */
const PROJECT_LIST_ROOT_FONT_PX = 15;
/** Horizontal padding of a cell (`px-2`). */
const CELL_PADDING_REM = 1;
/** Room past the measured content so subpixel rounding and font hinting never truncate it. */
const FIT_SLACK_REM = 0.25;
/** A 13px cell icon, such as the priority flag or a chevron. */
const CELL_ICON_REM = 13 / PROJECT_LIST_ROOT_FONT_PX;
/** The `gap-1.5` between an icon and its label. */
const ICON_GAP_REM = 0.375;
/** A header's property icon (`size-3.5`) and its gap. */
const HEADER_ICON_REM = 0.875 + ICON_GAP_REM;
/** Status badge padding (`px-2`), color dot (`size-2`), and gap. */
const STATUS_BADGE_CHROME_REM = 1 + 0.5 + ICON_GAP_REM;
/** Outlined badge padding (`px-1.5`) and its 1px borders. */
const OUTLINED_BADGE_CHROME_REM = 0.75 + 2 / PROJECT_LIST_ROOT_FONT_PX;
/** Gap between dependency badges (`gap-1`). */
const BADGE_GAP_REM = 0.25;
/** A 24px person avatar. */
const AVATAR_REM = 24 / PROJECT_LIST_ROOT_FONT_PX;
/** A checkbox (`size-4`, `size-5` on coarse pointers). */
const CHECKBOX_REM = 1.25;
/** Text field padding (`px-1`) and transparent 1px borders. */
const INPUT_CHROME_REM = 0.5 + 2 / PROJECT_LIST_ROOT_FONT_PX;
/** A custom date button's padding (`px-1`), calendar icon, and gap. */
const CUSTOM_DATE_CHROME_REM = 0.5 + CELL_ICON_REM + ICON_GAP_REM;
/** The clear button (`w-7`) and gap beside a set custom date. */
const CUSTOM_DATE_CLEAR_REM = 0.25 + 1.75;
/** A choice button's padding (`px-1.5`), gap (`gap-2`), and chevron. */
const CHOICE_CHROME_REM = 0.75 + 0.5 + CELL_ICON_REM;
/** Widest automatic header; longer names truncate until the column is resized. */
const PROJECT_LIST_HEADER_MAX_REM = 16;
/** Estimated width of one character, in rem, when text is not measured. */
const ESTIMATED_CHARACTER_REM: Record<ProjectTaskListTextStyle, number> = { cell: 0.42, badge: 0.36, badgeStrong: 0.37 };
/** The name cell's chrome around its title. */
const NAME_CHROME_REM = 1.35;
const PROJECT_LIST_NAME_WIDTH = { min: 12, autoMin: 16, max: 21.5 };
/** Narrowest manual and automatic width of a non-name column; narrower content still shows its header in full. */
const PROJECT_LIST_COLUMN_MIN_REM = 4;
const PROJECT_LIST_MANUAL_MAX_REM = 64;
const PROJECT_LIST_COLUMN_CONTENT_MAX = {
  status: 12,
  start: 12,
  due: 12,
  priority: 10,
  assignee: 12,
  reviewer: 12,
  estimate: 8,
  scheduled: 11.2,
  dependencies: 14,
  custom: 14,
} satisfies Record<ProjectCoreTaskListColumn | "custom", number>;

function normalizeProjectTaskListGridInput(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): ProjectTaskListGridInput {
  return "columns" in input ? input : { columns: input };
}

function isProjectTaskListCoreColumn(value: string): value is ProjectCoreTaskListColumn {
  return PROJECT_TASK_LIST_COLUMNS.includes(value as ProjectCoreTaskListColumn);
}

function isProjectTaskListResizableColumn(
  value: string,
  customFieldIds?: ReadonlySet<string>,
): value is ProjectTaskListResizableColumn {
  if (value === "name" || isProjectTaskListCoreColumn(value)) return true;
  const customFieldId = customFieldIdFromTaskListColumn(value as ProjectTaskListColumn);
  if (!customFieldId) return false;
  return customFieldIds ? customFieldIds.has(customFieldId) : true;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function projectTaskListColumnLabel(input: ProjectTaskListGridInput, column: ProjectTaskListColumn): string {
  return input.columnLabel?.(column) ?? column;
}

function projectTaskListTextWidthRem(input: ProjectTaskListGridInput, text: string, style: ProjectTaskListTextStyle): number {
  const normalized = text.trim();
  if (!normalized) return 0;
  return input.measureText?.(normalized, style) ?? Array.from(normalized).length * ESTIMATED_CHARACTER_REM[style];
}

function cellContent(texts: readonly string[], style: ProjectTaskListTextStyle, chromeRem: number): ProjectTaskListCellContent {
  return { texts, style, chromeRem };
}

/** Width a cell needs to show one content item without truncating it. */
function projectTaskListContentWidthRem(input: ProjectTaskListGridInput, content: ProjectTaskListCellContent): number {
  const textWidth = content.texts.reduce((total, text) => total + projectTaskListTextWidthRem(input, text, content.style), 0);
  return CELL_PADDING_REM + content.chromeRem + textWidth + FIT_SLACK_REM;
}

/** Width a header needs to show its icon and full label. */
function projectTaskListHeaderWidthRem(input: ProjectTaskListGridInput, label: string): number {
  return CELL_PADDING_REM + (input.headerIcons ? HEADER_ICON_REM : 0) + projectTaskListTextWidthRem(input, label, "cell")
    + FIT_SLACK_REM;
}

export function clampProjectTaskListManualColumnWidth(
  column: ProjectTaskListResizableColumn,
  width: number,
): number {
  const min = column === "name" ? PROJECT_LIST_NAME_WIDTH.min : PROJECT_LIST_COLUMN_MIN_REM;
  return Math.min(PROJECT_LIST_MANUAL_MAX_REM, Math.max(min, width));
}

function formatProjectTaskListRem(value: number): string {
  const rounded = Math.round(value * 100) / 100;
  return `${rounded}rem`;
}

export function roundProjectTaskListColumnWidthRem(width: number): number {
  return Math.round(width * 100) / 100;
}

function projectTaskListColumnBounds(column: ProjectTaskListColumn): ProjectTaskListColumnWidthBounds {
  const contentMax = isProjectTaskListCoreColumn(column)
    ? PROJECT_LIST_COLUMN_CONTENT_MAX[column]
    : PROJECT_LIST_COLUMN_CONTENT_MAX.custom;
  return { min: PROJECT_LIST_COLUMN_MIN_REM, autoMin: PROJECT_LIST_COLUMN_MIN_REM, contentMax };
}

function projectTaskListManualWidth(
  input: ProjectTaskListGridInput,
  column: ProjectTaskListResizableColumn,
): number | undefined {
  const width = input.columnWidths?.[column];
  return typeof width === "number" && Number.isFinite(width)
    ? clampProjectTaskListManualColumnWidth(column, width)
    : undefined;
}

/** Dates on the 28th of every month of today's year, so the widest month name in the column's format fits. */
function projectTaskListSampleDates(today: string | undefined): string[] {
  const year = today?.slice(0, 4);
  if (!year || !/^\d{4}$/.test(year)) return [];
  return Array.from({ length: 12 }, (_, month) => `${year}-${String(month + 1).padStart(2, "0")}-28`);
}

function projectTaskListDateContents(
  input: ProjectTaskListGridInput,
  column: "start" | "due",
): ProjectTaskListCellContent[] {
  const values = new Map<string, { date: string; time?: string }>();
  for (const date of projectTaskListSampleDates(input.today)) values.set(date, { date });
  for (const task of input.tasks ?? []) {
    const date = column === "start" ? task.startDate : task.dueDate;
    const time = column === "start" ? task.startTime : task.dueTime;
    if (date) values.set(time ? `${date} ${time}` : date, { date, time });
  }
  const texts = [
    input.emptyDateLabel?.(column) ?? "",
    ...Array.from(values, ([key, value]) => input.dateLabel?.(column, value.date, value.time) ?? key),
  ];
  return texts.map((text) => cellContent([text], "cell", 0));
}

function projectTaskListCustomFieldContents(
  input: ProjectTaskListGridInput,
  field: ProjectCustomField,
): ProjectTaskListCellContent[] {
  const tasks = input.tasks ?? [];
  const values = tasks.map((task) => input.customFieldCellLabel?.(task, field)).filter((value): value is string => Boolean(value));
  const empty = input.emptyValueLabel ?? "";
  if (field.fieldType === "checkbox") return [cellContent([], "cell", CHECKBOX_REM)];
  if (field.fieldType === "date") {
    return [
      cellContent([empty], "cell", CUSTOM_DATE_CHROME_REM),
      ...projectTaskListSampleDates(input.today).map((date) =>
        cellContent([input.dateLabel?.(`custom:${field.id}`, date) ?? date], "cell", CUSTOM_DATE_CHROME_REM + CUSTOM_DATE_CLEAR_REM)
      ),
      ...values.map((value) => cellContent([value], "cell", CUSTOM_DATE_CHROME_REM + CUSTOM_DATE_CLEAR_REM)),
    ];
  }
  if (projectCustomFieldUsesOptions(field.fieldType)) {
    const options = field.fieldType === "multi_select" ? [] : input.customFieldOptionNames?.(field) ?? [];
    return [empty, ...options, ...values].map((text) => cellContent([text], "cell", CHOICE_CHROME_REM));
  }
  const iconRem = projectCustomFieldHasCellIcon(field.fieldType)
    ? CELL_ICON_REM + ICON_GAP_REM
    : 0;
  return [empty, ...values].map((text) => cellContent([text], "cell", iconRem + INPUT_CHROME_REM));
}

/** Everything a column's cells can show, which its automatic width must fit. */
function projectTaskListColumnContents(input: ProjectTaskListGridInput, column: ProjectTaskListColumn): ProjectTaskListCellContent[] {
  const tasks = input.tasks ?? [];
  if (column === "status") {
    return (input.statuses ?? []).map((status) => cellContent([status.name], "badgeStrong", STATUS_BADGE_CHROME_REM));
  }
  if (column === "start" || column === "due") return projectTaskListDateContents(input, column);
  if (column === "priority") {
    const labels = [
      ...(input.priorityLabels ?? []),
      ...tasks.map((task) => input.priorityLabel?.(task.priority) ?? task.priority),
    ];
    return labels.map((label) => cellContent([label], "cell", CELL_ICON_REM + ICON_GAP_REM));
  }
  if (column === "assignee" || column === "reviewer") return [cellContent([], "cell", AVATAR_REM)];
  if (column === "estimate") {
    return tasks
      .filter((task) => task.estimateMinutes !== undefined)
      .map((task) => cellContent(
        [input.estimateLabel?.(task.estimateMinutes ?? 0) ?? String(task.estimateMinutes)],
        "badge",
        OUTLINED_BADGE_CHROME_REM,
      ));
  }
  if (column === "scheduled") {
    return tasks.map((task) => cellContent([input.scheduledLabel?.(task.id) ?? ""], "badge", OUTLINED_BADGE_CHROME_REM));
  }
  if (column === "dependencies") {
    return tasks.map((task) => {
      const labels = input.dependencyLabels?.(task) ?? [];
      const chrome = labels.length * OUTLINED_BADGE_CHROME_REM + Math.max(0, labels.length - 1) * BADGE_GAP_REM;
      return cellContent(labels, "badge", chrome);
    });
  }
  const customFieldId = customFieldIdFromTaskListColumn(column);
  const customField = customFieldId ? input.customFields?.find((field) => field.id === customFieldId) : undefined;
  return customField ? projectTaskListCustomFieldContents(input, customField) : [];
}

function projectTaskListContentsWidthRem(input: ProjectTaskListGridInput, contents: readonly ProjectTaskListCellContent[]): number {
  return contents.reduce((width, content) => Math.max(width, projectTaskListContentWidthRem(input, content)), 0);
}

function projectTaskListNameContents(input: ProjectTaskListGridInput): ProjectTaskListCellContent[] {
  return [
    input.nameLabel ?? "Name",
    ...(input.sectionLabels ?? []),
    ...(input.groupLabels ?? []),
    ...(input.tasks ?? []).map((task) => task.title),
  ].map((text) => cellContent([text], "cell", NAME_CHROME_REM - CELL_PADDING_REM - FIT_SLACK_REM));
}

function projectTaskListNameColumnWidth(input: ProjectTaskListGridInput, allowManualWidth = true): number {
  const manualWidth = projectTaskListManualWidth(input, "name");
  if (allowManualWidth && manualWidth !== undefined) return manualWidth;
  const autoWidth = input.autoColumnWidths?.name;
  if (autoWidth !== undefined) return autoWidth;
  const contentWidth = projectTaskListContentsWidthRem(input, projectTaskListNameContents(input));
  return roundProjectTaskListColumnWidthRem(Math.min(PROJECT_LIST_NAME_WIDTH.max, Math.max(PROJECT_LIST_NAME_WIDTH.autoMin, contentWidth)));
}

/**
 * Width of a list column. A manual width wins; otherwise the column fits its full header label (up to a cap) and
 * everything its cells can show (up to a per-column cap), so neither truncates in any language or font.
 */
export function projectTaskListColumnWidthRem(
  column: ProjectTaskListColumn,
  input: ProjectTaskListGridInput = { columns: [] },
  allowManualWidth = true,
): number {
  const manualWidth = projectTaskListManualWidth(input, column);
  if (allowManualWidth && manualWidth !== undefined) return manualWidth;
  const autoWidth = input.autoColumnWidths?.[column];
  if (autoWidth !== undefined) return autoWidth;
  const bounds = projectTaskListColumnBounds(column);
  const headerWidth = Math.min(
    PROJECT_LIST_HEADER_MAX_REM,
    projectTaskListHeaderWidthRem(input, projectTaskListColumnLabel(input, column)),
  );
  const contentWidth = Math.min(bounds.contentMax, projectTaskListContentsWidthRem(input, projectTaskListColumnContents(input, column)));
  return roundProjectTaskListColumnWidthRem(Math.max(bounds.autoMin, headerWidth, contentWidth));
}

export function projectTaskListResizableColumnWidthRem(
  column: ProjectTaskListResizableColumn,
  input: ProjectTaskListGridInput,
  allowManualWidth = true,
): number {
  if (column === "name") return projectTaskListNameColumnWidth(input, allowManualWidth);
  return projectTaskListColumnWidthRem(column, input, allowManualWidth);
}

/** Width that shows a column's header and every cell in full, ignoring the automatic caps. */
export function projectTaskListContentFitColumnWidthRem(
  column: ProjectTaskListResizableColumn,
  input: ProjectTaskListGridInput,
): number {
  const width = column === "name"
    ? projectTaskListContentsWidthRem(input, projectTaskListNameContents(input))
    : Math.max(
      projectTaskListHeaderWidthRem(input, projectTaskListColumnLabel(input, column)),
      projectTaskListContentsWidthRem(input, projectTaskListColumnContents(input, column)),
    );
  return clampProjectTaskListManualColumnWidth(column, width);
}

export function projectTaskListDoubleClickColumnWidthRem(
  column: ProjectTaskListResizableColumn,
  input: ProjectTaskListGridInput,
): number | undefined {
  const defaultWidth = projectTaskListResizableColumnWidthRem(column, input, false);
  const contentFitWidth = projectTaskListContentFitColumnWidthRem(column, input);
  return contentFitWidth > defaultWidth ? contentFitWidth : undefined;
}

function projectTaskListColumnResizeDraftWidths(
  widthsAtStart: ProjectTaskListColumnWidths,
  column: ProjectTaskListResizableColumn,
  widthRem: number,
): ProjectTaskListColumnWidths {
  return {
    ...widthsAtStart,
    [column]: roundProjectTaskListColumnWidthRem(clampProjectTaskListManualColumnWidth(column, widthRem)),
  };
}

export function startProjectTaskListColumnResize(input: {
  column: ProjectTaskListResizableColumn;
  pointerId: number;
  startClientX: number;
  startWidthRem: number;
  rootFontSizePx: number;
  widthsAtStart: ProjectTaskListColumnWidths;
}): ProjectTaskListColumnResizeGesture {
  const rootFontSizePx = Number.isFinite(input.rootFontSizePx) && input.rootFontSizePx > 0
    ? input.rootFontSizePx
    : 16;
  return {
    column: input.column,
    pointerId: input.pointerId,
    startClientX: input.startClientX,
    startWidthRem: input.startWidthRem,
    rootFontSizePx,
    widthsAtStart: input.widthsAtStart,
    draftWidths: projectTaskListColumnResizeDraftWidths(input.widthsAtStart, input.column, input.startWidthRem),
    moved: false,
  };
}

export function moveProjectTaskListColumnResize(
  gesture: ProjectTaskListColumnResizeGesture,
  clientX: number,
): ProjectTaskListColumnResizeGesture {
  const deltaPx = clientX - gesture.startClientX;
  const deltaRem = deltaPx / gesture.rootFontSizePx;
  return {
    ...gesture,
    draftWidths: projectTaskListColumnResizeDraftWidths(
      gesture.widthsAtStart,
      gesture.column,
      gesture.startWidthRem + deltaRem,
    ),
    moved: gesture.moved || Math.abs(deltaPx) >= 1,
  };
}

export function doubleClickProjectTaskListColumnResizeWidths(input: {
  column: ProjectTaskListResizableColumn;
  widths: ProjectTaskListColumnWidths;
  gridInput: ProjectTaskListGridInput;
}): ProjectTaskListColumnWidths {
  const nextWidths = { ...input.widths };
  const nextWidth = projectTaskListDoubleClickColumnWidthRem(input.column, input.gridInput);
  if (nextWidth === undefined) {
    delete nextWidths[input.column];
  } else {
    nextWidths[input.column] = roundProjectTaskListColumnWidthRem(nextWidth);
  }
  return nextWidths;
}

export function keyboardProjectTaskListColumnResizeWidths(input: {
  column: ProjectTaskListResizableColumn;
  direction: -1 | 1;
  wideStep?: boolean;
  widths: ProjectTaskListColumnWidths;
  gridInput: ProjectTaskListGridInput;
}): ProjectTaskListColumnWidths {
  const step = input.wideStep ? 2 : 0.5;
  const currentWidth = projectTaskListResizableColumnWidthRem(input.column, input.gridInput);
  return {
    ...input.widths,
    [input.column]: roundProjectTaskListColumnWidthRem(
      clampProjectTaskListManualColumnWidth(input.column, currentWidth + input.direction * step),
    ),
  };
}

export function projectTaskListColumnTrack(
  column: ProjectTaskListColumn,
  input: ProjectTaskListGridInput = { columns: [] },
): string {
  return formatProjectTaskListRem(projectTaskListColumnWidthRem(column, input));
}

export function projectTaskListGridColumnWidths(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): number[] {
  const normalizedInput = normalizeProjectTaskListGridInput(input);
  return [
    PROJECT_LIST_SELECTION_TRACK_REM,
    PROJECT_LIST_EXPAND_TRACK_REM,
    projectTaskListNameColumnWidth(normalizedInput),
    ...normalizedInput.columns.map((column) => projectTaskListColumnWidthRem(column, normalizedInput)),
    PROJECT_LIST_ADD_COLUMN_TRACK_REM,
  ];
}

/** Width of the selection and expand tracks that sit outside the table, before the name column. */
export function projectTaskListGutterWidth(): string {
  return formatProjectTaskListRem(PROJECT_LIST_SELECTION_TRACK_REM + PROJECT_LIST_EXPAND_TRACK_REM);
}

/** Width of the name track, where content of a whole-row control such as the add task field lines up with the next column. */
export function projectTaskListNameTrackWidth(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): string {
  return formatProjectTaskListRem(projectTaskListNameColumnWidth(normalizeProjectTaskListGridInput(input)));
}

export function projectTaskListGridTemplate(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): string {
  return projectTaskListGridColumnWidths(input)
    .map(formatProjectTaskListRem)
    .join(" ");
}

export function projectTaskListLeadingGridTemplate(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): string {
  const leadingTracks = projectTaskListGridColumnWidths(input)
    .slice(0, 2)
    .map(formatProjectTaskListRem);
  return [...leadingTracks, "minmax(0, 1fr)"].join(" ");
}

export function projectTaskListGridMinWidth(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): string {
  const remWidth = projectTaskListGridColumnWidths(input).reduce((total, width) => total + width, 0);
  return formatProjectTaskListRem(remWidth);
}

export function parseProjectTaskListColumnWidths(
  value: string | undefined,
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskListColumnWidths {
  if (value === undefined) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    return {};
  }
  const rawWidths = isRecord(parsed) && "widths" in parsed ? parsed.widths : parsed;
  if (!isRecord(rawWidths)) return {};
  const widths: ProjectTaskListColumnWidths = {};
  for (const [column, rawWidth] of Object.entries(rawWidths)) {
    if (!isProjectTaskListResizableColumn(column, customFieldIds)) continue;
    if (typeof rawWidth !== "number" || !Number.isFinite(rawWidth)) continue;
    widths[column] = clampProjectTaskListManualColumnWidth(column, rawWidth);
  }
  return widths;
}

export function taskListColumnWidthsPreferenceValue(widths: ProjectTaskListColumnWidths): string {
  const normalizedEntries = Object.entries(widths)
    .filter((entry): entry is [ProjectTaskListResizableColumn, number] =>
      isProjectTaskListResizableColumn(entry[0])
      && typeof entry[1] === "number"
      && Number.isFinite(entry[1])
    )
    .map(([column, width]) => [column, clampProjectTaskListManualColumnWidth(column, width)] as const)
    .sort(([firstColumn], [secondColumn]) => firstColumn.localeCompare(secondColumn));
  return JSON.stringify({ widths: Object.fromEntries(normalizedEntries) });
}

/** Restore one list view's manual column widths. */
export function taskListColumnWidthsForView(
  preferences: readonly ProjectViewPreference[],
  viewId: string | null | undefined,
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskListColumnWidths {
  if (!viewId) return {};
  const preference = preferences.find((entry) =>
    entry.viewId === viewId && entry.preferenceKey === TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY
  );
  return parseProjectTaskListColumnWidths(preference?.preferenceValue, customFieldIds);
}

/**
 * Automatic width of the name column and every listed column, ignoring manual widths. Compute it once per content
 * change and pass it as `autoColumnWidths`, since fitting reads every task.
 */
export function projectTaskListAutoColumnWidths(input: ProjectTaskListGridInput): ProjectTaskListColumnWidths {
  const contentInput = { ...input, autoColumnWidths: undefined };
  const widths: ProjectTaskListColumnWidths = { name: projectTaskListNameColumnWidth(contentInput, false) };
  for (const column of input.columns) widths[column] = projectTaskListColumnWidthRem(column, contentInput, false);
  return widths;
}
