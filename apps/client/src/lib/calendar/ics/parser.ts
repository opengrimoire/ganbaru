import ICAL from "ical.js";
import { Temporal } from "@js-temporal/polyfill";
import type {
	CalendarEvent,
	EventAlarm,
	EventAttendee,
	EventOrganizer,
	EventOverride,
	EventStatus,
	EventTransparency,
	EventVisibility,
	GeoCoordinates,
	GuestPermissions,
} from "$lib/calendar/types";
import { recurrenceToRrule, rruleToRecurrence } from "$lib/calendar/rrule";
import { utcIsoToWallClock, wallClockToUtcIso } from "$lib/calendar/utils";
import type {
	IcsParseResult,
	IcsPreservedComponent,
	IcsPreservedObject,
	IcsPreservationPayload,
	IcsPreservationStatus,
} from "./types";

type JcalProperty = [string, Record<string, unknown>, string, ...unknown[]];
type JcalComponent = [string, unknown[], unknown[]];
type RecurrenceRange = NonNullable<EventOverride["recurrenceRange"]>;

interface RawRecurrenceIdInfo {
	value: ICAL.Time;
	tzidHint?: string | null;
	recurrenceRange?: RecurrenceRange;
}

interface ParsedRecurrenceIdInfo {
	recurrenceId: string;
	recurrenceRange?: RecurrenceRange;
}

interface ParsedMasterEvent {
	event: CalendarEvent;
	sequence: number;
	revisionKey: string;
}

const MAX_UNFOLDED_LINE_CHARS = 2 * 1024 * 1024;
const MAX_COMPONENT_COUNT = 50_000;
const MAX_PROPERTY_COUNT = 500_000;
const MAX_NESTING_DEPTH = 32;
const MAX_INLINE_BINARY_CHARS = 1024 * 1024;

/**
 * Microsoft Windows to IANA timezone identifier map. Outlook emits
 * Windows zone names (e.g. "Pacific Standard Time") in TZID; Google emits
 * IANA names. The list below covers the most common Windows zones; missing
 * names fall back to UTC with a warning. Sourced from CLDR
 * (https://github.com/unicode-org/cldr/blob/main/common/supplemental/windowsZones.xml).
 */
const WINDOWS_TO_IANA: Record<string, string> = {
	"UTC": "UTC",
	"GMT Standard Time": "Europe/London",
	"GMT": "Etc/GMT",
	"Greenwich Standard Time": "Atlantic/Reykjavik",
	"W. Europe Standard Time": "Europe/Berlin",
	"Central Europe Standard Time": "Europe/Budapest",
	"Romance Standard Time": "Europe/Paris",
	"Central European Standard Time": "Europe/Warsaw",
	"E. Europe Standard Time": "Europe/Chisinau",
	"FLE Standard Time": "Europe/Kiev",
	"GTB Standard Time": "Europe/Bucharest",
	"Turkey Standard Time": "Europe/Istanbul",
	"Russian Standard Time": "Europe/Moscow",
	"Eastern Standard Time": "America/New_York",
	"Central Standard Time": "America/Chicago",
	"Mountain Standard Time": "America/Denver",
	"Pacific Standard Time": "America/Los_Angeles",
	"Alaskan Standard Time": "America/Anchorage",
	"Hawaiian Standard Time": "Pacific/Honolulu",
	"US Eastern Standard Time": "America/Indianapolis",
	"US Mountain Standard Time": "America/Phoenix",
	"Atlantic Standard Time": "America/Halifax",
	"SA Eastern Standard Time": "America/Cayenne",
	"SA Pacific Standard Time": "America/Bogota",
	"SA Western Standard Time": "America/La_Paz",
	"E. South America Standard Time": "America/Sao_Paulo",
	"Argentina Standard Time": "America/Argentina/Buenos_Aires",
	"Pacific SA Standard Time": "America/Santiago",
	"Newfoundland Standard Time": "America/St_Johns",
	"Tokyo Standard Time": "Asia/Tokyo",
	"Korea Standard Time": "Asia/Seoul",
	"China Standard Time": "Asia/Shanghai",
	"Singapore Standard Time": "Asia/Singapore",
	"Taipei Standard Time": "Asia/Taipei",
	"India Standard Time": "Asia/Kolkata",
	"Sri Lanka Standard Time": "Asia/Colombo",
	"Pakistan Standard Time": "Asia/Karachi",
	"Iran Standard Time": "Asia/Tehran",
	"Arabian Standard Time": "Asia/Dubai",
	"Arabic Standard Time": "Asia/Baghdad",
	"Israel Standard Time": "Asia/Jerusalem",
	"Egypt Standard Time": "Africa/Cairo",
	"South Africa Standard Time": "Africa/Johannesburg",
	"AUS Eastern Standard Time": "Australia/Sydney",
	"AUS Central Standard Time": "Australia/Darwin",
	"E. Australia Standard Time": "Australia/Brisbane",
	"W. Australia Standard Time": "Australia/Perth",
	"Cen. Australia Standard Time": "Australia/Adelaide",
	"New Zealand Standard Time": "Pacific/Auckland",
};

/**
 * Resolve a TZID emitted by ical.js to a usable IANA zone. Already-IANA
 * names pass through; Windows-style names are remapped via the lookup.
 * Unknown values fall back to UTC and append a warning.
 */
function resolveTimezone(tzid: string | null, warnings: string[]): string {
	if (!tzid) return "UTC";
	if (WINDOWS_TO_IANA[tzid]) return WINDOWS_TO_IANA[tzid];
	if (isIanaZone(tzid)) return tzid;
	if (!warnings.includes(`Unknown timezone "${tzid}", falling back to UTC.`)) {
		warnings.push(`Unknown timezone "${tzid}", falling back to UTC.`);
	}
	return "UTC";
}

function isIanaZone(tzid: string): boolean {
	try {
		new Intl.DateTimeFormat("en-US", { timeZone: tzid });
		return true;
	} catch {
		return false;
	}
}

/**
 * Convert an `ICAL.Time` to a UTC ISO 8601 instant ending in `Z`.
 *
 * - All-day (date-only) inputs become `YYYY-MM-DDT00:00:00Z` (RFC 5545 floating).
 * - When `tzidHint` is supplied (from the property's TZID parameter), the
 *   wall-clock is interpreted in that IANA zone via Temporal, regardless of
 *   what `ical.js` reports for `time.zone`. This matters because `ical.js`
 *   reports `floating` for any TZID it has not been pre-registered with via
 *   a VTIMEZONE block, dropping otherwise-valid IANA hints.
 * - Otherwise, floating wall-clock inputs (no TZID, no Z) are interpreted in
 *   the device's IANA zone, and zoned inputs are converted via `toJSDate()`.
 */
function timeToUtcIso(time: ICAL.Time, deviceZone: string, tzidHint?: string | null): string {
	if (time.isDate) {
		return `${dateOnlyFromTime(time)}T00:00:00Z`;
	}
	const zone = time.zone;
	const zoneTzid = (zone as ICAL.Timezone | null)?.tzid;
	// A `Z`-suffixed value is unambiguously UTC per RFC 5545. RFC 5545 also
	// disallows mixing TZID with Z, so any inherited hint must be ignored.
	if (zoneTzid === "UTC") {
		return Temporal.Instant.from(time.toJSDate().toISOString()).toString();
	}
	if (tzidHint) {
		const wall = formatWallClock(time);
		return wallClockToUtcIso(wall, tzidHint);
	}
	const isFloating = !zoneTzid || zoneTzid === "floating";
	if (isFloating) {
		const wall = formatWallClock(time);
		return wallClockToUtcIso(wall, deviceZone);
	}
	return Temporal.Instant.from(time.toJSDate().toISOString()).toString();
}

function dateOnlyFromTime(time: ICAL.Time): string {
	const y = String(time.year).padStart(4, "0");
	const m = String(time.month).padStart(2, "0");
	const d = String(time.day).padStart(2, "0");
	return `${y}-${m}-${d}`;
}

function inclusiveAllDayEndFromExclusive(startDate: string, exclusiveEndDate: string): string {
	const start = Temporal.PlainDate.from(startDate);
	const inclusiveEnd = Temporal.PlainDate.from(exclusiveEndDate).subtract({ days: 1 });
	if (Temporal.PlainDate.compare(inclusiveEnd, start) < 0) return startDate;
	return inclusiveEnd.toString();
}

function inclusiveAllDayEndFromDuration(startDate: string, duration: ICAL.Duration): string {
	const seconds = duration.toSeconds();
	if (seconds <= 0) return startDate;
	const days = Math.max(1, Math.ceil(seconds / 86_400));
	return Temporal.PlainDate.from(startDate).add({ days: days - 1 }).toString();
}

function formatWallClock(time: ICAL.Time): string {
	const y = String(time.year).padStart(4, "0");
	const m = String(time.month).padStart(2, "0");
	const d = String(time.day).padStart(2, "0");
	const h = String(time.hour).padStart(2, "0");
	const min = String(time.minute).padStart(2, "0");
	return `${y}-${m}-${d} ${h}:${min}`;
}

function getDeviceZone(): string {
	try {
		return Intl.DateTimeFormat().resolvedOptions().timeZone;
	} catch {
		return "UTC";
	}
}

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
	return jcalProperties(component).find((prop) => prop[0].toLowerCase() === target);
}

function firstJcalValue(component: JcalComponent, name: string): string | undefined {
	const prop = firstJcalProperty(component, name);
	if (!prop) return undefined;
	const value = prop[3];
	if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
		return String(value);
	}
	if (value === undefined || value === null) return undefined;
	return JSON.stringify(value);
}

function firstJcalNumber(component: JcalComponent, name: string): number | undefined {
	const value = firstJcalValue(component, name);
	if (value === undefined) return undefined;
	const parsed = Number(value);
	return Number.isFinite(parsed) ? parsed : undefined;
}

function sourceFingerprint(text: string): string {
	let hash = 0x811c9dc5;
	for (const char of text) {
		hash ^= char.codePointAt(0) ?? 0;
		hash = Math.imul(hash, 0x01000193) >>> 0;
	}
	return hash.toString(16).padStart(8, "0");
}

function unfoldContentLines(text: string): { lines: string[] } | { error: string } {
	const physicalLines = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
	const lines: string[] = [];
	let current = "";
	let hasCurrent = false;

	for (const physicalLine of physicalLines) {
		if (physicalLine.startsWith(" ") || physicalLine.startsWith("\t")) {
			if (!hasCurrent) {
				return { error: "iCalendar import has a folded continuation before any content line." };
			}
			current += physicalLine.slice(1);
		} else {
			if (hasCurrent) lines.push(current);
			current = physicalLine;
			hasCurrent = true;
		}
		if (current.length > MAX_UNFOLDED_LINE_CHARS) {
			return {
				error: `iCalendar import exceeds the unfolded line limit of ${MAX_UNFOLDED_LINE_CHARS} characters.`,
			};
		}
	}
	if (hasCurrent && current.length > 0) lines.push(current);
	return { lines };
}

function inlineBinaryValueLength(line: string): number | null {
	const separatorIndex = line.indexOf(":");
	if (separatorIndex === -1) return null;
	const header = line.slice(0, separatorIndex).toUpperCase();
	if (!header.startsWith("ATTACH")) return null;
	if (!header.includes("VALUE=BINARY") && !header.includes("ENCODING=BASE64")) return null;
	return line.length - separatorIndex - 1;
}

function validateIcsTextBudget(text: string): string | null {
	const unfolded = unfoldContentLines(text);
	if ("error" in unfolded) return unfolded.error;

	let componentCount = 0;
	let propertyCount = 0;
	let depth = 0;

	for (const line of unfolded.lines) {
		const upper = line.toUpperCase();
		if (upper.startsWith("BEGIN:")) {
			componentCount++;
			depth++;
			if (componentCount > MAX_COMPONENT_COUNT) {
				return `iCalendar import exceeds the component limit of ${MAX_COMPONENT_COUNT}.`;
			}
			if (depth > MAX_NESTING_DEPTH) {
				return `iCalendar import exceeds the component nesting limit of ${MAX_NESTING_DEPTH}.`;
			}
			continue;
		}
		if (upper.startsWith("END:")) {
			depth = Math.max(0, depth - 1);
			continue;
		}
		if (line.trim().length === 0) continue;

		propertyCount++;
		if (propertyCount > MAX_PROPERTY_COUNT) {
			return `iCalendar import exceeds the property limit of ${MAX_PROPERTY_COUNT}.`;
		}
		const binaryLength = inlineBinaryValueLength(line);
		if (binaryLength !== null && binaryLength > MAX_INLINE_BINARY_CHARS) {
			return `iCalendar import exceeds the inline binary attachment limit of ${MAX_INLINE_BINARY_CHARS} characters.`;
		}
	}

	return null;
}

function preservationStatusFor(componentType: string): IcsPreservationStatus {
	switch (componentType) {
		case "vevent":
		case "valarm":
		case "vtimezone":
			return "partial";
		case "standard":
		case "daylight":
			return "unsupported";
		default:
			return "unsupported";
	}
}

const PROJECTED_VEVENT_PROPERTIES = new Set([
	"uid", "summary", "description", "location", "url", "dtstart", "dtend",
	"duration", "rrule", "rdate", "exdate", "recurrence-id", "status", "class",
	"transp", "priority", "sequence", "categories", "geo", "organizer",
	"attendee", "dtstamp", "created", "last-modified",
	"x-google-guest-can-modify", "x-google-guest-can-invite-others",
	"x-google-guest-can-see-other-guests",
]);

const PROJECTED_VALARM_PROPERTIES = new Set([
	"action", "trigger", "description",
]);

const SUPPORTED_PARAMS_BY_PROPERTY: Record<string, Set<string>> = {
	attendee: new Set(["cn", "role", "partstat", "rsvp"]),
	organizer: new Set(["cn"]),
	dtstart: new Set(["tzid", "value"]),
	dtend: new Set(["tzid", "value"]),
	exdate: new Set(["tzid", "value"]),
	rdate: new Set(["tzid", "value"]),
	"recurrence-id": new Set(["tzid", "value", "range"]),
};

function pushUnique(list: string[], message: string): void {
	if (!list.includes(message)) list.push(message);
}

function addUnsupportedParameterWarnings(
	warnings: string[],
	componentType: string,
	property: JcalProperty,
): void {
	const propertyName = property[0].toLowerCase();
	const supported = SUPPORTED_PARAMS_BY_PROPERTY[propertyName] ?? new Set<string>();
	for (const paramName of Object.keys(property[1])) {
		const normalized = paramName.toLowerCase();
		if (supported.has(normalized)) continue;
		pushUnique(
			warnings,
			`${componentType.toUpperCase()} property ${propertyName.toUpperCase()} parameter ${normalized.toUpperCase()} is preserved but not projected.`,
		);
	}
}

function projectionWarningsFor(component: JcalComponent): string[] {
	const componentType = component[0].toLowerCase();
	const warnings: string[] = [];
	if (componentType === "vevent") {
		for (const property of jcalProperties(component)) {
			const propertyName = property[0].toLowerCase();
			if (!PROJECTED_VEVENT_PROPERTIES.has(propertyName)) {
				const projectedAsExtension = propertyName.startsWith("x-");
				pushUnique(
					warnings,
					projectedAsExtension
						? `VEVENT property ${propertyName.toUpperCase()} is preserved as an extended property only.`
						: `VEVENT property ${propertyName.toUpperCase()} is preserved but not projected.`,
				);
			}
			addUnsupportedParameterWarnings(warnings, componentType, property);
		}
		return warnings;
	}
	if (componentType === "valarm") {
		for (const property of jcalProperties(component)) {
			const propertyName = property[0].toLowerCase();
			if (!PROJECTED_VALARM_PROPERTIES.has(propertyName)) {
				pushUnique(
					warnings,
					`VALARM property ${propertyName.toUpperCase()} is preserved but not projected.`,
				);
			}
			addUnsupportedParameterWarnings(warnings, componentType, property);
		}
		return warnings;
	}
	if (componentType === "vtimezone") {
		pushUnique(
			warnings,
			"VTIMEZONE is preserved for export while the app projection uses a resolved timezone identifier.",
		);
		return warnings;
	}
	pushUnique(
		warnings,
		`${componentType.toUpperCase()} is preserved without a Ganbaru AI projection.`,
	);
	return warnings;
}

function preservedComponentFromJcal(component: JcalComponent): IcsPreservedComponent {
	const componentType = component[0].toLowerCase();
	const recurrenceIdProp = firstJcalProperty(component, "recurrence-id");
	const recurrenceIdValueType = recurrenceIdProp?.[2];
	return {
		componentType,
		uid: firstJcalValue(component, "uid"),
		recurrenceId: firstJcalValue(component, "recurrence-id"),
		recurrenceIdValueType,
		sequence: firstJcalNumber(component, "sequence"),
		dtstartKey: firstJcalValue(component, "dtstart"),
		rawJcal: component,
		preservationStatus: preservationStatusFor(componentType),
		projectionWarnings: projectionWarningsFor(component),
		components: jcalSubcomponents(component).map(preservedComponentFromJcal),
	};
}

function preservedObjectFromJcal(component: JcalComponent, warnings: string[]): IcsPreservedObject {
	return {
		prodid: firstJcalValue(component, "prodid"),
		version: firstJcalValue(component, "version"),
		method: firstJcalValue(component, "method"),
		calendarScale: firstJcalValue(component, "calscale"),
		rawJcal: component,
		diagnostics: warnings,
		components: jcalSubcomponents(component).map(preservedComponentFromJcal),
	};
}

function buildPreservationPayload(
	text: string,
	vcalendarJcals: JcalComponent[],
	warnings: string[],
): IcsPreservationPayload | undefined {
	if (vcalendarJcals.length === 0) return undefined;
	return {
		sourceFingerprint: sourceFingerprint(text),
		objects: vcalendarJcals.map((component) => preservedObjectFromJcal(component, warnings)),
	};
}

const ATTENDEE_ROLE_MAP: Record<string, EventAttendee["role"]> = {
	"CHAIR": "chair",
	"REQ-PARTICIPANT": "req-participant",
	"OPT-PARTICIPANT": "opt-participant",
	"NON-PARTICIPANT": "non-participant",
};

const ATTENDEE_STATUS_MAP: Record<string, EventAttendee["status"]> = {
	"NEEDS-ACTION": "needs-action",
	"ACCEPTED": "accepted",
	"DECLINED": "declined",
	"TENTATIVE": "tentative",
	"DELEGATED": "delegated",
};

function stripMailto(value: string): string {
	return value.replace(/^mailto:/i, "");
}

function parseAttendee(prop: ICAL.Property): EventAttendee {
	const value = prop.getFirstValue();
	const email = typeof value === "string" ? stripMailto(value) : "";
	const cn = prop.getFirstParameter("cn");
	const role = prop.getFirstParameter("role")?.toUpperCase();
	const partstat = prop.getFirstParameter("partstat")?.toUpperCase();
	const rsvp = prop.getFirstParameter("rsvp")?.toUpperCase() === "TRUE";
	return {
		id: crypto.randomUUID(),
		name: cn || undefined,
		email,
		role: (role && ATTENDEE_ROLE_MAP[role]) || "req-participant",
		status: (partstat && ATTENDEE_STATUS_MAP[partstat]) || "needs-action",
		rsvp,
	};
}

function parseOrganizer(prop: ICAL.Property): EventOrganizer {
	const value = prop.getFirstValue();
	const email = typeof value === "string" ? stripMailto(value) : "";
	const cn = prop.getFirstParameter("cn");
	return cn ? { name: cn, email } : { email };
}

function sortableTimeValue(value: unknown): string {
	if (value instanceof ICAL.Time) {
		return value.toJSDate().toISOString();
	}
	return typeof value === "string" ? value : "";
}

function componentRevisionKey(component: ICAL.Component): string {
	return sortableTimeValue(component.getFirstPropertyValue("last-modified"))
		|| sortableTimeValue(component.getFirstPropertyValue("dtstamp"))
		|| sortableTimeValue(component.getFirstPropertyValue("created"));
}

function shouldReplaceMasterEvent(existing: ParsedMasterEvent, candidate: ParsedMasterEvent): boolean {
	if (candidate.sequence !== existing.sequence) return candidate.sequence > existing.sequence;
	if (candidate.revisionKey && candidate.revisionKey !== existing.revisionKey) {
		return candidate.revisionKey > existing.revisionKey;
	}
	return false;
}

function parseAlarm(
	component: ICAL.Component,
	warnings: string[],
): EventAlarm | null {
	const action = (component.getFirstPropertyValue("action") as string | null)?.toUpperCase();
	const triggerProp = component.getFirstProperty("trigger");
	if (!action || !triggerProp) return null;
	const validAction: EventAlarm["action"] =
		action === "AUDIO" || action === "EMAIL" ? (action.toLowerCase() as EventAlarm["action"]) : "display";

	const triggerValue = triggerProp.getFirstValue();
	let triggerType: EventAlarm["triggerType"];
	let triggerString: string;
	if (triggerValue instanceof ICAL.Duration) {
		triggerType = "relative";
		triggerString = triggerValue.toString();
	} else if (triggerValue instanceof ICAL.Time) {
		triggerType = "absolute";
		triggerString = triggerValue.toJSDate().toISOString();
	} else if (typeof triggerValue === "string") {
		triggerType = triggerValue.startsWith("P") || triggerValue.startsWith("-P") ? "relative" : "absolute";
		triggerString = triggerValue;
	} else {
		return null;
	}

	const description = (component.getFirstPropertyValue("description") as string | null) ?? undefined;
	if (component.hasProperty("repeat") || component.hasProperty("duration")) {
		const msg = "VALARM REPEAT and DURATION dropped on import (not represented in schema).";
		if (!warnings.includes(msg)) warnings.push(msg);
	}
	return {
		id: crypto.randomUUID(),
		action: validAction,
		triggerType,
		triggerValue: triggerString,
		description,
	};
}

const STATUS_MAP: Record<string, EventStatus> = {
	"CONFIRMED": "confirmed",
	"TENTATIVE": "tentative",
	"CANCELLED": "cancelled",
};

const TRANSP_MAP: Record<string, EventTransparency> = {
	"OPAQUE": "opaque",
	"TRANSPARENT": "transparent",
};

const VISIBILITY_MAP: Record<string, EventVisibility> = {
	"PUBLIC": "public",
	"PRIVATE": "private",
	"CONFIDENTIAL": "private",
};

const STANDARD_PROPS = new Set([
	"uid", "summary", "description", "location", "url", "dtstart", "dtend",
	"duration", "rrule", "rdate", "exdate", "recurrence-id", "status", "class",
	"transp", "priority", "sequence", "categories", "geo", "organizer",
	"attendee", "dtstamp", "created", "last-modified",
]);

const GOOGLE_MEET_URL_RE = /^https:\/\/meet\.google\.com\/[a-z0-9-]+$/i;

function googleMeetUrlFromValue(value: unknown): string | undefined {
	if (typeof value !== "string") return undefined;
	const trimmed = value.trim();
	return GOOGLE_MEET_URL_RE.test(trimmed) ? trimmed : undefined;
}

function googleConferenceUrl(component: ICAL.Component): string | undefined {
	return googleMeetUrlFromValue(component.getFirstPropertyValue("x-google-conference"));
}

function isGoogleMeetBoundary(line: string): boolean {
	const trimmed = line.trim();
	return trimmed.startsWith("-::~") && trimmed.endsWith(":-");
}

function stripGoogleMeetGeneratedDescription(
	description: string | undefined,
	conferenceUrl: string | undefined,
): string | undefined {
	if (!description || !conferenceUrl || !description.includes("Join with Google Meet:")) {
		return description;
	}

	const lines = description.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
	const kept: string[] = [];
	for (let index = 0; index < lines.length; index++) {
		if (!isGoogleMeetBoundary(lines[index])) {
			kept.push(lines[index]);
			continue;
		}

		let end = index + 1;
		while (end < lines.length && !isGoogleMeetBoundary(lines[end])) end++;
		if (end >= lines.length) {
			kept.push(lines[index]);
			continue;
		}

		const block = lines.slice(index, end + 1).join("\n");
		if (block.includes("Join with Google Meet:") && block.includes(conferenceUrl)) {
			index = end;
			continue;
		}

		kept.push(...lines.slice(index, end + 1));
		index = end;
	}

	const cleaned = kept.join("\n").replace(/\n{3,}/g, "\n\n").trim();
	return cleaned || undefined;
}

function collectExtendedProperties(component: ICAL.Component): {
	extended: Record<string, string>;
	guests: GuestPermissions | undefined;
} {
	const extended: Record<string, string> = {};
	let guests: GuestPermissions | undefined;
	for (const prop of component.getAllProperties()) {
		const name = prop.name;
		if (STANDARD_PROPS.has(name)) continue;
		const value = prop.getFirstValue();
		const stringValue = typeof value === "string" ? value : value?.toString() ?? "";
		switch (name.toUpperCase()) {
			case "X-GOOGLE-GUEST-CAN-MODIFY":
				guests = guests ?? defaultGuestPermissions();
				guests.canModify = stringValue.toUpperCase() === "TRUE";
				break;
			case "X-GOOGLE-GUEST-CAN-INVITE-OTHERS":
				guests = guests ?? defaultGuestPermissions();
				guests.canInviteOthers = stringValue.toUpperCase() === "TRUE";
				break;
			case "X-GOOGLE-GUEST-CAN-SEE-OTHER-GUESTS":
				guests = guests ?? defaultGuestPermissions();
				guests.canSeeOtherGuests = stringValue.toUpperCase() === "TRUE";
				break;
			default:
				if (name.startsWith("x-")) {
					extended[name.toUpperCase()] = stringValue;
				}
		}
	}
	return { extended, guests };
}

function defaultGuestPermissions(): GuestPermissions {
	return { canModify: false, canInviteOthers: true, canSeeOtherGuests: true };
}

function parseExdates(
	component: ICAL.Component,
	deviceZone: string,
	homeZone: string,
	warnings: string[],
): string[] {
	const out: string[] = [];
	for (const prop of component.getAllProperties("exdate")) {
		const tzidParam = prop.getFirstParameter("tzid");
		const hint = tzidParam ? resolveTimezone(tzidParam, warnings) : undefined;
		for (const value of prop.getValues()) {
			if (value instanceof ICAL.Time) {
				out.push(recurrenceLocalDate(value, deviceZone, homeZone, hint));
			}
		}
	}
	return out;
}

function recurrenceLocalDate(
	time: ICAL.Time,
	deviceZone: string,
	homeZone: string,
	tzidHint?: string | null,
): string {
	if (time.isDate || tzidHint) return dateOnlyFromTime(time);
	const zoneTzid = (time.zone as ICAL.Timezone | null)?.tzid;
	if (!zoneTzid || zoneTzid === "floating") return dateOnlyFromTime(time);
	const utc = timeToUtcIso(time, deviceZone);
	return utcIsoToWallClock(utc, homeZone).slice(0, 10);
}

function isFloatingTime(time: ICAL.Time): boolean {
	const zoneTzid = (time.zone as ICAL.Timezone | null)?.tzid;
	return !zoneTzid || zoneTzid === "floating";
}

function recurrenceRangeFromProperty(prop: ICAL.Property): RecurrenceRange | undefined {
	const range = prop.getFirstParameter("range");
	return typeof range === "string" && range.toUpperCase() === "THISANDFUTURE"
		? "this-and-future"
		: undefined;
}

function rawRecurrenceIdInfo(
	component: ICAL.Component,
	warnings: string[],
	fallbackTzidHint?: string | null,
): RawRecurrenceIdInfo | undefined {
	const recurrenceIdProp = component.getFirstProperty("recurrence-id");
	if (!recurrenceIdProp) return undefined;
	const value = recurrenceIdProp.getFirstValue();
	if (!(value instanceof ICAL.Time)) return undefined;
	const ridTzid = recurrenceIdProp.getFirstParameter("tzid");
	const tzidHint = ridTzid ? resolveTimezone(ridTzid, warnings) : fallbackTzidHint;
	const recurrenceRange = recurrenceRangeFromProperty(recurrenceIdProp);
	return { value, tzidHint, recurrenceRange };
}

function materializeRecurrenceId(
	info: RawRecurrenceIdInfo,
	deviceZone: string,
	homeZone: string,
): ParsedRecurrenceIdInfo {
	const utcHint = info.tzidHint ?? (isFloatingTime(info.value) ? homeZone : undefined);
	const recurrenceId = timeToUtcIso(info.value, deviceZone, utcHint);
	return {
		recurrenceId,
		recurrenceRange: info.recurrenceRange,
	};
}

function parseRdates(
	component: ICAL.Component,
	deviceZone: string,
	warnings: string[],
): string[] {
	const out: string[] = [];
	for (const prop of component.getAllProperties("rdate")) {
		const tzidParam = prop.getFirstParameter("tzid");
		const hint = tzidParam ? resolveTimezone(tzidParam, warnings) : undefined;
		for (const value of prop.getValues()) {
			if (value instanceof ICAL.Time) {
				out.push(timeToUtcIso(value, deviceZone, hint));
			}
		}
	}
	return out;
}

function calendarEventBaseFromComponent(
	component: ICAL.Component,
	calendarId: string,
	deviceZone: string,
	warnings: string[],
): { event: CalendarEvent; recurrenceId?: ParsedRecurrenceIdInfo } | null {
	const dtstartProp = component.getFirstProperty("dtstart");
	if (!dtstartProp) return null;
	const dtstartValue = dtstartProp.getFirstValue();
	if (!(dtstartValue instanceof ICAL.Time)) return null;

	const isAllDay = dtstartValue.isDate;
	const dtstartTzid = dtstartProp.getFirstParameter("tzid");
	const dtstartHint = dtstartTzid ? resolveTimezone(dtstartTzid, warnings) : undefined;
	const startDate = dateOnlyFromTime(dtstartValue);
	const startUtc = isAllDay ? `${startDate}T00:00:00Z` : timeToUtcIso(dtstartValue, deviceZone, dtstartHint);

	const homeZone = isAllDay
		? deviceZone
		: dtstartHint
			? dtstartHint
			: dtstartValue.zone && (dtstartValue.zone as ICAL.Timezone).tzid === "UTC"
				? "UTC"
				: deviceZone;

	const sourceUid = (component.getFirstPropertyValue("uid") as string | null) ?? undefined;
	const summary = (component.getFirstPropertyValue("summary") as string | null) ?? "";
	let endUtc: string;
	let allDayEndDate = startDate;
	const dtendProp = component.getFirstProperty("dtend");
	const durationProp = component.getFirstProperty("duration");
	if (isAllDay) {
		if (dtendProp) {
			const dtendValue = dtendProp.getFirstValue();
			if (dtendValue instanceof ICAL.Time) {
				const rawEndDate = dateOnlyFromTime(dtendValue);
				allDayEndDate = dtendValue.isDate
					? inclusiveAllDayEndFromExclusive(startDate, rawEndDate)
					: rawEndDate;
			}
		} else if (durationProp) {
			const durValue = durationProp.getFirstValue();
			if (durValue instanceof ICAL.Duration) {
				allDayEndDate = inclusiveAllDayEndFromDuration(startDate, durValue);
			}
		}
		endUtc = `${allDayEndDate}T00:00:00Z`;
	} else if (dtendProp) {
		const dtendValue = dtendProp.getFirstValue();
		if (dtendValue instanceof ICAL.Time) {
			const dtendTzid = dtendProp.getFirstParameter("tzid");
			const dtendHint = dtendTzid ? resolveTimezone(dtendTzid, warnings) : dtstartHint;
			endUtc = timeToUtcIso(dtendValue, deviceZone, dtendHint);
		} else {
			endUtc = startUtc;
		}
	} else if (durationProp) {
		const durValue = durationProp.getFirstValue();
		if (durValue instanceof ICAL.Duration) {
			const startInstant = Temporal.Instant.from(startUtc);
			const seconds = durValue.toSeconds();
			endUtc = startInstant.add({ seconds }).toString();
		} else {
			endUtc = startUtc;
		}
	} else {
		endUtc = startUtc;
	}

	// In-memory wall-clock is anchored to the device zone (matching `mapRow`
	// in `db-rows.ts`). The home zone is preserved in `event.timezone`
	// for recurrence anchoring; the serializer reverses through `deviceZone`
	// so the round trip is lossless.
	const start = isAllDay ? `${startDate} 00:00` : utcIsoToWallClock(startUtc, deviceZone);
	const end = isAllDay ? `${allDayEndDate} 00:00` : utcIsoToWallClock(endUtc, deviceZone);

	const rawDescription = (component.getFirstPropertyValue("description") as string | null) ?? undefined;
	const location = (component.getFirstPropertyValue("location") as string | null) ?? undefined;
	const conferenceUrl = googleConferenceUrl(component);
	const description = stripGoogleMeetGeneratedDescription(rawDescription, conferenceUrl);
	const url = (component.getFirstPropertyValue("url") as string | null) ?? conferenceUrl ?? undefined;
	const status = (component.getFirstPropertyValue("status") as string | null);
	const transp = (component.getFirstPropertyValue("transp") as string | null);
	const visibility = (component.getFirstPropertyValue("class") as string | null);
	const priority = component.getFirstPropertyValue("priority");
	const sequence = component.getFirstPropertyValue("sequence");

	const categoriesProp = component.getFirstProperty("categories");
	const categories: string[] | undefined = categoriesProp
		? categoriesProp.getValues().map((v): string => (typeof v === "string" ? v : v?.toString() ?? ""))
		: undefined;

	const geoValue = component.getFirstPropertyValue("geo");
	const geo: GeoCoordinates | undefined = Array.isArray(geoValue) && geoValue.length >= 2
		? { lat: Number(geoValue[0]), lng: Number(geoValue[1]) }
		: undefined;

	const organizerProp = component.getFirstProperty("organizer");
	const organizer = organizerProp ? parseOrganizer(organizerProp) : undefined;

	const attendees: EventAttendee[] = component
		.getAllProperties("attendee")
		.map((p) => parseAttendee(p));

	const alarms: EventAlarm[] = component
		.getAllSubcomponents("valarm")
		.map((c) => parseAlarm(c, warnings))
		.filter((a): a is EventAlarm => a !== null);

	const exceptions = parseExdates(component, deviceZone, homeZone, warnings);
	const rdate = parseRdates(component, deviceZone, warnings);

	const rruleValue = component.getFirstPropertyValue("rrule");
	let rruleString: string | undefined;
	if (rruleValue instanceof ICAL.Recur) {
		const original = rruleValue.toString();
		try {
			const config = rruleToRecurrence(original);
			rruleString = recurrenceToRrule(config);
		} catch {
			rruleString = original;
			warnings.push(`Failed to canonicalize RRULE "${original}"; stored as-is.`);
		}
	}

	const { extended, guests } = collectExtendedProperties(component);

	const rawRecurrenceId = rawRecurrenceIdInfo(component, warnings, dtstartHint);
	const recurrenceId = rawRecurrenceId
		? materializeRecurrenceId(rawRecurrenceId, deviceZone, homeZone)
		: undefined;

	const event: CalendarEvent = {
		id: crypto.randomUUID(),
		title: summary,
		start,
		end,
		timezone: homeZone,
		calendarId,
		allDay: isAllDay || undefined,
	};
	if (description) event.description = description;
	if (location) event.location = location;
	if (url) event.url = url;
	if (sourceUid) event.sourceUid = sourceUid;
	if (status && STATUS_MAP[status.toUpperCase()]) event.status = STATUS_MAP[status.toUpperCase()];
	if (transp && TRANSP_MAP[transp.toUpperCase()]) event.transparency = TRANSP_MAP[transp.toUpperCase()];
	if (visibility && VISIBILITY_MAP[visibility.toUpperCase()]) {
		event.visibility = VISIBILITY_MAP[visibility.toUpperCase()];
	}
	if (typeof priority === "number") event.priority = priority;
	if (typeof sequence === "number") event.sequence = sequence;
	if (categories?.length) event.categories = categories;
	if (geo) event.geo = geo;
	if (organizer) event.organizer = organizer;
	if (attendees.length) event.attendees = attendees;
	if (alarms.length) event.alarms = alarms;
	if (exceptions.length) event.exceptions = exceptions;
	if (rdate.length) event.rdate = rdate;
	if (rruleString) {
		event.recurrence = rruleToRecurrence(rruleString);
	}
	if (Object.keys(extended).length) event.extendedProperties = extended;
	if (guests) event.guestPermissions = guests;

	return { event, recurrenceId };
}

function buildOverride(
	parentEvent: CalendarEvent,
	child: CalendarEvent,
	recurrenceId: ParsedRecurrenceIdInfo,
): EventOverride {
	const override: EventOverride = {
		id: crypto.randomUUID(),
		parentEventId: parentEvent.id,
		recurrenceId: recurrenceId.recurrenceId,
	};
	if (recurrenceId.recurrenceRange) override.recurrenceRange = recurrenceId.recurrenceRange;
	if (child.title !== parentEvent.title) override.title = child.title;
	if (child.start !== parentEvent.start) override.start = child.start;
	if (child.end !== parentEvent.end) override.end = child.end;
	if (child.description !== parentEvent.description) override.description = child.description;
	if (child.location !== parentEvent.location) override.location = child.location;
	if (child.url !== parentEvent.url) override.url = child.url;
	if (child.color !== parentEvent.color) override.color = child.color;
	if (child.status !== parentEvent.status) override.status = child.status;
	if (child.transparency !== parentEvent.transparency) override.transparency = child.transparency;
	if (child.visibility !== parentEvent.visibility) override.visibility = child.visibility;
	if (child.extendedProperties) override.extendedProperties = child.extendedProperties;
	return override;
}

function buildCancelledOverride(
	parentEvent: CalendarEvent,
	recurrenceId: ParsedRecurrenceIdInfo,
): EventOverride {
	const override: EventOverride = {
		id: crypto.randomUUID(),
		parentEventId: parentEvent.id,
		recurrenceId: recurrenceId.recurrenceId,
		status: "cancelled",
	};
	if (recurrenceId.recurrenceRange) override.recurrenceRange = recurrenceId.recurrenceRange;
	return override;
}

function statusFromComponent(component: ICAL.Component): EventStatus | undefined {
	const status = component.getFirstPropertyValue("status") as string | null;
	return status ? STATUS_MAP[status.toUpperCase()] : undefined;
}

/**
 * Parse the text of a `.ics` file into a list of `CalendarEvent` rows ready
 * for upsert by `source_uid`. Master events have any matching `RECURRENCE-ID`
 * components attached as overrides. Warnings collect lossy or skipped fields.
 */
export function parseIcs(text: string, calendarId = "local"): IcsParseResult {
	const warnings: string[] = [];
	const deviceZone = getDeviceZone();
	const budgetError = validateIcsTextBudget(text);
	if (budgetError) return { events: [], warnings: [budgetError] };

	let jcal: unknown;
	try {
		jcal = ICAL.parse(text);
	} catch (err) {
		const msg = err instanceof Error ? err.message : String(err);
		return { events: [], warnings: [`Failed to parse .ics file: ${msg}`] };
	}

	const rootJcal = isJcalComponent(jcal) ? jcal : undefined;
	const root = new ICAL.Component(jcal as [string, unknown[], unknown[]]);
	const vcalendars = root.name === "vcalendar" ? [root] : root.getAllSubcomponents("vcalendar");
	if (vcalendars.length === 0) {
		return { events: [], warnings: ["No VCALENDAR component found in .ics file."] };
	}
	const vcalendarJcals = rootJcal
		? rootJcal[0].toLowerCase() === "vcalendar"
			? [rootJcal]
			: jcalSubcomponents(rootJcal).filter((component) => component[0].toLowerCase() === "vcalendar")
		: [];

	const masters = new Map<string, ParsedMasterEvent>();
	const overrides: {
		uid: string;
		child: CalendarEvent | null;
		recurrenceId: ParsedRecurrenceIdInfo | RawRecurrenceIdInfo;
	}[] = [];

	for (const vcal of vcalendars) {
		for (const vevent of vcal.getAllSubcomponents("vevent")) {
			const parsed = calendarEventBaseFromComponent(vevent, calendarId, deviceZone, warnings);
			const uid = parsed?.event.sourceUid
				?? (vevent.getFirstPropertyValue("uid") as string | null)
				?? undefined;
			if (!uid) {
				warnings.push("VEVENT missing UID; skipped.");
				continue;
			}
			if (parsed?.recurrenceId) {
				overrides.push({ uid, child: parsed.event, recurrenceId: parsed.recurrenceId });
			} else if (!parsed) {
				const rawRecurrenceId = rawRecurrenceIdInfo(vevent, warnings);
				if (rawRecurrenceId && statusFromComponent(vevent) === "cancelled") {
					overrides.push({ uid, child: null, recurrenceId: rawRecurrenceId });
				}
			} else if (!masters.has(uid)) {
				masters.set(uid, {
					event: parsed.event,
					sequence: parsed.event.sequence ?? 0,
					revisionKey: componentRevisionKey(vevent),
				});
			} else {
				const existing = masters.get(uid)!;
				const candidate = {
					event: parsed.event,
					sequence: parsed.event.sequence ?? 0,
					revisionKey: componentRevisionKey(vevent),
				};
				if (shouldReplaceMasterEvent(existing, candidate)) {
					masters.set(uid, candidate);
					warnings.push(`Duplicate UID ${uid}; kept the newest master VEVENT revision.`);
				} else {
					warnings.push(`Duplicate UID ${uid}; kept the newest master VEVENT revision.`);
				}
			}
		}
	}

	for (const { uid, child, recurrenceId } of overrides) {
		const parent = masters.get(uid)?.event;
		if (!parent) {
			warnings.push(`Override for UID ${uid} has no matching master VEVENT; skipped.`);
			continue;
		}
		const resolvedRecurrenceId = "recurrenceId" in recurrenceId
			? recurrenceId
			: materializeRecurrenceId(recurrenceId, deviceZone, parent.timezone);
		parent.overrides = parent.overrides ?? [];
		if (child) {
			parent.overrides.push(buildOverride(parent, child, resolvedRecurrenceId));
		} else {
			parent.overrides.push(buildCancelledOverride(parent, resolvedRecurrenceId));
		}
	}

	return {
		events: Array.from(masters.values()).map((master) => master.event),
		warnings,
		preservation: buildPreservationPayload(text, vcalendarJcals, warnings),
	};
}
