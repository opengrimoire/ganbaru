import { describe, expect, it } from "vitest";
import {
  IDLE_HANDOFF_ACTIVITY,
  handoffBusy,
  initialHandoffActivity,
  transitionHandoffActivity,
  type HandoffActivity,
} from "./handoff-flow";

function inviting(state: HandoffActivity, request: number): HandoffActivity {
  return transitionHandoffActivity(state, { type: "inviteStarted", request });
}

function running(state: HandoffActivity, kind: "status" | "unlink" | "network", request: number): HandoffActivity {
  return transitionHandoffActivity(state, { type: "actionStarted", kind, request });
}

describe("transitionHandoffActivity", () => {
  it("keeps an unlink busy when the invitation created beside it finishes first", () => {
    const both = running(inviting(IDLE_HANDOFF_ACTIVITY, 1), "unlink", 2);
    expect(handoffBusy(both)).toBe("unlink");
    const inviteDone = transitionHandoffActivity(both, { type: "inviteFinished", request: 1 });
    expect(handoffBusy(inviteDone)).toBe("unlink");
    const idle = transitionHandoffActivity(inviteDone, { type: "actionFinished", request: 2 });
    expect(handoffBusy(idle)).toBeNull();
  });

  it("reports the invitation as busy once the action beside it finishes", () => {
    const both = running(inviting(IDLE_HANDOFF_ACTIVITY, 1), "network", 2);
    const actionDone = transitionHandoffActivity(both, { type: "actionFinished", request: 2 });
    expect(handoffBusy(actionDone)).toBe("invite");
  });

  it("ignores the end of an action that a later one replaced", () => {
    const first = running(IDLE_HANDOFF_ACTIVITY, "network", 1);
    const second = running(first, "unlink", 2);
    expect(transitionHandoffActivity(second, { type: "actionFinished", request: 1 })).toBe(second);
    expect(handoffBusy(second)).toBe("unlink");
  });

  it("ignores the end of an invitation that a later one replaced", () => {
    const second = inviting(inviting(IDLE_HANDOFF_ACTIVITY, 1), 2);
    expect(transitionHandoffActivity(second, { type: "inviteFinished", request: 1 })).toBe(second);
    expect(handoffBusy(transitionHandoffActivity(second, { type: "inviteFinished", request: 2 }))).toBeNull();
  });

  it("ignores ends when nothing runs", () => {
    expect(transitionHandoffActivity(IDLE_HANDOFF_ACTIVITY, { type: "actionFinished", request: 1 }))
      .toBe(IDLE_HANDOFF_ACTIVITY);
    expect(transitionHandoffActivity(IDLE_HANDOFF_ACTIVITY, { type: "inviteFinished", request: 1 }))
      .toBe(IDLE_HANDOFF_ACTIVITY);
  });

  it("starts busy reading status when no status is cached, until the first read replaces the placeholder", () => {
    expect(handoffBusy(initialHandoffActivity(true))).toBeNull();
    const placeholder = initialHandoffActivity(false);
    expect(handoffBusy(placeholder)).toBe("status");
    const reading = running(placeholder, "status", 1);
    expect(handoffBusy(transitionHandoffActivity(reading, { type: "actionFinished", request: 1 }))).toBeNull();
  });
});
