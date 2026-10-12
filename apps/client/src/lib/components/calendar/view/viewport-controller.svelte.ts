import type { TimezoneAbbrMode } from "$lib/calendar/utils";

const MAX_TIMEZONES = 5;

/** Owns shared timeline scroll state and configured timezone rows. */
export class CalendarViewViewportController {
  timezones = $state<string[]>([]);
  timezoneAbbreviationMode = $state<TimezoneAbbrMode>("acronym");
  scrollMinute = $state(-1);
  viewWrapper = $state<HTMLDivElement>();

  constructor(private readonly localTimezone: () => string) {
    this.timezones = [localTimezone()];
  }

  addTimezone(timezone: string): void {
    if (this.timezones.length >= MAX_TIMEZONES || this.timezones.includes(timezone)) return;
    this.timezones = [...this.timezones, timezone];
  }

  removeTimezone(index: number): void {
    if (index < 0 || index >= this.timezones.length) return;
    if (this.timezones[index] === this.localTimezone()) return;
    this.timezones = this.timezones.filter((_, current) => current !== index);
  }

  reorderTimezone(from: number, to: number): void {
    if (from < 0 || to < 0 || from >= this.timezones.length || to >= this.timezones.length
      || from === to) return;
    const next = [...this.timezones];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    this.timezones = next;
  }
}
