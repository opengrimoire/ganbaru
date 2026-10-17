import { Temporal } from "@js-temporal/polyfill";

/** Locales whose month names a typed date may always use, beside the active one. */
const FALLBACK_MONTH_LOCALES = ["en"] as const;

/** Fewest letters of a month name that identify it, so "ma" stays ambiguous between March and May. */
const MONTH_PREFIX_MIN_LENGTH = 3;

const NUMERIC_DATE = /^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})$/u;
const TRAILING_TIME = /(?:^|[\s,])(?:at |a las |a la )?(\d{1,2})(?::(\d{0,2}))?\s*(a\.?m\.?|p\.?m\.?)?$/u;

/** A date typed after the at sign, with the wall time typed after it when there is one. */
export interface NotesTypedDate {
  date: Temporal.PlainDate;
  time: Temporal.PlainTime | null;
}

/** The query text that names a day, and the wall time typed after it. */
export interface NotesTypedTimeSplit {
  dateQuery: string;
  time: Temporal.PlainTime | null;
}

/**
 * Normalize typed date text for matching: no diacritics, single spaces, and lowercase, so "Mié" and "mie" match.
 *
 * @param query The text typed after the at sign.
 * @returns The comparable form of the text.
 */
export function normalizeNotesDateQuery(query: string): string {
  return query
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .trim()
    .replace(/\s+/gu, " ")
    .toLocaleLowerCase();
}

/**
 * Split a trailing wall time from typed date text. A time needs a colon or a meridiem, so the day in "oct 30" stays
 * part of the date; minutes may be partial, as in "12:1" for 12:01.
 *
 * @param query Normalized date text.
 * @returns The remaining date text and the time, or the whole text with no time when none is typed or it is invalid.
 */
export function splitNotesTypedTime(query: string): NotesTypedTimeSplit {
  const match = TRAILING_TIME.exec(query);
  if (!match) return { dateQuery: query, time: null };
  const [whole, hourText, minuteText, meridiem] = match;
  if (minuteText === undefined && meridiem === undefined) return { dateQuery: query, time: null };
  let hour = Number(hourText);
  const minute = minuteText ? Number(minuteText) : 0;
  if (meridiem) {
    if (hour < 1 || hour > 12) return { dateQuery: query, time: null };
    const afternoon = meridiem.startsWith("p");
    hour = (hour % 12) + (afternoon ? 12 : 0);
  }
  if (hour > 23 || minute > 59) return { dateQuery: query, time: null };
  const dateQuery = query.slice(0, query.length - whole.length).replace(/[\s,]+$/u, "");
  return { dateQuery, time: Temporal.PlainTime.from({ hour, minute }) };
}

const monthNamesByLocales = new Map<string, string[][]>();

/** Month names, long and short, of the given locales in their comparable form, indexed by month number minus one. */
function monthNames(locales: readonly string[]): string[][] {
  const key = locales.join("|");
  const cached = monthNamesByLocales.get(key);
  if (cached) return cached;
  const names: string[][] = Array.from({ length: 12 }, () => []);
  for (const locale of locales) {
    for (const month of ["long", "short"] as const) {
      const format = new Intl.DateTimeFormat(locale, { month, timeZone: "UTC" });
      for (let index = 0; index < 12; index += 1) {
        const name = normalizeNotesDateQuery(format.format(Date.UTC(2026, index, 15))).replace(/\.$/u, "");
        if (name && !names[index].includes(name)) names[index].push(name);
      }
    }
  }
  monthNamesByLocales.set(key, names);
  return names;
}

/** The month a word names by a full name or a prefix of at least three letters, or null when none or several match. */
function monthFromWord(word: string, names: readonly string[][]): number | null {
  const typed = word.replace(/\.$/u, "");
  if (typed.length < MONTH_PREFIX_MIN_LENGTH) return null;
  const matches = names
    .map((candidates, index) => (candidates.some((name) => name.startsWith(typed)) ? index + 1 : null))
    .filter((month): month is number => month !== null);
  return matches.length === 1 ? matches[0] : null;
}

function plainDate(year: number, month: number, day: number): Temporal.PlainDate | null {
  try {
    return Temporal.PlainDate.from({ year, month, day }, { overflow: "reject" });
  } catch {
    return null;
  }
}

/**
 * Read a calendar date typed as numbers or with a month name: "2025-12-2", "oct 30", "October 30, 2026", or
 * "30 de octubre". Month names come from the active locale and English. A date without a year falls in the current
 * year.
 *
 * @param dateQuery Normalized date text without a trailing time.
 * @param options The current day and the active locale.
 * @returns The date, or null when the text is not a complete, real date.
 */
export function parseNotesTypedCalendarDate(
  dateQuery: string,
  options: { today: Temporal.PlainDate; locale: string },
): Temporal.PlainDate | null {
  const numeric = NUMERIC_DATE.exec(dateQuery);
  if (numeric) return plainDate(Number(numeric[1]), Number(numeric[2]), Number(numeric[3]));
  const words = dateQuery.replace(/,/gu, " ").split(" ").filter((word) => word && word !== "de" && word !== "of");
  if (words.length < 2 || words.length > 3) return null;
  const names = monthNames([options.locale, ...FALLBACK_MONTH_LOCALES]);
  const [first, second, third] = words;
  const yearWord = third ?? null;
  if (yearWord !== null && !/^\d{4}$/u.test(yearWord)) return null;
  const year = yearWord === null ? options.today.year : Number(yearWord);
  const leadingMonth = monthFromWord(first, names);
  if (leadingMonth !== null && /^\d{1,2}$/u.test(second)) return plainDate(year, leadingMonth, Number(second));
  const trailingMonth = monthFromWord(second, names);
  if (trailingMonth !== null && /^\d{1,2}$/u.test(first)) return plainDate(year, trailingMonth, Number(first));
  return null;
}
