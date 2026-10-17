export interface DatePickerDay {
  day: number;
  dateStr: string;
  currentMonth: boolean;
  selected: boolean;
  today: boolean;
}

/** Keeps the roving day on or after the picker's optional minimum date. */
export function clampDatePickerActiveDate(activeDate: string, minDate?: string): string {
  return minDate && activeDate < minDate ? minDate : activeDate;
}

/** Resolves an enabled day in a target month, or rejects a wholly disabled month. */
export function datePickerMonthNavigationTarget(
  activeDate: string,
  targetYear: number,
  targetMonth: number,
  minDate?: string,
): string | null {
  const lastDay = new Date(targetYear, targetMonth, 0).getDate();
  const lastDate = `${targetYear}-${String(targetMonth).padStart(2, "0")}-${String(lastDay).padStart(2, "0")}`;
  if (minDate && lastDate < minDate) return null;
  const parsedDay = Number(activeDate.slice(8, 10));
  const preferredDay = Number.isInteger(parsedDay) && parsedDay > 0
    ? Math.min(parsedDay, lastDay)
    : 1;
  const preferredDate = `${targetYear}-${String(targetMonth).padStart(2, "0")}-${String(preferredDay).padStart(2, "0")}`;
  return clampDatePickerActiveDate(preferredDate, minDate);
}

export function buildCalendarGrid(
  year: number,
  month: number,
  selectedStr: string,
): DatePickerDay[] {
  const now = new Date();
  const todayStr = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  const firstDow = new Date(year, month - 1, 1).getDay();
  const startOffset = firstDow === 0 ? 6 : firstDow - 1;
  const daysInMonth = new Date(year, month, 0).getDate();
  const daysInPrevMonth = new Date(year, month - 1, 0).getDate();
  const days: DatePickerDay[] = [];

  for (let i = startOffset - 1; i >= 0; i--) {
    const day = daysInPrevMonth - i;
    const prevMonth = month === 1 ? 12 : month - 1;
    const prevYear = month === 1 ? year - 1 : year;
    const dateStr = `${prevYear}-${String(prevMonth).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    days.push({
      day,
      dateStr,
      currentMonth: false,
      selected: dateStr === selectedStr,
      today: dateStr === todayStr,
    });
  }

  for (let day = 1; day <= daysInMonth; day++) {
    const dateStr = `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    days.push({
      day,
      dateStr,
      currentMonth: true,
      selected: dateStr === selectedStr,
      today: dateStr === todayStr,
    });
  }

  const totalNeeded = Math.ceil(days.length / 7) * 7;
  const nextMonth = month === 12 ? 1 : month + 1;
  const nextYear = month === 12 ? year + 1 : year;
  for (let day = 1; days.length < totalNeeded; day++) {
    const dateStr = `${nextYear}-${String(nextMonth).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    days.push({
      day,
      dateStr,
      currentMonth: false,
      selected: dateStr === selectedStr,
      today: dateStr === todayStr,
    });
  }

  return days;
}
