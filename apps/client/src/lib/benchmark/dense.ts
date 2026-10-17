/**
 * Deterministic dense calendar dataset generator.
 *
 * Dataset v1 creates `stackCount` overlapping one-hour Pomodoro events at the
 * start of every hour, plus three all-day events for each day in
 * `[anchor - yearRadius, anchor + yearRadius)`. Detail profile `d1` adds
 * realistic event metadata without recurrence, so the benchmark isolates the
 * cost of long calendar history and dense visible days instead of
 * recurring-template expansion.
 */
import { DENSE_TIMED_POMODORO_CONFIG } from "./pomodoro-history";
import {
  benchmarkDatasetId,
  type BenchmarkDatasetProfile,
  type BenchmarkEventDraft,
} from "./types";
import { PALETTE_SIZE } from "$lib/calendar/types";

const MS_PER_DAY = 86_400_000;
const HOURS_PER_DAY = 24;
const ALL_DAY_EVENTS_PER_DAY = 3;

interface DateParts {
  year: number;
  month: number;
  day: number;
}

export interface DenseCalendarDateRange {
  start: string;
  endExclusive: string;
  days: number;
}

const DETAIL_TEMPLATES = [
  {
    title: "Focus block",
    action: "make progress on the planned milestone",
    category: "deep work",
    location: "Home office",
  },
  {
    title: "Project review",
    action: "review decisions, risks, and next steps",
    category: "planning",
    location: "Desk",
  },
  {
    title: "Team sync",
    action: "align on blockers and ownership",
    category: "collaboration",
    location: "Meeting room",
  },
  {
    title: "Admin pass",
    action: "clear inbox items and update records",
    category: "admin",
    location: "Office",
  },
  {
    title: "Personal routine",
    action: "handle the recurring personal routine",
    category: "personal",
    location: "Apartment",
  },
] as const;

const PROJECTS = [
  "calendar",
  "planning",
  "health",
  "finances",
  "learning",
  "maintenance",
] as const;

const LOCATIONS = [
  { name: "Home office", lat: 25.6866, lng: -100.3161 },
  { name: "Desk", lat: 25.6744, lng: -100.3182 },
  { name: "Meeting room", lat: 25.6861, lng: -100.3169 },
  { name: "Office", lat: 25.6791, lng: -100.3084 },
  { name: "Apartment", lat: 25.6912, lng: -100.3216 },
] as const;

const ALL_DAY_TEMPLATES = [
  {
    title: "Daily priority review",
    action: "review the top priorities for the day",
    category: "planning",
  },
  {
    title: "Health routine",
    action: "protect sleep, movement, meals, and recovery basics",
    category: "health",
  },
  {
    title: "Personal reminder",
    action: "keep personal life admin visible without blocking timed work",
    category: "personal",
  },
] as const;

function pad2(n: number): string {
  return n < 10 ? `0${n}` : `${n}`;
}

function partsFromDate(date: Date): DateParts {
  return {
    year: date.getFullYear(),
    month: date.getMonth() + 1,
    day: date.getDate(),
  };
}

function dateToUtcMs(date: DateParts): number {
  return Date.UTC(date.year, date.month - 1, date.day);
}

function partsFromUtcMs(ms: number): DateParts {
  const date = new Date(ms);
  return {
    year: date.getUTCFullYear(),
    month: date.getUTCMonth() + 1,
    day: date.getUTCDate(),
  };
}

function addYears(date: DateParts, years: number): DateParts {
  return {
    year: date.year + years,
    month: date.month,
    day: date.day,
  };
}

function addDays(date: DateParts, days: number): DateParts {
  return partsFromUtcMs(dateToUtcMs(date) + days * MS_PER_DAY);
}

function daysBetween(start: DateParts, endExclusive: DateParts): number {
  return Math.round((dateToUtcMs(endExclusive) - dateToUtcMs(start)) / MS_PER_DAY);
}

function formatDate(date: DateParts): string {
  return `${date.year}-${pad2(date.month)}-${pad2(date.day)}`;
}

function parseDate(value: string): DateParts {
  const [year, month, day] = value.split("-").map(Number);
  return { year, month, day };
}

export function denseCalendarDateRange(
  dataset: BenchmarkDatasetProfile,
  anchor: Date,
): DenseCalendarDateRange {
  const anchorParts = partsFromDate(anchor);
  const startParts = addYears(anchorParts, -dataset.yearRadius);
  const endParts = addYears(anchorParts, dataset.yearRadius);
  return {
    start: formatDate(startParts),
    endExclusive: formatDate(endParts),
    days: daysBetween(startParts, endParts),
  };
}

export function countDenseCalendarEvents(
  dataset: BenchmarkDatasetProfile,
  anchor: Date,
): number {
  return denseCalendarDateRange(dataset, anchor).days * eventsPerDay(dataset);
}

function timedEventsPerDay(dataset: BenchmarkDatasetProfile): number {
  return HOURS_PER_DAY * dataset.stackCount;
}

function eventsPerDay(dataset: BenchmarkDatasetProfile): number {
  return timedEventsPerDay(dataset) + ALL_DAY_EVENTS_PER_DAY;
}

function templateFor(dayIndex: number, hour: number, stackIndex: number) {
  return DETAIL_TEMPLATES[(dayIndex + hour + stackIndex) % DETAIL_TEMPLATES.length];
}

function projectFor(dayIndex: number, hour: number, stackIndex: number): string {
  return PROJECTS[(dayIndex + hour * 2 + stackIndex) % PROJECTS.length];
}

function sourceUidFor(
  dataset: BenchmarkDatasetProfile,
  date: string,
  hour: number,
  stackIndex: number,
): string {
  return `dense-${dataset.version}-s${dataset.stackCount}-${dataset.detailProfile}-${date}T${pad2(hour)}-stack${stackIndex + 1}`;
}

function allDaySourceUidFor(
  dataset: BenchmarkDatasetProfile,
  date: string,
  allDayIndex: number,
): string {
  return `dense-${dataset.version}-s${dataset.stackCount}-${dataset.detailProfile}-${date}-allday${allDayIndex + 1}`;
}

function attendeesFor(date: string, hour: number, stackIndex: number) {
  if (stackIndex === 0) {
    return [
      {
        id: `dense-att-${date}-${pad2(hour)}-${stackIndex}-lead`,
        name: "Benchmark Lead",
        email: "lead@benchmark.local",
        role: "chair" as const,
        status: "accepted" as const,
        rsvp: true,
      },
    ];
  }
  if (stackIndex === 1) {
    return [
      {
        id: `dense-att-${date}-${pad2(hour)}-${stackIndex}-guest`,
        name: "Benchmark Guest",
        email: "guest@benchmark.local",
        role: "req-participant" as const,
        status: "needs-action" as const,
        rsvp: false,
      },
    ];
  }
  return undefined;
}

function buildDenseEvent(
  dataset: BenchmarkDatasetProfile,
  date: string,
  dayIndex: number,
  hour: number,
  stackIndex: number,
): BenchmarkEventDraft {
  const template = templateFor(dayIndex, hour, stackIndex);
  const project = projectFor(dayIndex, hour, stackIndex);
  const location = LOCATIONS[(dayIndex + stackIndex) % LOCATIONS.length];
  const start = `${date} ${pad2(hour)}:00`;
  const endDate = hour === 23 ? formatDate(addDays(parseDate(date), 1)) : date;
  const end = `${endDate} ${pad2((hour + 1) % HOURS_PER_DAY)}:00`;
  const datasetId = benchmarkDatasetId(dataset);
  const stackLabel = stackIndex + 1;
  return {
    title: `${template.title}: ${project} ${pad2(hour)}:00 #${stackLabel}`,
    start,
    end,
    sourceUid: sourceUidFor(dataset, date, hour, stackIndex),
    description: [
      `Agenda: ${template.action}.`,
      `Context: ${project} work with stacked hour ${stackLabel}.`,
      "Preparation: review the previous outcome, capture the next action, and leave a short summary.",
    ].join(" "),
    notifications: [10],
    color: (dayIndex + hour + stackIndex * 5) % PALETTE_SIZE,
    location: `${template.location}, ${location.name}`,
    url: `https://benchmark.local/${datasetId}/${date}/${pad2(hour)}/${stackLabel}`,
    transparency: stackIndex === 2 ? "transparent" : "opaque",
    status: (dayIndex + hour + stackIndex) % 11 === 0 ? "tentative" : "confirmed",
    visibility: template.category === "personal" ? "private" : "public",
    priority: 4 + (stackIndex % 3),
    categories: ["benchmark", dataset.detailProfile, template.category, project],
    geo: { lat: location.lat, lng: location.lng },
    extendedProperties: {
      "X-BENCHMARK-DATASET": datasetId,
      "X-BENCHMARK-DAY-INDEX": String(dayIndex),
      "X-BENCHMARK-STACK": String(stackLabel),
    },
    organizer: {
      name: "Benchmark Calendar",
      email: "owner@benchmark.local",
    },
    alarms: [
      {
        id: `dense-alarm-${date}-${pad2(hour)}-${stackIndex}`,
        action: "display",
        triggerType: "relative",
        triggerValue: "-PT10M",
        description: `${template.title} starts in 10 minutes`,
      },
    ],
    attendees: attendeesFor(date, hour, stackIndex),
    pomodoroConfig: DENSE_TIMED_POMODORO_CONFIG,
    guestPermissions: {
      canModify: false,
      canInviteOthers: stackIndex !== 2,
      canSeeOtherGuests: true,
    },
  };
}

function buildDenseAllDayEvent(
  dataset: BenchmarkDatasetProfile,
  date: string,
  dayIndex: number,
  allDayIndex: number,
): BenchmarkEventDraft {
  const template = ALL_DAY_TEMPLATES[allDayIndex % ALL_DAY_TEMPLATES.length];
  const datasetId = benchmarkDatasetId(dataset);
  const slot = allDayIndex + 1;
  return {
    title: `${template.title} #${slot}`,
    start: `${date} 00:00`,
    end: `${date} 00:00`,
    sourceUid: allDaySourceUidFor(dataset, date, allDayIndex),
    allDay: true,
    description: [
      `Reminder: ${template.action}.`,
      "Context: this all-day item keeps non-hourly planning visible beside dense timed focus blocks.",
    ].join(" "),
    color: (dayIndex + allDayIndex * 3) % PALETTE_SIZE,
    status: (dayIndex + allDayIndex) % 13 === 0 ? "tentative" : "confirmed",
    visibility: template.category === "personal" ? "private" : "public",
    priority: 5 + (allDayIndex % 2),
    categories: ["benchmark", dataset.detailProfile, "all-day", template.category],
    extendedProperties: {
      "X-BENCHMARK-DATASET": datasetId,
      "X-BENCHMARK-DAY-INDEX": String(dayIndex),
      "X-BENCHMARK-ALL-DAY": String(slot),
    },
    organizer: {
      name: "Benchmark Calendar",
      email: "owner@benchmark.local",
    },
    guestPermissions: {
      canModify: false,
      canInviteOthers: true,
      canSeeOtherGuests: true,
    },
  };
}

/**
 * Generate a deterministic slice of a dense dataset. `offset` and `count`
 * let seeding stream large datasets in chunks without allocating every
 * event in the browser at once.
 */
export function generateDenseCalendarEvents(opts: {
  dataset: BenchmarkDatasetProfile;
  anchor: Date;
  offset?: number;
  count?: number;
}): BenchmarkEventDraft[] {
  const total = countDenseCalendarEvents(opts.dataset, opts.anchor);
  const offset = Math.max(0, Math.floor(opts.offset ?? 0));
  const end = opts.count === undefined
    ? total
    : Math.min(total, offset + Math.max(0, Math.floor(opts.count)));
  if (offset >= end) return [];

  const anchorParts = partsFromDate(opts.anchor);
  const startParts = addYears(anchorParts, -opts.dataset.yearRadius);
  const perDay = eventsPerDay(opts.dataset);
  const timedPerDay = timedEventsPerDay(opts.dataset);
  const events: BenchmarkEventDraft[] = [];

  for (let index = offset; index < end; index++) {
    const dayIndex = Math.floor(index / perDay);
    const withinDay = index % perDay;
    const date = formatDate(addDays(startParts, dayIndex));

    if (withinDay < timedPerDay) {
      const hour = Math.floor(withinDay / opts.dataset.stackCount);
      const stackIndex = withinDay % opts.dataset.stackCount;
      events.push(buildDenseEvent(opts.dataset, date, dayIndex, hour, stackIndex));
    } else {
      events.push(buildDenseAllDayEvent(opts.dataset, date, dayIndex, withinDay - timedPerDay));
    }
  }

  return events;
}
