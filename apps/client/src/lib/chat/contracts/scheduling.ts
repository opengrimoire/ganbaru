import type { ChatConversationItemId, UtcTimestamp } from "./common";

/** Shortest interval between occurrences of an interval repeat rule, in minutes. */
export const CHAT_RECURRENCE_MIN_INTERVAL_MINUTES = 60;
/** Longest interval between occurrences of an interval repeat rule (30 days), in minutes. */
export const CHAT_RECURRENCE_MAX_INTERVAL_MINUTES = 43_200;
/** Last minute of a local day a fixed time rule or quiet window may name. */
export const CHAT_LAST_MINUTE_OF_DAY = 1_439;
/** Weekday mask naming every day: bit 0 is Monday through bit 6 Sunday. */
export const CHAT_ALL_WEEKDAYS_MASK = 0b111_1111;
/** Most quiet windows one device keeps. */
export const CHAT_MAX_QUIET_WINDOWS = 8;

/** How a repeating message behaves during a quiet period: wait until it ends, or post anyway. */
export const CHAT_QUIET_POLICIES = ["defer", "ignore"] as const;
export type ChatQuietPolicy = (typeof CHAT_QUIET_POLICIES)[number];

/** When a repeating message posts: every `minutes` from its anchor, or at one local minute on the masked weekdays. */
export type ChatRecurrenceRule =
  | { kind: "interval"; minutes: number }
  | { kind: "fixed_time"; localMinute: number; weekdays: number };

/** A repeat rule with the IANA zone its local times use and its quiet period policy. */
export interface ChatRecurrenceSpec {
  rule: ChatRecurrenceRule;
  timeZone: string;
  quietPolicy: ChatQuietPolicy;
}

/** The repeat rule of a scheduled message and how far its series has come. */
export interface ChatScheduledRecurrenceRead extends ChatRecurrenceSpec {
  nextOccurrenceAt: UtcTimestamp;
  firedCount: number;
  /** Occurrences this device skipped because they were missed or quiet; device-local. */
  skippedOccurrences: number;
  /** End of the quiet period the pending occurrence waits for on this device. */
  deferredUntil: UtcTimestamp | null;
}

/** A weekly local window during which a device is quiet; the end is exclusive and crosses midnight when it is before the start. */
export interface ChatQuietWindow {
  weekdays: number;
  startMinute: number;
  endMinute: number;
}

/** The quiet windows of one device. */
export interface ChatQuietPeriodPreferences {
  windows: ChatQuietWindow[];
}

export const CHAT_QUIET_REASONS = ["window", "focus"] as const;
export type ChatQuietReason = (typeof CHAT_QUIET_REASONS)[number];

/** Whether this device is quiet now, why, and when that ends when the end is known. */
export interface ChatQuietStateRead {
  quiet: boolean;
  reason: ChatQuietReason | null;
  endsAt: UtcTimestamp | null;
}

/** A schedule an agent proposed in its reply thread; nothing is scheduled until a person confirms it. */
export interface ChatScheduleProposal {
  /** Message text without the agent mention a confirmation adds. */
  message: string;
  firstAt: UtcTimestamp;
  recurrence: ChatRecurrenceSpec | null;
  reason: string;
}

export const CHAT_SCHEDULE_PROPOSAL_STATES = ["pending", "confirmed", "dismissed"] as const;
export type ChatScheduleProposalState = (typeof CHAT_SCHEDULE_PROPOSAL_STATES)[number];

/** Where one schedule proposal stands on this device; dismissals are device-local. */
export interface ChatScheduleProposalStateRead {
  proposalItemId: ChatConversationItemId;
  state: ChatScheduleProposalState;
}

/** A person's confirmation of a schedule proposal with the time, rule, and text they chose. */
export interface ConfirmScheduleProposalRequest {
  proposalItemId: ChatConversationItemId;
  scheduledFor: UtcTimestamp;
  recurrence: ChatRecurrenceSpec | null;
  /** Edited text without the agent mention; `null` keeps the proposed text. */
  normalizedMarkdown: string | null;
}
