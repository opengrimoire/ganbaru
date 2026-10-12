import { describe, expect, it } from "vitest";
import { parseIcs } from "./parser";

const HEADER = [
	"BEGIN:VCALENDAR",
	"PRODID:-//Ganbaru AI//Test//EN",
	"VERSION:2.0",
].join("\r\n");

const FOOTER = "END:VCALENDAR";

function wrap(...veventLines: string[]): string {
	return [HEADER, ...veventLines, FOOTER].join("\r\n");
}

function vevent(...lines: string[]): string {
	return ["BEGIN:VEVENT", ...lines, "END:VEVENT"].join("\r\n");
}

describe("parseIcs", () => {
	describe("required fields", () => {
		it("parses a minimal VEVENT with UTC times", () => {
			const ics = wrap(
				vevent(
					"UID:simple@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Simple Event",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.warnings).toHaveLength(0);
			const e = result.events[0];
			expect(e.title).toBe("Simple Event");
			expect(e.sourceUid).toBe("simple@example.com");
			expect(e.timezone).toBe("UTC");
		});

		it("returns empty events with a warning when input is malformed", () => {
			const result = parseIcs("not an ics file");
			expect(result.events).toHaveLength(0);
			expect(result.warnings.length).toBeGreaterThan(0);
		});

		it("skips a VEVENT with no UID and warns", () => {
			const ics = wrap(
				vevent(
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:No UID",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(0);
			expect(result.warnings.some((w) => w.includes("UID"))).toBe(true);
		});
	});

	describe("safety limits", () => {
		it("rejects overlong unfolded content lines before parsing", () => {
			const oversizedDescription = "A".repeat(2 * 1024 * 1024 + 1);
			const result = parseIcs(wrap(
				vevent(
					"UID:oversized-line@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					`DESCRIPTION:${oversizedDescription}`,
					"SUMMARY:Oversized",
				),
			));

			expect(result.events).toHaveLength(0);
			expect(result.warnings[0]).toContain("unfolded line limit");
		});

		it("rejects excessive component nesting before parsing", () => {
			const nested = Array.from({ length: 33 }, (_, index) => `BEGIN:X-NEST-${index}`);
			const closing = Array.from({ length: 33 }, (_, index) => `END:X-NEST-${32 - index}`);
			const result = parseIcs([HEADER, ...nested, ...closing, FOOTER].join("\r\n"));

			expect(result.events).toHaveLength(0);
			expect(result.warnings[0]).toContain("component nesting limit");
		});

		it("rejects oversized inline binary attachments before parsing", () => {
			const oversizedBinary = "A".repeat(1024 * 1024 + 1);
			const result = parseIcs(wrap(
				vevent(
					"UID:oversized-attach@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Oversized attachment",
					`ATTACH;ENCODING=BASE64;VALUE=BINARY:${oversizedBinary}`,
				),
			));

			expect(result.events).toHaveLength(0);
			expect(result.warnings[0]).toContain("inline binary attachment limit");
		});
	});

	describe("DTSTART variants", () => {
		it("parses DTSTART;VALUE=DATE as all-day", () => {
			const ics = wrap(
				vevent(
					"UID:allday@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;VALUE=DATE:20260601",
					"DTEND;VALUE=DATE:20260602",
					"SUMMARY:All Day",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].allDay).toBe(true);
			expect(result.events[0].start).toBe("2026-06-01 00:00");
			expect(result.events[0].end).toBe("2026-06-01 00:00");
		});

		it("normalizes Google yearly all-day events to the intended single date", () => {
			const ics = wrap(
				vevent(
					"UID:yearly-allday@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;VALUE=DATE:20260513",
					"DTEND;VALUE=DATE:20260514",
					"RRULE:FREQ=YEARLY",
					"SUMMARY:Yearly all day",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			const event = result.events[0];
			expect(event.allDay).toBe(true);
			expect(event.start).toBe("2026-05-13 00:00");
			expect(event.end).toBe("2026-05-13 00:00");
			expect(event.recurrence?.frequency).toBe("yearly");
		});

		it("normalizes all-day DURATION to an inclusive internal end date", () => {
			const ics = wrap(
				vevent(
					"UID:allday-duration@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;VALUE=DATE:20260601",
					"DURATION:P2D",
					"SUMMARY:Two day all day",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].allDay).toBe(true);
			expect(result.events[0].start).toBe("2026-06-01 00:00");
			expect(result.events[0].end).toBe("2026-06-02 00:00");
		});

		it("computes end from DURATION when DTEND is absent", () => {
			const ics = wrap(
				vevent(
					"UID:duration@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260602T140000Z",
					"DURATION:PT2H30M",
					"SUMMARY:Duration",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			const e = result.events[0];
			expect(e.start).toMatch(/14:00$/);
			expect(e.end).toMatch(/16:30$/);
		});

		it("keeps timed events with no DTEND or DURATION as zero-duration events", () => {
			const ics = wrap(
				vevent(
					"UID:no-end@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"SUMMARY:No End",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.warnings).toHaveLength(0);
			const e = result.events[0];
			expect(e.start).toMatch(/14:00$/);
			expect(e.end).toMatch(/14:00$/);
		});
	});

	describe("timezone handling", () => {
		it("uses TZID as the home zone for IANA names", () => {
			const ics = wrap(
				"BEGIN:VTIMEZONE",
				"TZID:America/New_York",
				"BEGIN:STANDARD",
				"DTSTART:19700101T000000",
				"TZOFFSETFROM:-0500",
				"TZOFFSETTO:-0500",
				"END:STANDARD",
				"END:VTIMEZONE",
				vevent(
					"UID:zoned@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;TZID=America/New_York:20260415T090000",
					"DTEND;TZID=America/New_York:20260415T100000",
					"SUMMARY:NY Event",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].timezone).toBe("America/New_York");
		});

		it("maps Outlook Windows zone names to IANA", () => {
			const ics = wrap(
				"BEGIN:VTIMEZONE",
				"TZID:Pacific Standard Time",
				"BEGIN:STANDARD",
				"DTSTART:19700101T000000",
				"TZOFFSETFROM:-0700",
				"TZOFFSETTO:-0800",
				"END:STANDARD",
				"END:VTIMEZONE",
				vevent(
					"UID:outlook@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;TZID=Pacific Standard Time:20260501T090000",
					"DTEND;TZID=Pacific Standard Time:20260501T100000",
					"SUMMARY:Outlook Event",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].timezone).toBe("America/Los_Angeles");
		});

		it("falls back to UTC and warns for unknown TZID", () => {
			const ics = wrap(
				"BEGIN:VTIMEZONE",
				"TZID:Bogus/Made-Up",
				"BEGIN:STANDARD",
				"DTSTART:19700101T000000",
				"TZOFFSETFROM:+0000",
				"TZOFFSETTO:+0000",
				"END:STANDARD",
				"END:VTIMEZONE",
				vevent(
					"UID:bogus@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;TZID=Bogus/Made-Up:20260801T100000",
					"DTEND;TZID=Bogus/Made-Up:20260801T110000",
					"SUMMARY:Bogus Zone",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].timezone).toBe("UTC");
			expect(result.warnings.some((w) => w.includes("Bogus/Made-Up"))).toBe(true);
		});
	});

	describe("recurrence", () => {
		it("preserves the UTC termination of Google UNTIL-capped recurring events", () => {
			const ics = wrap(
				vevent(
					"DTSTART;TZID=America/Mexico_City:20210208T060000",
					"DTEND;TZID=America/Mexico_City:20210208T063000",
					"RRULE:FREQ=DAILY;UNTIL=20210322T055959Z",
					"DTSTAMP:20260514T213541Z",
					"UID:3u1hfmnhrngcurk04rq0lnn37p@google.com",
					"CREATED:20210129T054436Z",
					"LAST-MODIFIED:20260505T031146Z",
					"SEQUENCE:1",
					"STATUS:CONFIRMED",
					"SUMMARY:Revisar correo\\, mensajes y noticias",
					"TRANSP:OPAQUE",
				),
				vevent(
					"DTSTART;TZID=America/Mexico_City:20210511T090000",
					"DTEND;TZID=America/Mexico_City:20210511T130000",
					"RRULE:FREQ=WEEKLY;WKST=MO;UNTIL=20210612T045959Z;BYDAY=FR,TU,WE",
					"DTSTAMP:20260514T213541Z",
					"UID:5uk9s8pts7uh8jmubpde7irjqs@google.com",
					"CREATED:20210116T024954Z",
					"LAST-MODIFIED:20260505T031146Z",
					"SEQUENCE:0",
					"STATUS:CONFIRMED",
					"SUMMARY:Dormir",
					"TRANSP:OPAQUE",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(2);
			expect(result.events[0].recurrence?.end).toEqual({
				type: "until",
				date: "2021-03-22T05:59:59Z",
			});
			expect(result.events[1].recurrence?.end).toEqual({
				type: "until",
				date: "2021-06-12T04:59:59Z",
			});

		});

		it("uses the newest duplicate master VEVENT revision for import", () => {
			const ics = wrap(
				vevent(
					"DTSTART;TZID=America/Mexico_City:20210208T060000",
					"DTEND;TZID=America/Mexico_City:20210208T063000",
					"RRULE:FREQ=DAILY",
					"DTSTAMP:20210129T054436Z",
					"UID:duplicate-revision@google.com",
					"LAST-MODIFIED:20210129T054436Z",
					"SEQUENCE:0",
					"STATUS:CONFIRMED",
					"SUMMARY:Uncapped old revision",
				),
				vevent(
					"DTSTART;TZID=America/Mexico_City:20210208T060000",
					"DTEND;TZID=America/Mexico_City:20210208T063000",
					"RRULE:FREQ=DAILY;UNTIL=20210322T055959Z",
					"DTSTAMP:20260514T213541Z",
					"UID:duplicate-revision@google.com",
					"LAST-MODIFIED:20260505T031146Z",
					"SEQUENCE:1",
					"STATUS:CONFIRMED",
					"SUMMARY:Capped newest revision",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].title).toBe("Capped newest revision");
			expect(result.events[0].recurrence?.end).toEqual({
				type: "until",
				date: "2021-03-22T05:59:59Z",
			});

		});

		it("parses RRULE into RecurrenceConfig", () => {
			const ics = wrap(
				vevent(
					"UID:daily@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"RRULE:FREQ=DAILY;COUNT=10",
					"SUMMARY:Daily Event",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			const rec = result.events[0].recurrence;
			expect(rec?.frequency).toBe("daily");
			expect(rec?.end).toEqual({ type: "count", count: 10 });
		});

		it("parses EXDATE as YYYY-MM-DD strings", () => {
			const ics = wrap(
				vevent(
					"UID:exdate@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"RRULE:FREQ=DAILY;COUNT=5",
					"EXDATE:20260602T140000Z",
					"SUMMARY:With EXDATE",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].exceptions).toEqual(["2026-06-02"]);
		});

		it("parses TZID EXDATE values as local recurrence dates", () => {
			const ics = wrap(
				vevent(
					"UID:exdate-zoned@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;TZID=Asia/Tokyo:20260601T003000",
					"DTEND;TZID=Asia/Tokyo:20260601T013000",
					"RRULE:FREQ=DAILY;COUNT=5",
					"EXDATE;TZID=Asia/Tokyo:20260602T003000",
					"SUMMARY:With zoned EXDATE",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].exceptions).toEqual(["2026-06-02"]);
		});

		it("parses UTC EXDATE values in the event home zone", () => {
			const ics = wrap(
				vevent(
					"UID:exdate-utc-zone@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;TZID=America/Los_Angeles:20260501T230000",
					"DTEND;TZID=America/Los_Angeles:20260502T000000",
					"RRULE:FREQ=DAILY;COUNT=5",
					"EXDATE:20260502T060000Z",
					"SUMMARY:With UTC EXDATE",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].exceptions).toEqual(["2026-05-01"]);
		});

		it("parses RDATE as a list of UTC ISO 8601 instants", () => {
			const ics = wrap(
				vevent(
					"UID:rdate@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260701T100000Z",
					"DTEND:20260701T110000Z",
					"RDATE:20260705T100000Z,20260712T100000Z",
					"SUMMARY:With RDATE",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].rdate).toEqual([
				"2026-07-05T10:00:00Z",
				"2026-07-12T10:00:00Z",
			]);
		});

		it("attaches RECURRENCE-ID overrides to the matching master", () => {
			const ics = wrap(
				vevent(
					"UID:recur@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"RRULE:FREQ=DAILY;COUNT=5",
					"SUMMARY:Master",
				),
				vevent(
					"UID:recur@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260603T160000Z",
					"DTEND:20260603T170000Z",
					"RECURRENCE-ID:20260603T140000Z",
					"SUMMARY:Override",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			const overrides = result.events[0].overrides;
			expect(overrides).toHaveLength(1);
			expect(overrides![0].title).toBe("Override");
			expect(overrides![0].recurrenceId).toBe("2026-06-03T14:00:00Z");
		});

		it("marks cancelled single recurrence components as cancelled overrides", () => {
			const ics = wrap(
				vevent(
					"UID:cancel-one@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"RRULE:FREQ=DAILY;COUNT=5",
					"SUMMARY:Master",
				),
				vevent(
					"UID:cancel-one@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260603T140000Z",
					"DTEND:20260603T150000Z",
					"RECURRENCE-ID:20260603T140000Z",
					"STATUS:CANCELLED",
					"SUMMARY:Cancelled",
				),
			);

			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].overrides?.[0]).toMatchObject({
				recurrenceId: "2026-06-03T14:00:00Z",
				status: "cancelled",
			});
		});

		it("preserves cancelled this-and-future recurrence components without DTSTART", () => {
			const ics = wrap(
				vevent(
					"UID:cancel-future@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20220601T140000Z",
					"DTEND:20220601T150000Z",
					"RRULE:FREQ=DAILY",
					"SUMMARY:Master",
				),
				vevent(
					"UID:cancel-future@example.com",
					"DTSTAMP:20260101T000000Z",
					"RECURRENCE-ID;RANGE=THISANDFUTURE:20220603T140000Z",
					"STATUS:CANCELLED",
					"SUMMARY:Cancelled future",
				),
			);

			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].overrides?.[0]).toMatchObject({
				recurrenceId: "2022-06-03T14:00:00Z",
				recurrenceRange: "this-and-future",
				status: "cancelled",
			});
		});

		it("warns when an override has no matching master", () => {
			const ics = wrap(
				vevent(
					"UID:orphan@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260603T160000Z",
					"DTEND:20260603T170000Z",
					"RECURRENCE-ID:20260603T140000Z",
					"SUMMARY:Orphan Override",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(0);
			expect(result.warnings.some((w) => w.includes("orphan@example.com"))).toBe(true);
		});
	});

	describe("attendees and organizer", () => {
		it("parses ATTENDEE properties with role and PARTSTAT", () => {
			const ics = wrap(
				vevent(
					"UID:att@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Meeting",
					"ATTENDEE;CN=Bob;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;RSVP=TRUE:mailto:bob@example.com",
					"ATTENDEE;CN=Carol;ROLE=OPT-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=FALSE:mailto:carol@example.com",
				),
			);
			const result = parseIcs(ics);
			const attendees = result.events[0].attendees;
			expect(attendees).toHaveLength(2);
			expect(attendees![0]).toMatchObject({
				name: "Bob",
				email: "bob@example.com",
				role: "req-participant",
				status: "accepted",
				rsvp: true,
			});
			expect(attendees![1]).toMatchObject({
				name: "Carol",
				email: "carol@example.com",
				role: "opt-participant",
				status: "needs-action",
				rsvp: false,
			});
		});

		it("parses ORGANIZER with CN and mailto", () => {
			const ics = wrap(
				vevent(
					"UID:org@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Meeting",
					"ORGANIZER;CN=Alice:mailto:alice@example.com",
				),
			);
			const result = parseIcs(ics);
			expect(result.events[0].organizer).toEqual({ name: "Alice", email: "alice@example.com" });
		});

		it("parses quoted and caret-escaped CN parameter values", () => {
			const ics = wrap(
				vevent(
					"UID:param-cn@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Meeting",
					"ORGANIZER;CN=\"Doe, Jane ^'JJ^'\":mailto:jane@example.com",
					"ATTENDEE;CN=\"Team; lead^nremote\";ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:lead@example.com",
				),
			);
			const result = parseIcs(ics);
			expect(result.events[0].organizer).toEqual({
				name: 'Doe, Jane "JJ"',
				email: "jane@example.com",
			});
			expect(result.events[0].attendees?.[0]).toMatchObject({
				name: "Team; lead\nremote",
				email: "lead@example.com",
			});
		});
	});

	describe("alarms", () => {
		it("parses VALARM into EventAlarm", () => {
			const ics = wrap(
				vevent(
					"UID:alarm@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:With alarm",
					"BEGIN:VALARM",
					"ACTION:DISPLAY",
					"DESCRIPTION:Reminder",
					"TRIGGER:-PT15M",
					"END:VALARM",
				),
			);
			const result = parseIcs(ics);
			const alarms = result.events[0].alarms;
			expect(alarms).toHaveLength(1);
			expect(alarms![0]).toMatchObject({
				action: "display",
				triggerType: "relative",
				description: "Reminder",
			});
			expect(alarms![0].triggerValue).toContain("PT15M");
		});

		it("warns when VALARM REPEAT/DURATION are present and drops them", () => {
			const ics = wrap(
				vevent(
					"UID:alarm-rep@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Repeating alarm",
					"BEGIN:VALARM",
					"ACTION:DISPLAY",
					"TRIGGER:-PT10M",
					"REPEAT:3",
					"DURATION:PT5M",
					"END:VALARM",
				),
			);
			const result = parseIcs(ics);
			expect(result.warnings.some((w) => w.includes("REPEAT") && w.includes("DURATION"))).toBe(true);
			expect(result.events[0].alarms).toHaveLength(1);
		});
	});

	describe("preservation diagnostics", () => {
		it("records component projection warnings for unsupported properties and parameters", () => {
			const ics = wrap(
				vevent(
					"UID:diag@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY;LANGUAGE=en:Diagnostics",
					"CONTACT:desk@example.com",
					"BEGIN:VALARM",
					"ACTION:DISPLAY",
					"TRIGGER:-PT15M",
					"REPEAT:2",
					"DURATION:PT5M",
					"END:VALARM",
				),
			);
			const result = parseIcs(ics);
			const eventComponent = result.preservation?.objects[0].components
				.find((component) => component.componentType === "vevent");
			const alarmComponent = eventComponent?.components
				.find((component) => component.componentType === "valarm");

			expect(eventComponent?.projectionWarnings).toEqual(
				expect.arrayContaining([
					"VEVENT property SUMMARY parameter LANGUAGE is preserved but not projected.",
					"VEVENT property CONTACT is preserved but not projected.",
				]),
			);
			expect(alarmComponent?.projectionWarnings).toEqual(
				expect.arrayContaining([
					"VALARM property REPEAT is preserved but not projected.",
					"VALARM property DURATION is preserved but not projected.",
				]),
			);
		});
	});

	describe("status, transparency, visibility, priority, sequence", () => {
		it("maps STATUS, TRANSP, CLASS, PRIORITY, SEQUENCE", () => {
			const ics = wrap(
				vevent(
					"UID:meta@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Meta",
					"STATUS:TENTATIVE",
					"TRANSP:TRANSPARENT",
					"CLASS:PRIVATE",
					"PRIORITY:3",
					"SEQUENCE:7",
				),
			);
			const e = parseIcs(ics).events[0];
			expect(e.status).toBe("tentative");
			expect(e.transparency).toBe("transparent");
			expect(e.visibility).toBe("private");
			expect(e.priority).toBe(3);
			expect(e.sequence).toBe(7);
		});

		it("treats CLASS:CONFIDENTIAL as private", () => {
			const ics = wrap(
				vevent(
					"UID:confidential@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Confidential",
					"CLASS:CONFIDENTIAL",
				),
			);
			const e = parseIcs(ics).events[0];
			expect(e.visibility).toBe("private");
		});
	});

	describe("categories, geo, X-properties", () => {
		it("parses CATEGORIES into a string array", () => {
			const ics = wrap(
				vevent(
					"UID:cat@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Tagged",
					"CATEGORIES:Work,Team,Important",
				),
			);
			expect(parseIcs(ics).events[0].categories).toEqual(["Work", "Team", "Important"]);
		});

		it("parses GEO into {lat, lng}", () => {
			const ics = wrap(
				vevent(
					"UID:geo@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Located",
					"GEO:40.7128;-74.0060",
				),
			);
			const e = parseIcs(ics).events[0];
			expect(e.geo).toEqual({ lat: 40.7128, lng: -74.006 });
		});

		it("collects unrecognized X-* properties into extendedProperties", () => {
			const ics = wrap(
				vevent(
					"UID:xprop@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Custom",
					"X-CUSTOM-FIELD:custom-value",
					"X-ANOTHER:another-value",
				),
			);
			const e = parseIcs(ics).events[0];
			expect(e.extendedProperties).toMatchObject({
				"X-CUSTOM-FIELD": "custom-value",
				"X-ANOTHER": "another-value",
			});
		});

		it("recognizes X-GOOGLE-GUEST-* permissions", () => {
			const ics = wrap(
				vevent(
					"UID:guests@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Restricted",
					"X-GOOGLE-GUEST-CAN-MODIFY:FALSE",
					"X-GOOGLE-GUEST-CAN-INVITE-OTHERS:FALSE",
					"X-GOOGLE-GUEST-CAN-SEE-OTHER-GUESTS:TRUE",
				),
			);
			const e = parseIcs(ics).events[0];
			expect(e.guestPermissions).toEqual({
				canModify: false,
				canInviteOthers: false,
				canSeeOtherGuests: true,
			});
		});

		it("promotes Google Meet conference links out of generated descriptions", () => {
			const meetUrl = "https://meet.google.com/abc-defg-hij";
			const generatedBlock = [
				"-::~:~::~:~:~:~:~:~:~:~:~:~:~:~:~:~::~:-",
				`Join with Google Meet: ${meetUrl}`,
				"Or dial: (US) +1 555-0100 PIN: 123 456 789#",
				"More phone numbers: https://tel.meet/abc-defg-hij",
				"-::~:~::~:~:~:~:~:~:~:~:~:~:~:~:~:~::~:-",
			].join("\\n");
			const ics = wrap(
				vevent(
					"UID:meet@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Meeting",
					`DESCRIPTION:Agenda\\n${generatedBlock}`,
					`X-GOOGLE-CONFERENCE:${meetUrl}`,
				),
			);

			const event = parseIcs(ics).events[0];
			expect(event.url).toBe(meetUrl);
			expect(event.description).toBe("Agenda");
			expect(event.extendedProperties).toMatchObject({
				"X-GOOGLE-CONFERENCE": meetUrl,
			});
		});

		it("drops Google Meet generated descriptions when there is no user description", () => {
			const meetUrl = "https://meet.google.com/abc-defg-hij";
			const ics = wrap(
				vevent(
					"UID:meet-empty-description@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Meeting",
					"DESCRIPTION:-::~:~::~:~:~:~:~:~:~:~::~:-\\nJoin with Google Meet: https://meet.google.com/abc-defg-hij\\n-::~:~::~:~:~:~:~:~:~:~::~:-",
					`X-GOOGLE-CONFERENCE:${meetUrl}`,
				),
			);

			const event = parseIcs(ics).events[0];
			expect(event.url).toBe(meetUrl);
			expect(event.description).toBeUndefined();
		});
	});

	describe("multiple events", () => {
		it("parses multiple VEVENTs in one VCALENDAR", () => {
			const ics = wrap(
				vevent(
					"UID:one@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:One",
				),
				vevent(
					"UID:two@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260602T140000Z",
					"DTEND:20260602T150000Z",
					"SUMMARY:Two",
				),
			);
			const events = parseIcs(ics).events;
			expect(events).toHaveLength(2);
			expect(events.map((e) => e.title)).toEqual(["One", "Two"]);
		});

		it("warns and keeps the first when two masters share a UID", () => {
			const ics = wrap(
				vevent(
					"UID:dup@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:First",
				),
				vevent(
					"UID:dup@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260602T140000Z",
					"DTEND:20260602T150000Z",
					"SUMMARY:Second",
				),
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(1);
			expect(result.events[0].title).toBe("First");
			expect(result.warnings.some((w) => w.includes("Duplicate UID"))).toBe(true);
		});
	});

	describe("preservation payload", () => {
		it("preserves non-event components even when nothing is projected", () => {
			const ics = wrap(
				"BEGIN:VTODO",
				"UID:task@example.com",
				"DTSTAMP:20260101T000000Z",
				"SUMMARY:Task",
				"X-CUSTOM-TODO:kept",
				"END:VTODO",
				"BEGIN:VJOURNAL",
				"UID:journal@example.com",
				"DTSTAMP:20260101T000000Z",
				"DTSTART;VALUE=DATE:20260601",
				"SUMMARY:Journal",
				"END:VJOURNAL",
				"BEGIN:VFREEBUSY",
				"UID:freebusy@example.com",
				"DTSTAMP:20260101T000000Z",
				"FREEBUSY:20260601T120000Z/20260601T130000Z",
				"END:VFREEBUSY",
			);
			const result = parseIcs(ics);
			expect(result.events).toHaveLength(0);
			const components = result.preservation?.objects[0].components;
			expect(components?.map((component) => component.componentType)).toEqual([
				"vtodo",
				"vjournal",
				"vfreebusy",
			]);
			expect(components?.[0].uid).toBe("task@example.com");
			expect(JSON.stringify(components?.[0].rawJcal)).toContain("x-custom-todo");
		});

		it("preserves nested timezone and alarm components", () => {
			const ics = wrap(
				"BEGIN:VTIMEZONE",
				"TZID:America/New_York",
				"BEGIN:DAYLIGHT",
				"DTSTART:19700308T020000",
				"TZOFFSETFROM:-0500",
				"TZOFFSETTO:-0400",
				"END:DAYLIGHT",
				"END:VTIMEZONE",
				vevent(
					"UID:nested@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART;TZID=America/New_York:20260601T090000",
					"DTEND;TZID=America/New_York:20260601T100000",
					"SUMMARY:Nested",
					"BEGIN:VALARM",
					"ACTION:DISPLAY",
					"TRIGGER:-PT10M",
					"END:VALARM",
				),
			);
			const result = parseIcs(ics);
			const components = result.preservation?.objects[0].components ?? [];
			const timezone = components.find((component) => component.componentType === "vtimezone");
			const event = components.find((component) => component.componentType === "vevent");
			expect(timezone?.components[0].componentType).toBe("daylight");
			expect(event?.components[0].componentType).toBe("valarm");
		});
	});

	describe("calendarId argument", () => {
		it("uses the provided calendarId on every event", () => {
			const ics = wrap(
				vevent(
					"UID:cid@example.com",
					"DTSTAMP:20260101T000000Z",
					"DTSTART:20260601T140000Z",
					"DTEND:20260601T150000Z",
					"SUMMARY:Test",
				),
			);
			const result = parseIcs(ics, "imported-calendar-1");
			expect(result.events[0].calendarId).toBe("imported-calendar-1");
		});
	});
});
