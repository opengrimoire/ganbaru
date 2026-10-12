import ICAL from "ical.js";
import { Temporal } from "@js-temporal/polyfill";
import type {
	Calendar,
	CalendarEvent,
	EventAlarm,
	EventAttendee,
	EventOrganizer,
	EventOverride,
} from "$lib/calendar/types";
import { recurrenceToRrule } from "$lib/calendar/rrule";
import { wallClockToUtcIso } from "$lib/calendar/utils";

const CRLF = "\r\n";
const PRODID = "-//Ganbaru AI//Calendar//EN";
const DEFAULT_METHOD = "PUBLISH";
const METHOD_TOKEN_RE = /^[A-Za-z0-9-]+$/;
const textEncoder = new TextEncoder();

type JcalProperty = [string, Record<string, unknown>, string, ...unknown[]];
type JcalComponent = [string, unknown[], unknown[]];

/**
 * Convert an in-memory wall-clock "YYYY-MM-DD HH:MM" pair to a UTC instant.
 * The in-memory representation is anchored to `renderZone` (device zone at
 * load time); we convert back through the same zone so the round trip is
 * lossless.
 */
function toUtcInstant(wallClock: string, renderZone: string, allDay: boolean | undefined): string {
	if (allDay) {
		return `${wallClock.substring(0, 10)}T00:00:00Z`;
	}
	return wallClockToUtcIso(wallClock, renderZone);
}

/**
 * Format a UTC instant as a UTC-Z RFC 5545 datetime, e.g. `20260601T140000Z`.
 */
function formatUtcDateTime(utcIso: string): string {
	const instant = Temporal.Instant.from(utcIso);
	const zoned = instant.toZonedDateTimeISO("UTC");
	return (
		String(zoned.year).padStart(4, "0") +
		String(zoned.month).padStart(2, "0") +
		String(zoned.day).padStart(2, "0") +
		"T" +
		String(zoned.hour).padStart(2, "0") +
		String(zoned.minute).padStart(2, "0") +
		String(zoned.second).padStart(2, "0") +
		"Z"
	);
}

/**
 * Format a UTC instant as a wall-clock RFC 5545 datetime in `zone`, e.g.
 * `20260601T090000`. Used together with `;TZID=` parameters.
 */
function formatZonedDateTime(utcIso: string, zone: string): string {
	const instant = Temporal.Instant.from(utcIso);
	const zoned = instant.toZonedDateTimeISO(zone);
	return (
		String(zoned.year).padStart(4, "0") +
		String(zoned.month).padStart(2, "0") +
		String(zoned.day).padStart(2, "0") +
		"T" +
		String(zoned.hour).padStart(2, "0") +
		String(zoned.minute).padStart(2, "0") +
		String(zoned.second).padStart(2, "0")
	);
}

/**
 * Format a UTC instant as a date-only RFC 5545 string, e.g. `20260601`.
 */
function formatUtcDate(utcIso: string): string {
	return utcIso.substring(0, 10).replace(/-/g, "");
}

function formatDateOnly(calendarDate: string): string {
	return calendarDate.substring(0, 10).replace(/-/g, "");
}

function addDaysToDateOnly(calendarDate: string, days: number): string {
	return Temporal.PlainDate.from(calendarDate.substring(0, 10)).add({ days }).toString();
}

function formatDateTimeOnDate(date: string, timeSource: string): string {
	const time = timeSource.slice(9);
	return `${formatDateOnly(date)}T${time}`;
}

function formatFloatingDateTime(calendarDate: string): string {
	const date = calendarDate.substring(0, 10);
	const time = calendarDate.substring(11, 16);
	return `${date}T${time}:00`;
}

/**
 * Compute the UTC offset (in minutes) of `zone` at the given instant. Used
 * to fill VTIMEZONE STANDARD/DAYLIGHT stub blocks.
 */
function getUtcOffsetMinutes(zone: string, instant: Temporal.Instant): number {
	const zoned = instant.toZonedDateTimeISO(zone);
	return Math.floor(zoned.offsetNanoseconds / 60_000_000_000);
}

function formatUtcOffset(minutes: number): string {
	const sign = minutes >= 0 ? "+" : "-";
	const abs = Math.abs(minutes);
	const h = Math.floor(abs / 60);
	const m = abs % 60;
	return `${sign}${String(h).padStart(2, "0")}${String(m).padStart(2, "0")}`;
}

/**
 * Build a stub VTIMEZONE block. We do not attempt to encode full DST rules;
 * we emit a single STANDARD subcomponent with the offset at the epoch. This
 * satisfies strict parsers (Outlook) and round-trips because consumers
 * resolve TZID as an IANA name anyway.
 */
function buildVTimezone(zone: string): string[] {
	if (zone === "UTC") return [];
	const offsetEpoch = getUtcOffsetMinutes(zone, Temporal.Instant.from("1970-01-01T00:00:00Z"));
	const offsetStr = formatUtcOffset(offsetEpoch);
	return [
		"BEGIN:VTIMEZONE",
		`TZID:${zone}`,
		"BEGIN:STANDARD",
		"DTSTART:19700101T000000",
		`TZOFFSETFROM:${offsetStr}`,
		`TZOFFSETTO:${offsetStr}`,
		"END:STANDARD",
		"END:VTIMEZONE",
	];
}

/**
 * Escape a TEXT value per RFC 5545 (backslash, semicolon, comma, newline).
 */
function escapeText(value: string): string {
	return value
		.replace(/\\/g, "\\\\")
		.replace(/;/g, "\\;")
		.replace(/,/g, "\\,")
		.replace(/\n/g, "\\n")
		.replace(/\r/g, "");
}

/**
 * Escape a parameter value per RFC 5545 plus RFC 6868. Parameter escaping is
 * separate from TEXT value escaping.
 */
function escapeParamValue(value: string): string {
	const encoded = value
		.replace(/\^/g, "^^")
		.replace(/"/g, "^'")
		.replace(/\r\n|\r|\n/g, "^n");
	if (/[:;,]/.test(encoded)) return `"${encoded}"`;
	return encoded;
}

function param(name: string, value: string): string {
	return `${name}=${escapeParamValue(value)}`;
}

function normalizeMethod(method: string | undefined): string {
	if (!method) return DEFAULT_METHOD;
	const normalized = method.trim().toUpperCase();
	return METHOD_TOKEN_RE.test(normalized) ? normalized : DEFAULT_METHOD;
}

function utf8ByteLength(value: string): number {
	return textEncoder.encode(value).length;
}

/**
 * Fold lines longer than 75 octets per RFC 5545 without splitting UTF-8
 * code points. Continuation lines include the leading space in the limit.
 */
function foldLine(line: string): string {
	if (utf8ByteLength(line) <= 75) return line;
	const out: string[] = [];
	let current = "";
	let currentBytes = 0;
	for (const char of Array.from(line)) {
		const charBytes = utf8ByteLength(char);
		if (current && currentBytes + charBytes > 75) {
			out.push(current);
			current = ` ${char}`;
			currentBytes = 1 + charBytes;
		} else {
			current += char;
			currentBytes += charBytes;
		}
	}
	if (current) out.push(current);
	return out.join(CRLF);
}

const STATUS_REVERSE: Record<NonNullable<CalendarEvent["status"]>, string> = {
	confirmed: "CONFIRMED",
	tentative: "TENTATIVE",
	cancelled: "CANCELLED",
};

const TRANSP_REVERSE: Record<NonNullable<CalendarEvent["transparency"]>, string> = {
	opaque: "OPAQUE",
	transparent: "TRANSPARENT",
};

const VISIBILITY_REVERSE: Record<NonNullable<CalendarEvent["visibility"]>, string> = {
	public: "PUBLIC",
	private: "PRIVATE",
};

const ROLE_REVERSE: Record<EventAttendee["role"], string> = {
	"chair": "CHAIR",
	"req-participant": "REQ-PARTICIPANT",
	"opt-participant": "OPT-PARTICIPANT",
	"non-participant": "NON-PARTICIPANT",
};

const STATUS_ATTENDEE_REVERSE: Record<EventAttendee["status"], string> = {
	"needs-action": "NEEDS-ACTION",
	"accepted": "ACCEPTED",
	"declined": "DECLINED",
	"tentative": "TENTATIVE",
	"delegated": "DELEGATED",
};

function buildAttendee(att: EventAttendee): string {
	const params: string[] = [];
	if (att.name) params.push(param("CN", att.name));
	params.push(`ROLE=${ROLE_REVERSE[att.role]}`);
	params.push(`PARTSTAT=${STATUS_ATTENDEE_REVERSE[att.status]}`);
	params.push(`RSVP=${att.rsvp ? "TRUE" : "FALSE"}`);
	return `ATTENDEE;${params.join(";")}:mailto:${att.email}`;
}

function buildOrganizer(org: EventOrganizer): string {
	if (org.name) return `ORGANIZER;${param("CN", org.name)}:mailto:${org.email}`;
	return `ORGANIZER:mailto:${org.email}`;
}

function buildRecurrenceId(
	recurrenceIdUtc: string,
	isAllDay: boolean,
	useTzid: boolean,
	homeZone: string,
	recurrenceRange?: EventOverride["recurrenceRange"],
): string {
	const rangeParam = recurrenceRange === "this-and-future" ? ";RANGE=THISANDFUTURE" : "";
	if (isAllDay) return `RECURRENCE-ID${rangeParam};VALUE=DATE:${formatUtcDate(recurrenceIdUtc)}`;
	if (useTzid && homeZone !== "UTC") {
		return `RECURRENCE-ID${rangeParam};TZID=${homeZone}:${formatZonedDateTime(recurrenceIdUtc, homeZone)}`;
	}
	return `RECURRENCE-ID${rangeParam}:${formatUtcDateTime(recurrenceIdUtc)}`;
}

function buildExdate(
	event: CalendarEvent,
	isAllDay: boolean,
	useTzid: boolean,
	homeZone: string,
	startUtc: string,
): string | undefined {
	if (!event.exceptions?.length) return undefined;
	if (isAllDay) {
		const exdates = event.exceptions.map((d) => formatDateOnly(d)).join(",");
		return `EXDATE;VALUE=DATE:${exdates}`;
	}

	if (useTzid && homeZone !== "UTC") {
		const localStart = formatZonedDateTime(startUtc, homeZone);
		const exdates = event.exceptions
			.map((d) => formatDateTimeOnDate(d, localStart))
			.join(",");
		return `EXDATE;TZID=${homeZone}:${exdates}`;
	}

	const utcStart = formatUtcDateTime(startUtc);
	const exdates = event.exceptions
		.map((d) => formatDateTimeOnDate(d, utcStart))
		.join(",");
	return `EXDATE:${exdates}`;
}

function recurrenceToExportRrule(
	event: CalendarEvent,
	isAllDay: boolean,
	homeZone: string,
): string | undefined {
	const recurrence = event.recurrence;
	if (!recurrence) return undefined;
	if (isAllDay || recurrence.end.type !== "until" || recurrence.end.date.includes("T")) {
		return recurrenceToRrule(recurrence);
	}

	// RFC 5545 requires timed recurrence UNTIL values to be UTC date-times.
	const untilInstant = Temporal.PlainDate.from(recurrence.end.date)
		.toZonedDateTime({
			timeZone: homeZone || "UTC",
			plainTime: Temporal.PlainTime.from("23:59:59"),
		})
		.toInstant()
		.toString();
	return recurrenceToRrule({
		...recurrence,
		end: { type: "until", date: untilInstant },
	});
}

function buildAlarm(alarm: EventAlarm): string[] {
	const lines = ["BEGIN:VALARM", `ACTION:${alarm.action.toUpperCase()}`];
	if (alarm.description) lines.push(`DESCRIPTION:${escapeText(alarm.description)}`);
	lines.push(`TRIGGER:${alarm.triggerValue}`);
	lines.push("END:VALARM");
	return lines;
}

const BASE_VEVENT_MERGE_PROPERTIES = new Set([
	"uid", "dtstamp", "dtstart", "dtend", "duration", "recurrence-id",
	"summary", "description", "location", "url", "rrule", "exdate", "rdate",
	"status", "transp", "class", "priority", "sequence", "categories", "geo",
	"organizer", "attendee", "x-google-guest-can-modify",
	"x-google-guest-can-invite-others", "x-google-guest-can-see-other-guests",
]);

const VALARM_MERGE_PROPERTIES = new Set([
	"action", "trigger", "description",
]);

const GENERATED_OWNED_PARAMS_BY_PROPERTY: Record<string, Set<string>> = {
	attendee: new Set(["cn", "role", "partstat", "rsvp"]),
	organizer: new Set(["cn"]),
	dtstart: new Set(["tzid", "value"]),
	dtend: new Set(["tzid", "value"]),
	exdate: new Set(["tzid", "value"]),
	rdate: new Set(["tzid", "value"]),
	"recurrence-id": new Set(["tzid", "value"]),
};

function isJcalComponent(value: unknown): value is JcalComponent {
	return (
		Array.isArray(value) &&
		typeof value[0] === "string" &&
		Array.isArray(value[1]) &&
		Array.isArray(value[2])
	);
}

function isJcalProperty(value: unknown): value is JcalProperty {
	return (
		Array.isArray(value) &&
		typeof value[0] === "string" &&
		typeof value[1] === "object" &&
		value[1] !== null &&
		!Array.isArray(value[1]) &&
		typeof value[2] === "string"
	);
}

function jcalProperties(component: JcalComponent): JcalProperty[] {
	return component[1].filter(isJcalProperty);
}

function jcalSubcomponents(component: JcalComponent): JcalComponent[] {
	return component[2].filter(isJcalComponent);
}

function firstJcalProperty(component: JcalComponent, name: string): JcalProperty | undefined {
	const target = name.toLowerCase();
	return jcalProperties(component).find((property) => property[0].toLowerCase() === target);
}

function cloneJcalComponent(value: unknown): JcalComponent | null {
	if (!isJcalComponent(value)) return null;
	const cloned = JSON.parse(JSON.stringify(value)) as unknown;
	return isJcalComponent(cloned) ? cloned : null;
}

function generatedVeventJcal(lines: string[]): JcalComponent | null {
	const text = [
		"BEGIN:VCALENDAR",
		"VERSION:2.0",
		...lines,
		"END:VCALENDAR",
	].join(CRLF) + CRLF;
	try {
		const parsed = ICAL.parse(text) as unknown;
		if (!isJcalComponent(parsed)) return null;
		return jcalSubcomponents(parsed)
			.find((component) => component[0].toLowerCase() === "vevent") ?? null;
	} catch {
		return null;
	}
}

function firstJcalValue(component: JcalComponent, name: string): string | undefined {
	const prop = firstJcalProperty(component, name);
	if (!prop) return undefined;
	const value = prop[3];
	if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
		return String(value);
	}
	if (value == null) return undefined;
	return JSON.stringify(value);
}

function jcalComponentToLines(component: JcalComponent): string[] {
	const text = ICAL.stringify(component) as string;
	return text.split(/\r\n|\n|\r/).filter((line) => line.length > 0);
}

function mergePropertyParams(
	preserved: JcalProperty,
	generated: JcalProperty,
): JcalProperty {
	const propertyName = generated[0].toLowerCase();
	const params = { ...preserved[1] };
	for (const ownedName of GENERATED_OWNED_PARAMS_BY_PROPERTY[propertyName] ?? []) {
		if (!(ownedName in generated[1])) delete params[ownedName];
	}
	return [
		generated[0],
		{ ...params, ...generated[1] },
		generated[2],
		...generated.slice(3),
	];
}

function mergeJcalProperties(
	preserved: JcalComponent,
	generated: JcalComponent,
	replaceNames: Set<string>,
): unknown[] {
	const generatedByName = new Map<string, JcalProperty[]>();
	for (const property of jcalProperties(generated)) {
		const name = property[0].toLowerCase();
		if (!replaceNames.has(name)) continue;
		const list = generatedByName.get(name) ?? [];
		list.push(property);
		generatedByName.set(name, list);
	}

	const consumed = new Set<JcalProperty>();
	const merged: unknown[] = [];
	for (const property of preserved[1]) {
		if (!isJcalProperty(property)) {
			merged.push(property);
			continue;
		}
		const name = property[0].toLowerCase();
		if (!replaceNames.has(name)) {
			merged.push(property);
			continue;
		}
		const replacement = generatedByName.get(name)?.shift();
		if (replacement) {
			consumed.add(replacement);
			merged.push(mergePropertyParams(property, replacement));
		}
	}

	for (const property of jcalProperties(generated)) {
		const name = property[0].toLowerCase();
		if (replaceNames.has(name) && !consumed.has(property)) {
			merged.push(property);
		}
	}
	return merged;
}

function mergeJcalSubcomponents(preserved: JcalComponent, generated: JcalComponent): unknown[] {
	const generatedAlarms = jcalSubcomponents(generated)
		.filter((component) => component[0].toLowerCase() === "valarm");
	let alarmIndex = 0;
	const merged: unknown[] = [];
	for (const component of preserved[2]) {
		if (!isJcalComponent(component)) {
			merged.push(component);
			continue;
		}
		if (component[0].toLowerCase() !== "valarm") {
			merged.push(component);
			continue;
		}
		const generatedAlarm = generatedAlarms[alarmIndex++];
		if (generatedAlarm) {
			merged.push(mergeJcalComponent(component, generatedAlarm, VALARM_MERGE_PROPERTIES));
		}
	}
	for (const alarm of generatedAlarms.slice(alarmIndex)) {
		merged.push(alarm);
	}
	return merged;
}

function mergeJcalComponent(
	preserved: JcalComponent,
	generated: JcalComponent,
	replaceNames: Set<string>,
): JcalComponent {
	return [
		generated[0],
		mergeJcalProperties(preserved, generated, replaceNames),
		mergeJcalSubcomponents(preserved, generated),
	];
}

function formatDurationSeconds(seconds: number): string {
	if (seconds <= 0) return "PT0S";
	const days = Math.floor(seconds / 86_400);
	const hours = Math.floor((seconds % 86_400) / 3_600);
	const minutes = Math.floor((seconds % 3_600) / 60);
	const secs = seconds % 60;
	let value = "P";
	if (days > 0) value += `${days}D`;
	if (hours > 0 || minutes > 0 || secs > 0) {
		value += "T";
		if (hours > 0) value += `${hours}H`;
		if (minutes > 0) value += `${minutes}M`;
		if (secs > 0) value += `${secs}S`;
	}
	return value;
}

function jcalDateDuration(start: JcalProperty, end: JcalProperty): string | null {
	if (start[2] !== "date" || end[2] !== "date") return null;
	const startDate = Temporal.PlainDate.from(String(start[3]));
	const endDate = Temporal.PlainDate.from(String(end[3]));
	const days = startDate.until(endDate).days;
	return days >= 0 ? `P${days}D` : null;
}

function jcalDateTimeDuration(start: JcalProperty, end: JcalProperty): string | null {
	if (start[2] !== "date-time" || end[2] !== "date-time") return null;
	const startValue = String(start[3]);
	const endValue = String(end[3]);
	let seconds: number;
	if (startValue.endsWith("Z") || endValue.endsWith("Z")) {
		seconds = Temporal.Instant.from(startValue)
			.until(Temporal.Instant.from(endValue))
			.total({ unit: "second" });
	} else {
		seconds = Temporal.PlainDateTime.from(startValue)
			.until(Temporal.PlainDateTime.from(endValue))
			.total({ unit: "second" });
	}
	return Number.isFinite(seconds) ? formatDurationSeconds(Math.trunc(seconds)) : null;
}

function durationFromGeneratedTimes(generated: JcalComponent): string | null {
	const start = firstJcalProperty(generated, "dtstart");
	const end = firstJcalProperty(generated, "dtend");
	if (!start || !end) return null;
	return jcalDateDuration(start, end) ?? jcalDateTimeDuration(start, end);
}

function withPreservedDurationShape(
	preserved: JcalComponent,
	generated: JcalComponent,
): JcalComponent {
	if (!firstJcalProperty(preserved, "duration") || firstJcalProperty(preserved, "dtend")) {
		return generated;
	}
	const duration = durationFromGeneratedTimes(generated);
	if (!duration) return generated;
	return [
		generated[0],
		generated[1].map((property) => {
			if (!isJcalProperty(property) || property[0].toLowerCase() !== "dtend") return property;
			return ["duration", {}, "duration", duration] satisfies JcalProperty;
		}),
		generated[2],
	];
}

function isFloatingJcalDateTime(property: JcalProperty | undefined): boolean {
	if (!property || property[2] !== "date-time") return false;
	if ("tzid" in property[1]) return false;
	const value = String(property[3]);
	return !value.endsWith("Z");
}

function withPreservedFloatingDateTimeShape(
	event: CalendarEvent,
	preserved: JcalComponent,
	generated: JcalComponent,
): JcalComponent {
	if (!isFloatingJcalDateTime(firstJcalProperty(preserved, "dtstart"))) {
		return generated;
	}
	return [
		generated[0],
		generated[1].map((property) => {
			if (!isJcalProperty(property)) return property;
			const name = property[0].toLowerCase();
			if (name === "dtstart") {
				return ["dtstart", {}, "date-time", formatFloatingDateTime(event.start)] satisfies JcalProperty;
			}
			if (name === "dtend") {
				return ["dtend", {}, "date-time", formatFloatingDateTime(event.end)] satisfies JcalProperty;
			}
			return property;
		}),
		generated[2],
	];
}

function veventMergePropertyNames(event: CalendarEvent): Set<string> {
	const names = new Set(BASE_VEVENT_MERGE_PROPERTIES);
	for (const key of Object.keys(event.extendedProperties ?? {})) {
		names.add(key.toLowerCase());
	}
	return names;
}

function mergePreservedVevent(event: CalendarEvent, generatedLines: string[]): string[] {
	const preserved = cloneJcalComponent(event.icalendarRawJcal);
	const generated = generatedVeventJcal(generatedLines);
	if (!preserved || !generated || preserved[0].toLowerCase() !== "vevent") {
		return generatedLines;
	}
	const generatedWithFloatingShape = withPreservedFloatingDateTimeShape(event, preserved, generated);
	const generatedWithShape = withPreservedDurationShape(preserved, generatedWithFloatingShape);
	return jcalComponentToLines(
		mergeJcalComponent(preserved, generatedWithShape, veventMergePropertyNames(event)),
	);
}

function preservedVTimezoneLines(rawJcal: unknown): { lines: string[]; tzid: string | null } | null {
	const component = cloneJcalComponent(rawJcal);
	if (!component || component[0].toLowerCase() !== "vtimezone") return null;
	return {
		lines: jcalComponentToLines(component),
		tzid: firstJcalValue(component, "tzid") ?? null,
	};
}

function preservedPassthroughComponentLines(rawJcal: unknown): string[] | null {
	const component = cloneJcalComponent(rawJcal);
	if (!component) return null;
	const componentType = component[0].toLowerCase();
	if (componentType === "vcalendar" || componentType === "vevent" || componentType === "vtimezone") {
		return null;
	}
	return jcalComponentToLines(component);
}

interface BuildVeventOptions {
	event: CalendarEvent;
	uid: string;
	renderZone: string;
	dtStamp: string;
	useTzid: boolean;
	recurrenceIdUtc?: string;
	recurrenceRange?: EventOverride["recurrenceRange"];
}

function buildVevent(opts: BuildVeventOptions): string[] {
	const { event, uid, renderZone, dtStamp, useTzid, recurrenceIdUtc, recurrenceRange } = opts;
	const isAllDay = event.allDay === true;
	const homeZone = event.timezone || "UTC";
	const startUtc = toUtcInstant(event.start, renderZone, isAllDay);
	const endUtc = toUtcInstant(event.end, renderZone, isAllDay);

	const lines: string[] = ["BEGIN:VEVENT", `UID:${uid}`, `DTSTAMP:${dtStamp}`];

	if (isAllDay) {
		lines.push(`DTSTART;VALUE=DATE:${formatDateOnly(event.start)}`);
		lines.push(`DTEND;VALUE=DATE:${formatDateOnly(addDaysToDateOnly(event.end, 1))}`);
	} else if (useTzid && homeZone !== "UTC") {
		lines.push(`DTSTART;TZID=${homeZone}:${formatZonedDateTime(startUtc, homeZone)}`);
		lines.push(`DTEND;TZID=${homeZone}:${formatZonedDateTime(endUtc, homeZone)}`);
	} else {
		lines.push(`DTSTART:${formatUtcDateTime(startUtc)}`);
		lines.push(`DTEND:${formatUtcDateTime(endUtc)}`);
	}

	if (recurrenceIdUtc) {
		lines.push(buildRecurrenceId(recurrenceIdUtc, isAllDay, useTzid, homeZone, recurrenceRange));
	}

	lines.push(`SUMMARY:${escapeText(event.title)}`);
	if (event.description) lines.push(`DESCRIPTION:${escapeText(event.description)}`);
	if (event.location) lines.push(`LOCATION:${escapeText(event.location)}`);
	if (event.url) lines.push(`URL:${event.url}`);

	const rrule = recurrenceToExportRrule(event, isAllDay, homeZone);
	if (rrule) lines.push(`RRULE:${rrule}`);

	const exdateLine = buildExdate(event, isAllDay, useTzid, homeZone, startUtc);
	if (exdateLine) lines.push(exdateLine);

	if (event.rdate?.length) {
		if (isAllDay) {
			const rdates = event.rdate.map((iso) => formatUtcDate(iso)).join(",");
			lines.push(`RDATE;VALUE=DATE:${rdates}`);
		} else {
			const rdates = event.rdate.map((iso) => formatUtcDateTime(iso)).join(",");
			lines.push(`RDATE:${rdates}`);
		}
	}

	if (event.status) lines.push(`STATUS:${STATUS_REVERSE[event.status]}`);
	if (event.transparency) lines.push(`TRANSP:${TRANSP_REVERSE[event.transparency]}`);
	if (event.visibility) lines.push(`CLASS:${VISIBILITY_REVERSE[event.visibility]}`);
	if (typeof event.priority === "number") lines.push(`PRIORITY:${event.priority}`);
	if (typeof event.sequence === "number") lines.push(`SEQUENCE:${event.sequence}`);
	if (event.categories?.length) lines.push(`CATEGORIES:${event.categories.map(escapeText).join(",")}`);
	if (event.geo) lines.push(`GEO:${event.geo.lat};${event.geo.lng}`);
	if (event.organizer) lines.push(buildOrganizer(event.organizer));
	if (event.attendees?.length) {
		for (const att of event.attendees) lines.push(buildAttendee(att));
	}
	if (event.alarms?.length) {
		for (const alarm of event.alarms) lines.push(...buildAlarm(alarm));
	}
	if (event.guestPermissions) {
		const gp = event.guestPermissions;
		lines.push(`X-GOOGLE-GUEST-CAN-MODIFY:${gp.canModify ? "TRUE" : "FALSE"}`);
		lines.push(`X-GOOGLE-GUEST-CAN-INVITE-OTHERS:${gp.canInviteOthers ? "TRUE" : "FALSE"}`);
		lines.push(`X-GOOGLE-GUEST-CAN-SEE-OTHER-GUESTS:${gp.canSeeOtherGuests ? "TRUE" : "FALSE"}`);
	}
	if (event.extendedProperties) {
		for (const [k, v] of Object.entries(event.extendedProperties)) {
			lines.push(`${k}:${escapeText(v)}`);
		}
	}

	lines.push("END:VEVENT");
	return mergePreservedVevent(event, lines);
}

function buildOverrideVevent(
	parent: CalendarEvent,
	override: EventOverride,
	renderZone: string,
	dtStamp: string,
	useTzid: boolean,
): string[] {
	// Only `title`/`start`/`end` fall back to the parent so the emitted VEVENT
	// always has the RFC-required SUMMARY/DTSTART/DTEND. Optional fields keep
	// the override's exact value (including `undefined`) so a round trip is
	// lossless: an override that "explicitly clears" the parent's location
	// must not re-inherit the parent's location during export.
	const childEvent: CalendarEvent = {
		...parent,
		title: override.title ?? parent.title,
		start: override.start ?? parent.start,
		end: override.end ?? parent.end,
		description: override.description,
		location: override.location,
		url: override.url,
		color: override.color,
		status: override.status,
		transparency: override.transparency,
		visibility: override.visibility,
		extendedProperties: override.extendedProperties,
		icalendarRawJcal: override.icalendarRawJcal,
		recurrence: undefined,
		exceptions: undefined,
		rdate: undefined,
		overrides: undefined,
	};
	if (!parent.sourceUid) {
		throw new Error("Cannot serialize override for parent without a sourceUid (UID).");
	}
	return buildVevent({
		event: childEvent,
		uid: parent.sourceUid,
		renderZone,
		dtStamp,
		useTzid,
		recurrenceIdUtc: override.recurrenceId,
		recurrenceRange: override.recurrenceRange,
	});
}

/**
 * Serialize a calendar plus its events into a single `.ics` string ready to
 * be written to disk. Emits VTIMEZONE stubs for each unique IANA zone among
 * recurring events, plus one VEVENT per master and one VEVENT per override.
 *
 * Non-recurring zoned events are emitted as UTC (`DTSTART:...Z`); recurring
 * ones use `DTSTART;TZID=<zone>:...` so RRULE expansion stays anchored to
 * the correct wall clock through DST.
 *
 * @param calendar - The calendar being exported (used for X-WR-CALNAME).
 * @param events - Master events plus their attached overrides.
 * @param renderZone - The zone in which `event.start`/`event.end` are
 *                    expressed (defaults to the device's IANA zone).
 */
export function serializeCalendarToIcs(
	calendar: Calendar,
	events: CalendarEvent[],
	renderZone?: string,
	preservedTimezones: unknown[] = [],
	preservedPassthroughComponents: unknown[] = [],
	preservedMethod?: string,
): string {
	const zone = renderZone ?? Intl.DateTimeFormat().resolvedOptions().timeZone ?? "UTC";
	const dtStamp = formatUtcDateTime(new Date().toISOString());

	const recurringZones = new Set<string>();
	for (const ev of events) {
		if (ev.recurrence && !ev.allDay && ev.timezone && ev.timezone !== "UTC") {
			recurringZones.add(ev.timezone);
		}
	}

	const lines: string[] = [
		"BEGIN:VCALENDAR",
		`PRODID:${PRODID}`,
		"VERSION:2.0",
		"CALSCALE:GREGORIAN",
		`METHOD:${normalizeMethod(preservedMethod)}`,
		`X-WR-CALNAME:${escapeText(calendar.name)}`,
	];

	const emittedTimezoneIds = new Set<string>();
	for (const timezone of preservedTimezones) {
		const preserved = preservedVTimezoneLines(timezone);
		if (!preserved) continue;
		lines.push(...preserved.lines);
		if (preserved.tzid) emittedTimezoneIds.add(preserved.tzid);
	}

	for (const component of preservedPassthroughComponents) {
		const preserved = preservedPassthroughComponentLines(component);
		if (preserved) lines.push(...preserved);
	}

	for (const zoneId of recurringZones) {
		if (emittedTimezoneIds.has(zoneId)) continue;
		lines.push(...buildVTimezone(zoneId));
	}

	for (const ev of events) {
		const uid = ev.sourceUid ?? ev.id;
		const useTzid = !!ev.recurrence;
		lines.push(...buildVevent({ event: ev, uid, renderZone: zone, dtStamp, useTzid }));
		if (ev.overrides?.length) {
			for (const ovr of ev.overrides) {
				lines.push(
					...buildOverrideVevent(
						{ ...ev, sourceUid: uid },
						ovr,
						zone,
						dtStamp,
						useTzid,
					),
				);
			}
		}
	}

	lines.push("END:VCALENDAR");
	return lines.map(foldLine).join(CRLF) + CRLF;
}
