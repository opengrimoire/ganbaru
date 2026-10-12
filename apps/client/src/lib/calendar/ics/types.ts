import type { CalendarEvent } from "$lib/calendar/types";

/**
 * Result of parsing a `.ics` file: events ready for upsert plus warnings about
 * fields that were lossy or skipped (unknown TZIDs, dropped alarm REPEAT, etc.).
 */
export interface IcsParseResult {
	events: CalendarEvent[];
	warnings: string[];
	preservation?: IcsPreservationPayload;
}

export type IcsPreservationStatus =
	| "lossless"
	| "partial"
	| "unsupported"
	| "needs-review"
	| "regenerated"
	| "invalid";

export interface IcsPreservationPayload {
	sourceFingerprint: string;
	objects: IcsPreservedObject[];
}

export interface IcsPreservedObject {
	prodid?: string;
	version?: string;
	method?: string;
	calendarScale?: string;
	rawJcal: unknown;
	diagnostics: string[];
	components: IcsPreservedComponent[];
}

export interface IcsPreservedComponent {
	componentType: string;
	uid?: string;
	recurrenceId?: string;
	recurrenceIdValueType?: string;
	sequence?: number;
	dtstartKey?: string;
	rawJcal: unknown;
	preservationStatus: IcsPreservationStatus;
	projectionWarnings: string[];
	components: IcsPreservedComponent[];
}

/**
 * Counters returned to the UI after a bulk import. Drives the toast message
 * `Imported N new, updated M, skipped K older revisions`.
 */
export interface IcsImportSummary {
	added: number;
	updated: number;
	skippedOlder: number;
	warnings: string[];
}
