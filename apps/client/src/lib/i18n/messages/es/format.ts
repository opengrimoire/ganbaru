import type { format as enFormat } from "../en/format";
import type { MessageShape } from "../types";

export const format = {
  relativeMinutesNow: "Ahora",
  relativeMinutesFuture: (count: number) => `en ${count} min`,
  relativeMinutesPast: (count: number) => `hace ${count} min`,
  relativeDaysToday: "Hoy",
  relativeDaysFuture: (count: number) => count === 1 ? "en 1 día" : `en ${count} días`,
  relativeDaysPast: (count: number) => count === 1 ? "hace 1 día" : `hace ${count} días`,
} as const satisfies MessageShape<typeof enFormat>;
