export const format = {
  relativeMinutesNow: "Now",
  relativeMinutesFuture: (count: number) => `in ${count} min`,
  relativeMinutesPast: (count: number) => `${count} min ago`,
  relativeDaysToday: "Today",
  relativeDaysFuture: (count: number) => count === 1 ? "in 1 day" : `in ${count} days`,
  relativeDaysPast: (count: number) => count === 1 ? "1 day ago" : `${count} days ago`,
} as const;
