import {
  CHAT_ALL_WEEKDAYS_MASK,
  CHAT_LAST_MINUTE_OF_DAY,
  CHAT_MAX_QUIET_WINDOWS,
  CHAT_QUIET_POLICIES,
  CHAT_QUIET_REASONS,
  CHAT_RECURRENCE_MAX_INTERVAL_MINUTES,
  CHAT_RECURRENCE_MIN_INTERVAL_MINUTES,
  CHAT_SCHEDULE_PROPOSAL_STATES,
  type ChatQuietPeriodPreferences,
  type ChatQuietStateRead,
  type ChatQuietWindow,
  type ChatRecurrenceRule,
  type ChatRecurrenceSpec,
  type ChatScheduledRecurrenceRead,
  type ChatScheduleProposal,
  type ChatScheduleProposalStateRead,
} from "../contracts";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
  readUtcTimestamp,
} from "./readers";

/** Longest proposed message an agent may post, in characters. */
const MAX_PROPOSED_MESSAGE_CHARS = 4_000;
/** Longest reason a schedule proposal gives, in characters. */
const MAX_PROPOSAL_REASON_CHARS = 500;

/** Longest IANA zone name a repeat rule carries, in bytes. */
const MAX_TIME_ZONE_BYTES = 64;

function readBoundedInteger(value: unknown, minimum: number, maximum: number, label: string): number {
  const integer = readNonNegativeSafeInteger(value, label);
  if (integer < minimum || integer > maximum) {
    throw new Error(`${label} must be between ${minimum} and ${maximum}`);
  }
  return integer;
}

function readWeekdayMask(value: unknown, label: string): number {
  return readBoundedInteger(value, 1, CHAT_ALL_WEEKDAYS_MASK, label);
}

function readLocalMinute(value: unknown, label: string): number {
  return readBoundedInteger(value, 0, CHAT_LAST_MINUTE_OF_DAY, label);
}

function readTimeZone(value: unknown, label: string): string {
  const zone = readString(value, label);
  if (!zone.trim() || new TextEncoder().encode(zone).length > MAX_TIME_ZONE_BYTES) {
    throw new Error(`${label} must be an IANA zone name`);
  }
  return zone;
}

/** Parses a repeat rule, refusing intervals and minutes outside the backend bounds and empty weekday masks. */
export function parseChatRecurrenceRule(value: unknown, label = "Chat recurrence rule"): ChatRecurrenceRule {
  const record = readRecord(value, label);
  const kind = readEnum(record.kind, ["interval", "fixed_time"] as const, `${label}.kind`);
  if (kind === "interval") {
    return {
      kind,
      minutes: readBoundedInteger(
        record.minutes,
        CHAT_RECURRENCE_MIN_INTERVAL_MINUTES,
        CHAT_RECURRENCE_MAX_INTERVAL_MINUTES,
        `${label}.minutes`,
      ),
    };
  }
  return {
    kind,
    localMinute: readLocalMinute(record.localMinute, `${label}.localMinute`),
    weekdays: readWeekdayMask(record.weekdays, `${label}.weekdays`),
  };
}

/** Parses a repeat rule with its zone and quiet period policy. */
export function parseChatRecurrenceSpec(value: unknown, label = "Chat recurrence"): ChatRecurrenceSpec {
  const record = readRecord(value, label);
  return {
    rule: parseChatRecurrenceRule(record.rule, `${label}.rule`),
    timeZone: readTimeZone(record.timeZone, `${label}.timeZone`),
    quietPolicy: readEnum(record.quietPolicy, CHAT_QUIET_POLICIES, `${label}.quietPolicy`),
  };
}

/** Parses the repeat rule and series progress of a scheduled message. */
export function parseChatScheduledRecurrence(
  value: unknown,
  label = "Chat scheduled recurrence",
): ChatScheduledRecurrenceRead {
  const record = readRecord(value, label);
  return {
    ...parseChatRecurrenceSpec(record, label),
    nextOccurrenceAt: readUtcTimestamp(record.nextOccurrenceAt, `${label}.nextOccurrenceAt`),
    firedCount: readNonNegativeSafeInteger(record.firedCount, `${label}.firedCount`),
    skippedOccurrences: readNonNegativeSafeInteger(record.skippedOccurrences, `${label}.skippedOccurrences`),
    deferredUntil: readNullable(record.deferredUntil, `${label}.deferredUntil`, readUtcTimestamp),
  };
}

/** Parses one quiet window; its start and end must differ. */
export function parseChatQuietWindow(value: unknown, label = "Chat quiet window"): ChatQuietWindow {
  const record = readRecord(value, label);
  const startMinute = readLocalMinute(record.startMinute, `${label}.startMinute`);
  const endMinute = readLocalMinute(record.endMinute, `${label}.endMinute`);
  if (startMinute === endMinute) throw new Error(`${label} must end at another minute than it starts`);
  return { weekdays: readWeekdayMask(record.weekdays, `${label}.weekdays`), startMinute, endMinute };
}

/** Parses this device's quiet windows. */
export function parseChatQuietPeriodPreferences(
  value: unknown,
  label = "Chat quiet periods",
): ChatQuietPeriodPreferences {
  const record = readRecord(value, label);
  const windows = readArray(record.windows, `${label}.windows`, parseChatQuietWindow);
  if (windows.length > CHAT_MAX_QUIET_WINDOWS) {
    throw new Error(`${label}.windows holds at most ${CHAT_MAX_QUIET_WINDOWS} windows`);
  }
  return { windows };
}

/** Parses whether this device is quiet now; a quiet state names its reason and an open one has none. */
export function parseChatQuietState(value: unknown, label = "Chat quiet state"): ChatQuietStateRead {
  const record = readRecord(value, label);
  const quiet = readBoolean(record.quiet, `${label}.quiet`);
  const reason = readNullable(record.reason, `${label}.reason`, (entry, path) => readEnum(entry, CHAT_QUIET_REASONS, path));
  const endsAt = readNullable(record.endsAt, `${label}.endsAt`, readUtcTimestamp);
  if (quiet !== (reason !== null) || (!quiet && endsAt !== null)) {
    throw new Error(`${label} must name a reason exactly when it is quiet`);
  }
  return { quiet, reason, endsAt };
}

/** Parses the device states of the proposals one read named. */
export function parseChatScheduleProposalStates(
  value: unknown,
  label = "Chat schedule proposal states",
): ChatScheduleProposalStateRead[] {
  return readArray(value, label, (entry, path) => {
    const record = readRecord(entry, path);
    return {
      proposalItemId: readIdentifier(record.proposalItemId, `${path}.proposalItemId`),
      state: readEnum(record.state, CHAT_SCHEDULE_PROPOSAL_STATES, `${path}.state`),
    };
  });
}

function readBoundedText(value: unknown, maximum: number, label: string, allowEmpty: boolean): string {
  const text = readString(value, label).trim();
  if (!allowEmpty && !text) throw new Error(`${label} must not be empty`);
  if ([...text].length > maximum) throw new Error(`${label} holds at most ${maximum} characters`);
  return text;
}

/** Parses the payload of an agent's schedule proposal update. */
export function parseChatScheduleProposal(value: unknown, label = "Chat schedule proposal"): ChatScheduleProposal {
  const record = readRecord(value, label);
  return {
    message: readBoundedText(record.message, MAX_PROPOSED_MESSAGE_CHARS, `${label}.message`, false),
    firstAt: readUtcTimestamp(record.firstAt, `${label}.firstAt`),
    recurrence: readNullable(record.recurrence, `${label}.recurrence`, parseChatRecurrenceSpec),
    reason: readBoundedText(record.reason, MAX_PROPOSAL_REASON_CHARS, `${label}.reason`, true),
  };
}
