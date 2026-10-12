import { describe, expect, it } from "vitest";
import type { Calendar, CalendarEvent } from "$lib/calendar/types";
import { parseIcs } from "./parser";
import { serializeCalendarToIcs } from "./serializer";
import googleSample from "../../../../test-fixtures/ics/google-calendar-sample.ics?raw";
import outlookSample from "../../../../test-fixtures/ics/outlook-sample.ics?raw";
import edgeCases from "../../../../test-fixtures/ics/edge-cases.ics?raw";

const FIXTURES: Record<string, string> = {
	"google-calendar-sample.ics": googleSample,
	"outlook-sample.ics": outlookSample,
	"edge-cases.ics": edgeCases,
};

function loadFixture(name: string): string {
	const text = FIXTURES[name];
	if (!text) throw new Error(`Unknown fixture: ${name}`);
	return text;
}

const TEST_CALENDAR: Calendar = {
	id: "rt-cal",
	name: "Round Trip",
	color: "",
	source: "ics",
	visible: true,
	readOnly: false,
};

/**
 * Strip volatile fields (random UUIDs the parser emits for ids) before
 * comparing so deep-equal can match the structurally meaningful payload.
 */
function normalize(event: CalendarEvent): Omit<CalendarEvent, "id"> {
	const { id: _id, ...rest } = event;
	if (rest.attendees) {
		rest.attendees = rest.attendees.map(({ id: _aid, ...att }) => ({
			id: "",
			...att,
		}));
	}
	if (rest.alarms) {
		rest.alarms = rest.alarms.map(({ id: _alid, ...alm }) => ({
			id: "",
			...alm,
		}));
	}
	if (rest.overrides) {
		rest.overrides = rest.overrides.map(({ id: _oid, parentEventId: _pid, ...ovr }) => ({
			id: "",
			parentEventId: "",
			...ovr,
		}));
	}
	return rest;
}

function indexByUid(events: CalendarEvent[]): Map<string, CalendarEvent> {
	const map = new Map<string, CalendarEvent>();
	for (const e of events) {
		if (e.sourceUid) map.set(e.sourceUid, e);
	}
	return map;
}

describe("ics round-trip", () => {
	it("preserves Google Calendar fixture through parse → serialize → parse", () => {
		const original = loadFixture("google-calendar-sample.ics");
		const first = parseIcs(original, TEST_CALENDAR.id);
		const reSerialized = serializeCalendarToIcs(TEST_CALENDAR, first.events);
		const second = parseIcs(reSerialized, TEST_CALENDAR.id);

		expect(second.events).toHaveLength(first.events.length);
		const a = indexByUid(first.events);
		const b = indexByUid(second.events);
		expect([...b.keys()].sort()).toEqual([...a.keys()].sort());

		for (const [uid, firstEvent] of a) {
			const secondEvent = b.get(uid);
			expect(secondEvent, `missing UID ${uid} after round-trip`).toBeDefined();
			expect(normalize(secondEvent!)).toEqual(normalize(firstEvent));
		}
	});

	it("preserves Outlook fixture through parse → serialize → parse", () => {
		const original = loadFixture("outlook-sample.ics");
		const first = parseIcs(original, TEST_CALENDAR.id);
		const reSerialized = serializeCalendarToIcs(TEST_CALENDAR, first.events);
		const second = parseIcs(reSerialized, TEST_CALENDAR.id);

		expect(second.events.length).toBe(first.events.length);
		const a = indexByUid(first.events);
		const b = indexByUid(second.events);
		for (const [uid, firstEvent] of a) {
			const secondEvent = b.get(uid);
			expect(secondEvent, `missing UID ${uid} after round-trip`).toBeDefined();
			expect(normalize(secondEvent!)).toEqual(normalize(firstEvent));
		}
	});

	it("preserves edge-cases fixture (with at least one warning) through round-trip", () => {
		const original = loadFixture("edge-cases.ics");
		const first = parseIcs(original, TEST_CALENDAR.id);
		expect(first.warnings.length).toBeGreaterThan(0);
		const reSerialized = serializeCalendarToIcs(TEST_CALENDAR, first.events);
		const second = parseIcs(reSerialized, TEST_CALENDAR.id);

		expect(second.events.length).toBe(first.events.length);
		const a = indexByUid(first.events);
		const b = indexByUid(second.events);
		for (const [uid, firstEvent] of a) {
			const secondEvent = b.get(uid);
			expect(secondEvent, `missing UID ${uid} after round-trip`).toBeDefined();
			expect(normalize(secondEvent!)).toEqual(normalize(firstEvent));
		}
	});

	it("emits the standard VCALENDAR envelope on every export", () => {
		for (const name of ["google-calendar-sample.ics", "outlook-sample.ics", "edge-cases.ics"]) {
			const ics = loadFixture(name);
			const parsed = parseIcs(ics, TEST_CALENDAR.id);
			const out = serializeCalendarToIcs(TEST_CALENDAR, parsed.events);
			expect(out.startsWith("BEGIN:VCALENDAR\r\n"), name).toBe(true);
			expect(out).toContain("PRODID:-//Ganbaru AI//Calendar//EN");
			expect(out).toContain("VERSION:2.0");
			expect(out.trimEnd().endsWith("END:VCALENDAR")).toBe(true);
		}
	});
});
