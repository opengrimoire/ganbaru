import { describe, expect, it } from "vitest";
import type { Calendar, CalendarEvent, EventOverride } from "$lib/calendar/types";
import type { IcsParseResult, IcsPreservedComponent } from "./types";
import { parseIcs } from "./parser";
import { serializeCalendarToIcs } from "./serializer";
import coreEventsFixture from "../../../../test-fixtures/ics/rfc5545/core-events.ics?raw";
import recurrenceTimezonesFixture from "../../../../test-fixtures/ics/rfc5545/recurrence-timezones.ics?raw";
import schedulingFixture from "../../../../test-fixtures/ics/rfc5545/scheduling.ics?raw";
import componentsFixture from "../../../../test-fixtures/ics/rfc5545/components.ics?raw";
import attachmentsExtensionsFixture from "../../../../test-fixtures/ics/rfc5545/attachments-extensions.ics?raw";

const FIXTURE_CALENDAR: Calendar = {
	id: "fixture-calendar",
	name: "RFC 5545 fixtures",
	color: "",
	source: "ics",
	visible: true,
	readOnly: false,
};

const FIXTURES: Record<string, string> = {
	"core-events.ics": coreEventsFixture,
	"recurrence-timezones.ics": recurrenceTimezonesFixture,
	"scheduling.ics": schedulingFixture,
	"components.ics": componentsFixture,
	"attachments-extensions.ics": attachmentsExtensionsFixture,
};

function unfold(ics: string): string {
	return ics.replace(/\r\n[ \t]/g, "");
}

function topLevelComponents(parsed: IcsParseResult): IcsPreservedComponent[] {
	return parsed.preservation?.objects.flatMap((object) => object.components) ?? [];
}

function topLevelRawComponents(parsed: IcsParseResult, type: string): unknown[] {
	return topLevelComponents(parsed)
		.filter((component) => component.componentType === type)
		.map((component) => component.rawJcal);
}

function topLevelPassthroughRawComponents(parsed: IcsParseResult): unknown[] {
	return topLevelComponents(parsed)
		.filter((component) => component.componentType !== "vevent" && component.componentType !== "vtimezone")
		.map((component) => component.rawJcal);
}

function distinctPreservedMethod(parsed: IcsParseResult): string | undefined {
	const methods = new Set(
		(parsed.preservation?.objects ?? [])
			.map((object) => object.method?.trim().toUpperCase())
			.filter((method): method is string => !!method),
	);
	return methods.size === 1 ? [...methods][0] : undefined;
}

function veventComponentsByUid(parsed: IcsParseResult): Map<string, IcsPreservedComponent[]> {
	const byUid = new Map<string, IcsPreservedComponent[]>();
	for (const component of topLevelComponents(parsed)) {
		if (component.componentType !== "vevent" || !component.uid) continue;
		const list = byUid.get(component.uid) ?? [];
		list.push(component);
		byUid.set(component.uid, list);
	}
	return byUid;
}

function withPreservedVeventRaw(parsed: IcsParseResult): CalendarEvent[] {
	const componentsByUid = veventComponentsByUid(parsed);
	return parsed.events.map((event) => {
		const uidComponents = event.sourceUid ? componentsByUid.get(event.sourceUid) ?? [] : [];
		const master = uidComponents.find((component) => !component.recurrenceId);
		const overrideComponents = uidComponents.filter((component) => !!component.recurrenceId);
		return {
			...event,
			icalendarRawJcal: master?.rawJcal ?? event.icalendarRawJcal,
			overrides: event.overrides?.map((override, index): EventOverride => ({
				...override,
				icalendarRawJcal: overrideComponents[index]?.rawJcal ?? override.icalendarRawJcal,
			})),
		};
	});
}

function normalizeEvent(event: CalendarEvent): CalendarEvent {
	const normalized: CalendarEvent = { ...event, id: "" };
	if (!normalized.recurrence && !normalized.allDay) {
		normalized.timezone = "";
	}
	delete normalized.icalendarComponentId;
	delete normalized.icalendarPreservationStatus;
	delete normalized.icalendarProjectionWarnings;
	delete normalized.icalendarRawJcal;
	if (event.attendees) {
		normalized.attendees = event.attendees.map((attendee) => {
			const normalizedAttendee = { ...attendee, id: "" };
			delete normalizedAttendee.icalendarComponentId;
			delete normalizedAttendee.icalendarPropertyIndex;
			return normalizedAttendee;
		});
	}
	if (event.alarms) {
		normalized.alarms = event.alarms.map((alarm) => {
			const normalizedAlarm = { ...alarm, id: "" };
			delete normalizedAlarm.icalendarComponentId;
			return normalizedAlarm;
		});
	}
	if (event.overrides) {
		normalized.overrides = event.overrides.map((override) => {
			const normalizedOverride = { ...override, id: "", parentEventId: "" };
			delete normalizedOverride.icalendarComponentId;
			delete normalizedOverride.icalendarRawJcal;
			return normalizedOverride;
		});
	}
	return normalized;
}

function eventsByUid(events: CalendarEvent[]): Map<string, CalendarEvent> {
	const byUid = new Map<string, CalendarEvent>();
	for (const event of events) {
		if (event.sourceUid) byUid.set(event.sourceUid, event);
	}
	return byUid;
}

function serializeParsedWithPreservation(parsed: IcsParseResult): string {
	return serializeCalendarToIcs(
		FIXTURE_CALENDAR,
		withPreservedVeventRaw(parsed),
		undefined,
		topLevelRawComponents(parsed, "vtimezone"),
		topLevelPassthroughRawComponents(parsed),
		distinctPreservedMethod(parsed),
	);
}

function expectProjectedEventsRoundTrip(name: string, source: string): void {
	const first = parseIcs(source, FIXTURE_CALENDAR.id);
	const exported = serializeParsedWithPreservation(first);
	const second = parseIcs(exported, FIXTURE_CALENDAR.id);
	const firstByUid = eventsByUid(first.events);
	const secondByUid = eventsByUid(second.events);

	expect(secondByUid.size, name).toBe(firstByUid.size);
	for (const [uid, firstEvent] of firstByUid) {
		const secondEvent = secondByUid.get(uid);
		expect(secondEvent, `${name} missing UID ${uid}`).toBeDefined();
		expect(normalizeEvent(secondEvent!), `${name} UID ${uid}`).toEqual(normalizeEvent(firstEvent));
	}
}

describe("RFC 5545 fixture suite", () => {
	for (const [name, source] of Object.entries(FIXTURES)) {
		it(`round-trips projected event semantics for ${name}`, () => {
			expectProjectedEventsRoundTrip(name, source);
		});
	}

	it("preserves scheduling metadata while keeping RSVP projection read-only", () => {
		const parsed = parseIcs(schedulingFixture, FIXTURE_CALENDAR.id);
		const exported = unfold(serializeParsedWithPreservation(parsed));

		expect(exported).toContain("METHOD:REQUEST");
		expect(exported).toContain("REQUEST-STATUS:2.0;Success");
		expect(exported).toContain('SENT-BY="mailto:assistant@example.com"');
		expect(exported).toContain('DIR="https://example.com/alice"');
		expect(exported).toContain('DELEGATED-FROM="mailto:manager@example.com"');
		expect(exported).toContain('DELEGATED-TO="mailto:delegate@example.com"');
		expect(exported).toContain('MEMBER="mailto:team@example.com"');
		expect(parsed.events[0].attendees?.map((attendee) => attendee.status)).toEqual([
			"accepted",
			"tentative",
		]);
	});

	it("preserves RANGE, custom VTIMEZONE data, and recurrence exceptions", () => {
		const parsed = parseIcs(recurrenceTimezonesFixture, FIXTURE_CALENDAR.id);
		const exported = unfold(serializeParsedWithPreservation(parsed));

		expect(exported).toContain("BEGIN:VTIMEZONE");
		expect(exported).toContain("TZID:America/New_York");
		expect(exported).toContain("RANGE=THISANDFUTURE");
		expect(exported).toContain("EXDATE;TZID=America/New_York:20260309T090000");
		expect(exported).toContain("RDATE:20260320T130000Z");
	});

	it("passes through preserved non-event top-level components", () => {
		const parsed = parseIcs(componentsFixture, FIXTURE_CALENDAR.id);
		const exported = unfold(serializeParsedWithPreservation(parsed));

		expect(exported).toContain("BEGIN:VTODO");
		expect(exported).toContain("PERCENT-COMPLETE:40");
		expect(exported).toContain("BEGIN:VJOURNAL");
		expect(exported).toContain("BEGIN:VFREEBUSY");
		expect(exported).toContain("FREEBUSY;FBTYPE=BUSY:20260601T100000Z/20260601T110000Z,20260601T140000Z/PT30M");
	});

	it("keeps registered unsupported properties preservation-only", () => {
		const parsed = parseIcs(attachmentsExtensionsFixture, FIXTURE_CALENDAR.id);
		const event = parsed.events[0];
		const exported = unfold(serializeParsedWithPreservation(parsed));

		expect(event.extendedProperties).toEqual({
			"X-COMPONENT-FIELD": "beta",
		});
		expect(exported).toContain("ATTACH;FMTTYPE=application/pdf:https://example.com/brief.pdf");
		expect(exported).toContain("ATTACH;ENCODING=BASE64;FMTTYPE=text/plain;VALUE=BINARY:SGVsbG8=");
		expect(exported).toContain("IMAGE;FMTTYPE=image/png;VALUE=URI:https://example.com/image.png");
		expect(exported).toContain('CONFERENCE;FEATURE="AUDIO,VIDEO";LABEL=Call;VALUE=URI:https://meet.example.com/fixture');
		expect(exported).toContain("X-COMPONENT-FIELD;X-PARAM=alpha:beta");
	});
});
