import type { ProviderSessionState, RateLimitStatusEvent, ThreadUsageUpdatedEvent } from "$lib/chat/contracts";
import {
  composerRateLimitWindows,
  contextMeter,
  type ComposerRateLimitWindow,
  type ContextMeterRead,
} from "./model";

/** The context usage and provider limits one composer shows. */
export interface ComposerMeterRead {
  /** Context window usage, or `null` when the provider reported no usable context count. */
  meter: ContextMeterRead | null;
  /** Usage windows of the provider's rate limits, empty when none were reported. */
  windows: ComposerRateLimitWindow[];
}

/** The usage fields a composer meter reads. */
export type ComposerUsage = Pick<ThreadUsageUpdatedEvent, "contextTokens" | "contextLimit">;

/**
 * Reads the meter and limits a composer shows from the latest provider usage and rate limit
 * status of its session, so every composer renders one shape.
 *
 * @param usage The latest context usage of the session, or `null` when none was reported.
 * @param rateLimit The latest rate limit status of the session, or `null` when none was reported.
 * @returns The context meter, `null` without a valid context count, and the limit windows.
 */
export function composerMeterRead(
  usage: ComposerUsage | null,
  rateLimit: RateLimitStatusEvent | null,
): ComposerMeterRead {
  return {
    meter: contextMeter(usage?.contextTokens ?? null, usage?.contextLimit ?? null),
    windows: composerRateLimitWindows(rateLimit),
  };
}

/** What the composer status panel shows about the session a composer talks to. */
export interface ComposerStatusRead {
  /** Label of the provider instance, or `null` when it is unknown. */
  providerLabel: string | null;
  sessionId: string | null;
  sessionState: ProviderSessionState;
  /** Display name of the chosen model, or `null` when the provider manages the model. */
  modelLabel: string | null;
  accountLabel: string | null;
  planLabel: string | null;
  usage: ComposerMeterRead;
}
