import type { ProbeState } from "$lib/chat/contracts";

/**
 * The setup test of a provider draft. Each variant past `none` carries the key of the configuration it tested, so a
 * result is only shown while the draft still matches what was probed.
 */
export type ProviderTestState<Result> =
  | { phase: "none" }
  | { phase: "testing"; configKey: string }
  | { phase: "done"; configKey: string; result: Result; succeeded: boolean };

export type ProviderTestEvent<Result> =
  /** A test of the configuration with `configKey` started; it replaces any shown result. */
  | { type: "started"; configKey: string }
  /** The test of `configKey` returned a result. */
  | { type: "finished"; configKey: string; result: Result; succeeded: boolean }
  /** The test of `configKey` failed or returned nothing to show. */
  | { type: "ended"; configKey: string }
  /** The draft was replaced by another provider family. */
  | { type: "reset" };

export const NO_PROVIDER_TEST: ProviderTestState<never> = { phase: "none" };

/**
 * Apply one test event. Results of a test that is not in flight return the same state object, so callers can tell an
 * ignored move by identity.
 */
export function transitionProviderTest<Result>(
  state: ProviderTestState<Result>, event: ProviderTestEvent<Result>,
): ProviderTestState<Result> {
  switch (event.type) {
    case "started":
      return { phase: "testing", configKey: event.configKey };
    case "finished":
      if (state.phase !== "testing" || state.configKey !== event.configKey) return state;
      return { phase: "done", configKey: event.configKey, result: event.result, succeeded: event.succeeded };
    case "ended":
      return state.phase === "testing" && state.configKey === event.configKey ? NO_PROVIDER_TEST : state;
    case "reset":
      return state.phase === "none" ? state : NO_PROVIDER_TEST;
  }
}

/** The finished result when it belongs to the configuration with `configKey`, otherwise `null`. */
export function currentProviderTest<Result>(
  state: ProviderTestState<Result>, configKey: string,
): { result: Result; succeeded: boolean } | null {
  if (state.phase !== "done" || state.configKey !== configKey) return null;
  return { result: state.result, succeeded: state.succeeded };
}

/** Whether a probe reached the provider, including one that still needs the user to sign in. */
export function providerTestSucceeded(state: ProbeState): boolean {
  return state === "healthy" || state === "authentication_required";
}
