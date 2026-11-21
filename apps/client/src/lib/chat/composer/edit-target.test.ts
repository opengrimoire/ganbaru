import { describe, expect, it } from "vitest";
import type { ChatAssignmentInputRead, ChatScheduledMessageRead } from "$lib/chat/contracts";
import {
  DRAFT_EDIT_TARGET,
  composerSubmitPlan,
  editedReply,
  editedScheduled,
  transitionComposerEditTarget,
  type ComposerEditEvent,
} from "./edit-target";

const input = { inputId: "input:1" } as unknown as ChatAssignmentInputRead;
const message = { id: "scheduled:1" } as unknown as ChatScheduledMessageRead;
const replyStarted: ComposerEditEvent = { type: "replyEditStarted", input };
const scheduledStarted: ComposerEditEvent = { type: "scheduledEditStarted", message, draftAlsoSendToChannel: true };

const replyEdit = transitionComposerEditTarget(DRAFT_EDIT_TARGET, replyStarted);
const scheduledEdit = transitionComposerEditTarget(DRAFT_EDIT_TARGET, scheduledStarted);

describe("transitionComposerEditTarget", () => {
  it("starts a waiting reply edit from the draft", () => {
    expect(replyEdit).toEqual({ phase: "waitingReply", input });
    expect(editedReply(replyEdit)).toBe(input);
    expect(editedScheduled(replyEdit)).toBeNull();
  });

  it("starts a scheduled message edit from the draft and keeps the draft's share choice", () => {
    expect(scheduledEdit).toEqual({ phase: "scheduled", message, draftAlsoSendToChannel: true });
    expect(editedScheduled(scheduledEdit)?.draftAlsoSendToChannel).toBe(true);
    expect(editedReply(scheduledEdit)).toBeNull();
  });

  it("never holds both edits: a second start while one runs is ignored", () => {
    expect(transitionComposerEditTarget(replyEdit, scheduledStarted)).toBe(replyEdit);
    expect(transitionComposerEditTarget(replyEdit, replyStarted)).toBe(replyEdit);
    expect(transitionComposerEditTarget(scheduledEdit, replyStarted)).toBe(scheduledEdit);
    expect(transitionComposerEditTarget(scheduledEdit, scheduledStarted)).toBe(scheduledEdit);
  });

  it("returns to the draft when either edit ends", () => {
    expect(transitionComposerEditTarget(replyEdit, { type: "editEnded" })).toEqual(DRAFT_EDIT_TARGET);
    expect(transitionComposerEditTarget(scheduledEdit, { type: "editEnded" })).toEqual(DRAFT_EDIT_TARGET);
  });

  it("ignores an end without an edit", () => {
    expect(transitionComposerEditTarget(DRAFT_EDIT_TARGET, { type: "editEnded" })).toBe(DRAFT_EDIT_TARGET);
    expect(editedReply(DRAFT_EDIT_TARGET)).toBeNull();
    expect(editedScheduled(DRAFT_EDIT_TARGET)).toBeNull();
  });
});

describe("composerSubmitPlan", () => {
  it("sends the draft after the assignment preview settles", () => {
    expect(composerSubmitPlan(DRAFT_EDIT_TARGET)).toEqual({ action: "send", awaitsPreview: true });
  });

  it("saves a waiting reply's text without waiting for the preview", () => {
    expect(composerSubmitPlan(replyEdit)).toEqual({ action: "saveReply", awaitsPreview: false });
  });

  it("saves a scheduled message only after the preview settles, since it checks the same agent blockers as a send", () => {
    expect(composerSubmitPlan(scheduledEdit)).toEqual({ action: "saveScheduled", awaitsPreview: true });
  });
});
