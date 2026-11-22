import { describe, expect, it } from "vitest";
import {
  NO_PROVIDER_TEST,
  currentProviderTest,
  providerTestSucceeded,
  transitionProviderTest,
  type ProviderTestState,
} from "./provider-test-state";

interface Result { label: string }

const TESTED = "{\"executable\":\"codex\"}";
const EDITED = "{\"executable\":\"codex-nightly\"}";

function started(state: ProviderTestState<Result>, configKey: string): ProviderTestState<Result> {
  return transitionProviderTest(state, { type: "started", configKey });
}

describe("transitionProviderTest", () => {
  it("shows a finished result while the draft still matches the tested configuration", () => {
    const testing = started(NO_PROVIDER_TEST, TESTED);
    expect(testing).toEqual({ phase: "testing", configKey: TESTED });
    const done = transitionProviderTest(testing, {
      type: "finished", configKey: TESTED, result: { label: "ok" }, succeeded: true,
    });
    expect(currentProviderTest(done, TESTED)).toEqual({ result: { label: "ok" }, succeeded: true });
  });

  it("hides the result once the draft is edited after the test", () => {
    const done = transitionProviderTest(started(NO_PROVIDER_TEST, TESTED), {
      type: "finished", configKey: TESTED, result: { label: "ok" }, succeeded: true,
    });
    expect(currentProviderTest(done, EDITED)).toBeNull();
    expect(currentProviderTest(done, TESTED)).not.toBeNull();
  });

  it("hides a result that finishes after the draft was edited during the test", () => {
    const done = transitionProviderTest(started(NO_PROVIDER_TEST, TESTED), {
      type: "finished", configKey: TESTED, result: { label: "ok" }, succeeded: false,
    });
    expect(currentProviderTest(done, EDITED)).toBeNull();
  });

  it("drops the result of a test that is no longer in flight", () => {
    const testing = started(NO_PROVIDER_TEST, TESTED);
    const ended = transitionProviderTest(testing, { type: "ended", configKey: TESTED });
    expect(ended).toEqual(NO_PROVIDER_TEST);
    expect(transitionProviderTest(ended, {
      type: "finished", configKey: TESTED, result: { label: "late" }, succeeded: true,
    })).toBe(ended);
  });

  it("ignores the end of a test other than the one in flight", () => {
    const testing = started(NO_PROVIDER_TEST, EDITED);
    expect(transitionProviderTest(testing, { type: "ended", configKey: TESTED })).toBe(testing);
    expect(transitionProviderTest(testing, {
      type: "finished", configKey: TESTED, result: { label: "other" }, succeeded: true,
    })).toBe(testing);
  });

  it("clears a shown result when a new test starts or the family is reset", () => {
    const done = transitionProviderTest(started(NO_PROVIDER_TEST, TESTED), {
      type: "finished", configKey: TESTED, result: { label: "ok" }, succeeded: true,
    });
    expect(currentProviderTest(started(done, TESTED), TESTED)).toBeNull();
    expect(transitionProviderTest(done, { type: "reset" })).toEqual(NO_PROVIDER_TEST);
    expect(transitionProviderTest(NO_PROVIDER_TEST, { type: "reset" })).toBe(NO_PROVIDER_TEST);
  });

  it("shows no result while testing or before any test", () => {
    expect(currentProviderTest(NO_PROVIDER_TEST, TESTED)).toBeNull();
    expect(currentProviderTest(started(NO_PROVIDER_TEST, TESTED), TESTED)).toBeNull();
  });
});

describe("providerTestSucceeded", () => {
  it("passes a reachable provider whether or not it still needs sign-in", () => {
    expect(providerTestSucceeded("healthy")).toBe(true);
    expect(providerTestSucceeded("authentication_required")).toBe(true);
    expect(providerTestSucceeded("executable_missing")).toBe(false);
    expect(providerTestSucceeded("transport_unavailable")).toBe(false);
  });
});
