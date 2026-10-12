import { describe, expect, it } from "vitest";
import type { Calendar, CalendarEvent } from "$lib/calendar/types";
import { serializeCalendarToIcs } from "./serializer";

const baseCalendar: Calendar = {
	id: "cal-1",
	name: "Test Calendar",
	color: "",
	source: "ics",
	visible: true,
	readOnly: false,
};

function makeEvent(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
	return {
		id: "ev-1",
		title: "Sample",
		start: "2026-06-01 14:00",
		end: "2026-06-01 15:00",
		timezone: "UTC",
		calendarId: "cal-1",
		sourceUid: "sample-uid@ganbaru-ai",
		...overrides,
	};
}

/**
 * Unfold long lines per RFC 5545 (continuation lines start with a single
 * space or tab). Used so substring assertions match the logical content.
 */
function unfold(ics: string): string {
	return ics.replace(/\r\n[ \t]/g, "");
}

describe("serializeCalendarToIcs", () => {
	describe("VCALENDAR envelope", () => {
		it("emits BEGIN/END VCALENDAR with required headers", () => {
			const ics = serializeCalendarToIcs(baseCalendar, [makeEvent()], "UTC");
			expect(ics).toMatch(/^BEGIN:VCALENDAR\r\n/);
			expect(ics).toContain("PRODID:-//Ganbaru AI//Calendar//EN");
			expect(ics).toContain("VERSION:2.0");
			expect(ics).toContain("CALSCALE:GREGORIAN");
			expect(ics).toContain("METHOD:PUBLISH");
			expect(ics).toContain(`X-WR-CALNAME:${baseCalendar.name}`);
			expect(ics.trimEnd()).toMatch(/END:VCALENDAR$/);
		});

		it("uses a preserved scheduling METHOD when it is valid", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[makeEvent()],
				"UTC",
				[],
				[],
				"request",
			);

			expect(ics).toContain("METHOD:REQUEST");
		});

		it("falls back to PUBLISH for invalid preserved scheduling methods", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[makeEvent()],
				"UTC",
				[],
				[],
				"REQUEST\r\nX-BAD:1",
			);

			expect(ics).toContain("METHOD:PUBLISH");
			expect(ics).not.toContain("X-BAD:1");
		});

		it("uses CRLF line endings", () => {
			const ics = serializeCalendarToIcs(baseCalendar, [makeEvent()], "UTC");
			const lfOnlyCount = (ics.match(/(?<!\r)\n/g) ?? []).length;
			expect(lfOnlyCount).toBe(0);
		});
	});

	describe("DTSTART/DTEND emission", () => {
		it("emits non-recurring zoned events as UTC Z", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[makeEvent({ timezone: "UTC" })],
				"UTC",
			);
			expect(ics).toContain("DTSTART:20260601T140000Z");
			expect(ics).toContain("DTEND:20260601T150000Z");
		});

		it("emits all-day events with an exclusive VALUE=DATE end", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						allDay: true,
						start: "2026-06-01 00:00",
						end: "2026-06-05 00:00",
					}),
				],
				"UTC",
			);
			expect(ics).toContain("DTSTART;VALUE=DATE:20260601");
			expect(ics).toContain("DTEND;VALUE=DATE:20260606");
		});

		it("emits single-day all-day events with the next date as DTEND", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						allDay: true,
						start: "2026-05-13 00:00",
						end: "2026-05-13 00:00",
					}),
				],
				"UTC",
			);
			expect(ics).toContain("DTSTART;VALUE=DATE:20260513");
			expect(ics).toContain("DTEND;VALUE=DATE:20260514");
		});

		it("emits recurring zoned events with TZID and wall clock", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						timezone: "America/New_York",
						start: "2026-04-15 09:00",
						end: "2026-04-15 10:00",
						recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
					}),
				],
				"America/New_York",
			);
			expect(ics).toContain("DTSTART;TZID=America/New_York:20260415T090000");
			expect(ics).toContain("DTEND;TZID=America/New_York:20260415T100000");
			expect(ics).toContain("RRULE:FREQ=DAILY");
		});

		it("emits timed recurrence UNTIL as a UTC end-of-day date-time", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						timezone: "America/Mexico_City",
						start: "2021-05-09 22:00",
						end: "2021-05-10 07:00",
						recurrence: { frequency: "daily", interval: 1, end: { type: "until", date: "2021-05-09" } },
					}),
				],
				"America/Mexico_City",
			);

			expect(ics).toContain("DTSTART;TZID=America/Mexico_City:20210509T220000");
			expect(ics).toContain("RRULE:FREQ=DAILY;UNTIL=20210510T045959Z");
		});

		it("keeps all-day recurrence UNTIL as a date value", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						allDay: true,
						start: "2026-05-13 00:00",
						end: "2026-05-13 00:00",
						recurrence: { frequency: "yearly", interval: 1, end: { type: "until", date: "2028-05-13" } },
					}),
				],
				"UTC",
			);

			expect(ics).toContain("DTSTART;VALUE=DATE:20260513");
			expect(ics).toContain("RRULE:FREQ=YEARLY;UNTIL=20280513");
		});

		it("emits recurring UTC events as UTC Z (no VTIMEZONE block)", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						timezone: "UTC",
						recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
					}),
				],
				"UTC",
			);
			expect(ics).toContain("DTSTART:20260601T140000Z");
			expect(ics).not.toContain("BEGIN:VTIMEZONE");
		});
	});

	describe("VTIMEZONE stubs", () => {
		it("emits a VTIMEZONE block per unique zone among recurring events", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						sourceUid: "ny@ex",
						timezone: "America/New_York",
						start: "2026-04-15 09:00",
						end: "2026-04-15 10:00",
						recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
					}),
					makeEvent({
						sourceUid: "tokyo@ex",
						timezone: "Asia/Tokyo",
						start: "2026-04-15 09:00",
						end: "2026-04-15 10:00",
						recurrence: { frequency: "weekly", interval: 1, end: { type: "never" } },
					}),
				],
				"UTC",
			);
			expect(ics).toContain("TZID:America/New_York");
			expect(ics).toContain("TZID:Asia/Tokyo");
			const nyMatches = ics.match(/TZID:America\/New_York/g) ?? [];
			expect(nyMatches.length).toBeGreaterThanOrEqual(1);
		});

		it("does not emit a VTIMEZONE block for non-recurring events", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						timezone: "America/New_York",
						start: "2026-04-15 09:00",
						end: "2026-04-15 10:00",
					}),
				],
				"America/New_York",
			);
			expect(ics).not.toContain("BEGIN:VTIMEZONE");
		});

		it("VTIMEZONE stub has the correct UTC offset for the zone", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						timezone: "Asia/Tokyo",
						start: "2026-04-15 09:00",
						end: "2026-04-15 10:00",
						recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
					}),
				],
				"UTC",
			);
			expect(ics).toContain("TZID:Asia/Tokyo");
			expect(ics).toContain("TZOFFSETFROM:+0900");
			expect(ics).toContain("TZOFFSETTO:+0900");
		});

		it("exports preserved VTIMEZONE definitions before generated stubs", () => {
			const preservedTimezone = [
				"vtimezone",
				[
					["tzid", {}, "text", "America/New_York"],
					["last-modified", {}, "date-time", "2026-01-01T00:00:00Z"],
				],
				[
					[
						"standard",
						[
							["dtstart", {}, "date-time", "1970-01-01T00:00:00"],
							["tzoffsetfrom", {}, "utc-offset", "-05:00"],
							["tzoffsetto", {}, "utc-offset", "-05:00"],
							["tzname", {}, "text", "Custom standard"],
						],
						[],
					],
				],
			];
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						timezone: "America/New_York",
						recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
					}),
				],
				"UTC",
				[preservedTimezone],
			);

			expect(ics).toContain("BEGIN:VTIMEZONE");
			expect(ics).toContain("TZID:America/New_York");
			expect(ics).toContain("LAST-MODIFIED:20260101T000000Z");
			expect(ics).toContain("TZNAME:Custom standard");
			expect((ics.match(/TZID:America\/New_York/g) ?? []).length).toBe(1);
		});
	});

	describe("text escaping", () => {
		it("escapes backslash, comma, semicolon, and newline in TEXT values", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						title: "Title; with, special\\chars\nand newline",
					}),
				],
				"UTC",
			);
			expect(ics).toContain("SUMMARY:Title\\; with\\, special\\\\chars\\nand newline");
		});

		it("does not escape URL values", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[makeEvent({ url: "https://meet.example.com/xyz?id=1&t=2" })],
				"UTC",
			);
			expect(ics).toContain("URL:https://meet.example.com/xyz?id=1&t=2");
		});
	});

	describe("optional fields", () => {
		it("emits STATUS, TRANSP, CLASS, PRIORITY, SEQUENCE", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						status: "tentative",
						transparency: "transparent",
						visibility: "private",
						priority: 5,
						sequence: 3,
					}),
				],
				"UTC",
			);
			expect(ics).toContain("STATUS:TENTATIVE");
			expect(ics).toContain("TRANSP:TRANSPARENT");
			expect(ics).toContain("CLASS:PRIVATE");
			expect(ics).toContain("PRIORITY:5");
			expect(ics).toContain("SEQUENCE:3");
		});

		it("emits CATEGORIES and GEO", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						categories: ["Work", "Team"],
						geo: { lat: 40.7128, lng: -74.006 },
					}),
				],
				"UTC",
			);
			expect(ics).toContain("CATEGORIES:Work,Team");
			expect(ics).toContain("GEO:40.7128;-74.006");
		});

		it("emits ORGANIZER and ATTENDEE properties", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						organizer: { name: "Alice", email: "alice@example.com" },
						attendees: [
							{
								id: "a1",
								name: "Bob",
								email: "bob@example.com",
								role: "req-participant",
								status: "accepted",
								rsvp: true,
							},
						],
					}),
				],
				"UTC",
			);
			const unfolded = unfold(ics);
			expect(unfolded).toContain("ORGANIZER;CN=Alice:mailto:alice@example.com");
			expect(unfolded).toContain(
				"ATTENDEE;CN=Bob;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;RSVP=TRUE:mailto:bob@example.com",
			);
		});

		it("quotes and caret-escapes CN parameter values", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						organizer: { name: 'Doe, Jane "JJ"', email: "jane@example.com" },
						attendees: [
							{
								id: "a1",
								name: "Team; lead\nremote",
								email: "lead@example.com",
								role: "req-participant",
								status: "needs-action",
								rsvp: true,
							},
						],
					}),
				],
				"UTC",
			);
			const unfolded = unfold(ics);
			expect(unfolded).toContain('ORGANIZER;CN="Doe, Jane ^\'JJ^\'":mailto:jane@example.com');
			expect(unfolded).toContain(
				'ATTENDEE;CN="Team; lead^nremote";ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:lead@example.com',
			);
		});

		it("emits VALARM blocks", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						alarms: [
							{
								id: "al1",
								action: "display",
								triggerType: "relative",
								triggerValue: "-PT15M",
								description: "Reminder",
							},
						],
					}),
				],
				"UTC",
			);
			expect(ics).toContain("BEGIN:VALARM");
			expect(ics).toContain("ACTION:DISPLAY");
			expect(ics).toContain("DESCRIPTION:Reminder");
			expect(ics).toContain("TRIGGER:-PT15M");
			expect(ics).toContain("END:VALARM");
		});

		it("emits X-* extended properties and X-GOOGLE-GUEST-* permissions", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						extendedProperties: { "X-CUSTOM": "custom-value" },
						guestPermissions: {
							canModify: false,
							canInviteOthers: false,
							canSeeOtherGuests: true,
						},
					}),
				],
				"UTC",
			);
			expect(ics).toContain("X-CUSTOM:custom-value");
			expect(ics).toContain("X-GOOGLE-GUEST-CAN-MODIFY:FALSE");
			expect(ics).toContain("X-GOOGLE-GUEST-CAN-INVITE-OTHERS:FALSE");
			expect(ics).toContain("X-GOOGLE-GUEST-CAN-SEE-OTHER-GUESTS:TRUE");
		});
	});

	describe("preservation merge", () => {
		it("keeps unsupported properties and parameters while exporting supported edits", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						title: "Edited title",
						organizer: { name: "Alice", email: "alice-new@example.com" },
						attendees: [
							{
								id: "a1",
								name: "Bob",
								email: "bob@example.com",
								role: "opt-participant",
								status: "accepted",
								rsvp: false,
							},
						],
						alarms: [
							{
								id: "al1",
								action: "display",
								triggerType: "relative",
								triggerValue: "-PT15M",
								description: "Reminder",
							},
						],
						icalendarRawJcal: [
							"vevent",
							[
								["uid", {}, "text", "sample-uid@ganbaru-ai"],
								["dtstamp", {}, "date-time", "20260101T000000Z"],
								["dtstart", {}, "date-time", "20260601T140000Z"],
								["dtend", {}, "date-time", "20260601T150000Z"],
								["summary", { language: "en" }, "text", "Original title"],
								["comment", { language: "en" }, "text", "review note"],
								["resources", {}, "text", "Room A"],
								["related-to", { reltype: "PARENT" }, "text", "parent-uid"],
								["request-status", {}, "text", "2.0;Success"],
								["attach", { fmttype: "application/pdf" }, "uri", "https://example.com/a.pdf"],
								["x-unsupported", { "x-param": "kept" }, "text", "value"],
								[
									"organizer",
									{ cn: "Alice old", "sent-by": "mailto:assistant@example.com", dir: "https://example.com/alice" },
									"cal-address",
									"mailto:alice-old@example.com",
								],
								[
									"attendee",
									{ cn: "Old", role: "REQ-PARTICIPANT", partstat: "NEEDS-ACTION", rsvp: "TRUE", cutype: "INDIVIDUAL" },
									"cal-address",
									"mailto:old@example.com",
								],
							],
							[
								[
									"valarm",
									[
										["action", {}, "text", "DISPLAY"],
										["trigger", {}, "duration", "-PT10M"],
										["repeat", {}, "integer", 3],
										["duration", {}, "duration", "PT5M"],
									],
									[],
								],
							],
						],
					}),
				],
				"UTC",
			);
			const unfolded = unfold(ics);

			expect(unfolded).toContain("SUMMARY;LANGUAGE=en:Edited title");
			expect(unfolded).toContain("COMMENT;LANGUAGE=en:review note");
			expect(unfolded).toContain("RESOURCES:Room A");
			expect(unfolded).toContain("RELATED-TO;RELTYPE=PARENT:parent-uid");
			expect(unfolded).toContain("REQUEST-STATUS:2.0\\;Success");
			expect(unfolded).toContain("ATTACH;FMTTYPE=application/pdf:https://example.com/a.pdf");
			expect(unfolded).toContain("X-UNSUPPORTED;X-PARAM=kept;VALUE=TEXT:value");
			expect(unfolded).toContain("ORGANIZER;CN=Alice");
			expect(unfolded).toContain('SENT-BY="mailto:assistant@example.com"');
			expect(unfolded).toContain('DIR="https://example.com/alice"');
			expect(unfolded).toContain("mailto:alice-new@example.com");
			expect(unfolded).toContain("ATTENDEE;CN=Bob");
			expect(unfolded).toContain("CUTYPE=INDIVIDUAL");
			expect(unfolded).toContain("ROLE=OPT-PARTICIPANT");
			expect(unfolded).toContain("PARTSTAT=ACCEPTED");
			expect(unfolded).toContain("RSVP=FALSE");
			expect(unfolded).toContain("mailto:bob@example.com");
			expect(unfolded).toContain("TRIGGER:-PT15M");
			expect(unfolded).toContain("DESCRIPTION:Reminder");
			expect(unfolded).toContain("REPEAT:3");
			expect(unfolded).toContain("DURATION:PT5M");
		});

		it("does not re-export preserved alarms deleted from the projection", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						alarms: undefined,
						icalendarRawJcal: [
							"vevent",
							[
								["uid", {}, "text", "sample-uid@ganbaru-ai"],
								["dtstamp", {}, "date-time", "20260101T000000Z"],
								["dtstart", {}, "date-time", "20260601T140000Z"],
								["dtend", {}, "date-time", "20260601T150000Z"],
								["summary", {}, "text", "Sample"],
							],
							[
								[
									"valarm",
									[
										["action", {}, "text", "DISPLAY"],
										["trigger", {}, "duration", "-PT10M"],
										["repeat", {}, "integer", 3],
									],
									[],
								],
							],
						],
					}),
				],
				"UTC",
			);

			expect(ics).not.toContain("BEGIN:VALARM");
			expect(ics).not.toContain("REPEAT:3");
		});

		it("does not keep stale generated-owned time parameters", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						timezone: "UTC",
						icalendarRawJcal: [
							"vevent",
							[
								["uid", {}, "text", "sample-uid@ganbaru-ai"],
								["dtstamp", {}, "date-time", "20260101T000000Z"],
								["dtstart", { tzid: "America/New_York" }, "date-time", "20260601T100000"],
								["dtend", { tzid: "America/New_York" }, "date-time", "20260601T110000"],
								["summary", {}, "text", "Sample"],
							],
							[],
						],
					}),
				],
				"UTC",
			);

			expect(ics).toContain("DTSTART:20260601T140000Z");
			expect(ics).not.toContain("DTSTART;TZID=America/New_York");
		});

		it("preserves imported DURATION shape when exporting edited event times", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						start: "2026-06-01 14:00",
						end: "2026-06-01 16:30",
						icalendarRawJcal: [
							"vevent",
							[
								["uid", {}, "text", "sample-uid@ganbaru-ai"],
								["dtstamp", {}, "date-time", "20260101T000000Z"],
								["dtstart", {}, "date-time", "20260601T140000Z"],
								["duration", {}, "duration", "PT1H"],
								["summary", {}, "text", "Sample"],
							],
							[],
						],
					}),
				],
				"UTC",
			);

			expect(ics).toContain("DTSTART:20260601T140000Z");
			expect(ics).toContain("DURATION:PT2H30M");
			expect(ics).not.toContain("DTEND:");
		});

		it("preserves floating date-time shape for linked imports", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						start: "2026-06-01 14:00",
						end: "2026-06-01 15:00",
						icalendarRawJcal: [
							"vevent",
							[
								["uid", {}, "text", "sample-uid@ganbaru-ai"],
								["dtstamp", {}, "date-time", "20260101T000000Z"],
								["dtstart", {}, "date-time", "2026-06-01T14:00:00"],
								["dtend", {}, "date-time", "2026-06-01T15:00:00"],
								["summary", {}, "text", "Sample"],
							],
							[],
						],
					}),
				],
				"UTC",
			);

			expect(ics).toContain("DTSTART:20260601T140000");
			expect(ics).toContain("DTEND:20260601T150000");
			expect(ics).not.toContain("DTSTART:20260601T140000Z");
		});
	});

	describe("preserved non-event components", () => {
		it("passes through preserved top-level components without projection", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[],
				"UTC",
				[],
				[
					[
						"vtodo",
						[
							["uid", {}, "text", "task@example.com"],
							["dtstamp", {}, "date-time", "2026-01-01T00:00:00Z"],
							["summary", { language: "en" }, "text", "Task"],
							["due", {}, "date", "2026-06-01"],
							["x-task-field", { "x-param": "kept" }, "text", "custom"],
						],
						[],
					],
					[
						"vfreebusy",
						[
							["uid", {}, "text", "busy@example.com"],
							["dtstamp", {}, "date-time", "2026-01-01T00:00:00Z"],
							["freebusy", { fbtype: "BUSY" }, "period", ["2026-06-01T10:00:00Z", "2026-06-01T11:00:00Z"]],
						],
						[],
					],
				],
			);
			const unfolded = unfold(ics);

			expect(unfolded).toContain("BEGIN:VTODO");
			expect(unfolded).toContain("SUMMARY;LANGUAGE=en:Task");
			expect(unfolded).toContain("DUE;VALUE=DATE:20260601");
			expect(unfolded).toContain("X-TASK-FIELD;X-PARAM=kept;VALUE=TEXT:custom");
			expect(unfolded).toContain("BEGIN:VFREEBUSY");
			expect(unfolded).toContain("FREEBUSY;FBTYPE=BUSY:20260601T100000Z/20260601T110000Z");
		});
	});

	describe("recurrence and overrides", () => {
		it("emits EXDATE for exceptions", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						recurrence: { frequency: "daily", interval: 1, end: { type: "count", count: 5 } },
						exceptions: ["2026-06-02", "2026-06-04"],
					}),
				],
				"UTC",
			);
			expect(ics).toContain("EXDATE:20260602T140000Z,20260604T140000Z");
		});

		it("emits zoned EXDATE values at the recurring event start time", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						timezone: "America/New_York",
						start: "2026-04-15 09:00",
						end: "2026-04-15 10:00",
						recurrence: { frequency: "daily", interval: 1, end: { type: "count", count: 5 } },
						exceptions: ["2026-04-17"],
					}),
				],
				"America/New_York",
			);
			expect(ics).toContain("EXDATE;TZID=America/New_York:20260417T090000");
		});

		it("emits RDATE", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						rdate: ["2026-07-05T10:00:00.000Z", "2026-07-12T10:00:00.000Z"],
					}),
				],
				"UTC",
			);
			expect(ics).toContain("RDATE:20260705T100000Z,20260712T100000Z");
		});

		it("emits a separate VEVENT per override with RECURRENCE-ID", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						sourceUid: "rec@ex",
						recurrence: { frequency: "daily", interval: 1, end: { type: "count", count: 5 } },
						overrides: [
							{
								id: "ov1",
								parentEventId: "ev-1",
								recurrenceId: "2026-06-03T14:00:00.000Z",
								title: "Adjusted",
							},
						],
					}),
				],
				"UTC",
			);
			const veventCount = (ics.match(/BEGIN:VEVENT/g) ?? []).length;
			expect(veventCount).toBe(2);
			expect(ics).toContain("RECURRENCE-ID:20260603T140000Z");
			expect(ics).toContain("SUMMARY:Adjusted");
		});

		it("emits zoned RECURRENCE-ID values for zoned recurring overrides", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						sourceUid: "rec-zoned@ex",
						timezone: "America/New_York",
						start: "2026-04-15 09:00",
						end: "2026-04-15 10:00",
						recurrence: { frequency: "daily", interval: 1, end: { type: "count", count: 5 } },
						overrides: [
							{
								id: "ov1",
								parentEventId: "ev-1",
								recurrenceId: "2026-04-17T13:00:00Z",
								title: "Adjusted",
							},
						],
					}),
				],
				"America/New_York",
			);
			expect(ics).toContain("RECURRENCE-ID;TZID=America/New_York:20260417T090000");
		});

		it("preserves RANGE=THISANDFUTURE on linked override recurrence ids", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						sourceUid: "range@ex",
						recurrence: { frequency: "daily", interval: 1, end: { type: "count", count: 5 } },
						overrides: [
							{
								id: "ov1",
								parentEventId: "ev-1",
								recurrenceId: "2026-06-03T14:00:00.000Z",
								title: "Adjusted",
								icalendarRawJcal: [
									"vevent",
									[
										["uid", {}, "text", "range@ex"],
										["dtstamp", {}, "date-time", "2026-01-01T00:00:00Z"],
										["dtstart", {}, "date-time", "2026-06-03T16:00:00Z"],
										["dtend", {}, "date-time", "2026-06-03T17:00:00Z"],
										["recurrence-id", { range: "THISANDFUTURE" }, "date-time", "2026-06-03T14:00:00Z"],
										["summary", {}, "text", "Original"],
									],
									[],
								],
							},
						],
					}),
				],
				"UTC",
			);

			expect(ics).toContain("RECURRENCE-ID;RANGE=THISANDFUTURE:20260603T140000Z");
		});

		it("emits RANGE=THISANDFUTURE from projected overrides", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[
					makeEvent({
						sourceUid: "cancel-future@ex",
						recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
						overrides: [
							{
								id: "ov1",
								parentEventId: "ev-1",
								recurrenceId: "2026-06-03T14:00:00Z",
								recurrenceRange: "this-and-future",
								status: "cancelled",
							},
						],
					}),
				],
				"UTC",
			);

			expect(ics).toContain("RECURRENCE-ID;RANGE=THISANDFUTURE:20260603T140000Z");
			expect(ics).toContain("STATUS:CANCELLED");
		});
	});

	describe("UID fallback", () => {
		it("uses event.id as UID when sourceUid is absent", () => {
			const ics = serializeCalendarToIcs(
				baseCalendar,
				[makeEvent({ sourceUid: undefined, id: "internal-id-123" })],
				"UTC",
			);
			expect(ics).toContain("UID:internal-id-123");
		});
	});

	describe("line folding", () => {
		it("folds lines longer than 75 octets", () => {
			const longTitle = "X".repeat(200);
			const ics = serializeCalendarToIcs(baseCalendar, [makeEvent({ title: longTitle })], "UTC");
			const lines = ics.split("\r\n");
			for (const line of lines) {
				if (line.startsWith(" ")) continue;
				expect(line.length).toBeLessThanOrEqual(75);
			}
		});

		it("folds UTF-8 lines without exceeding 75 bytes", () => {
			const longTitle = "Cumpleaños ".repeat(20);
			const ics = serializeCalendarToIcs(baseCalendar, [makeEvent({ title: longTitle })], "UTC");
			const encoder = new TextEncoder();
			for (const line of ics.split("\r\n")) {
				expect(encoder.encode(line).length).toBeLessThanOrEqual(75);
			}
			expect(unfold(ics)).toContain(`SUMMARY:${longTitle}`);
		});
	});
});
