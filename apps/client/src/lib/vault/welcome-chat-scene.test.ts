import { describe, expect, it } from "vitest";
import { welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import {
  CHAT_BEATS,
  CHAT_CURSOR,
  CHAT_SCENE_AGENTS,
  CHAT_SCENE_MENTIONED,
  CHAT_SCENE_QUERY,
  CHAT_SCENE_STEPS,
  CHAT_STILL,
  CHAT_THREAD_OPEN_MS,
  chatActiveSince,
  chatComposerCaretOn,
  chatComposerDraft,
  chatComposerFocused,
  chatMentioned,
  chatMessageIn,
  chatPickerNarrowed,
  chatPickerOpen,
  chatPickerShown,
  chatReplyIn,
  chatReplyStripShown,
  chatSent,
  chatShimmerPosition,
  chatStepState,
  chatStepsFolded,
  chatStreamedReply,
  chatThinkingShown,
  chatThreadShown,
  chatTouches,
  chatWorking,
} from "./welcome-chat-scene";

const B = CHAT_BEATS;
const QUESTION = "read chapter 2 and tell me what Mateo knows about Clara.";
const REPLY = "In chapter 2, Mateo sees Clara leave the station with the letter.";

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome chat scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const elapsed of moments()) {
      const values = [
        chatPickerShown(elapsed),
        chatMessageIn(elapsed),
        chatThreadShown(elapsed),
        chatReplyIn(elapsed),
        chatStepsFolded(elapsed),
        ...Object.values(chatTouches(elapsed)),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("narrows the picker to the mentioned agent alone with its query", () => {
    const matches = CHAT_SCENE_AGENTS.filter((agent) => agent.name.toLowerCase().startsWith(CHAT_SCENE_QUERY));
    expect(matches).toEqual([CHAT_SCENE_MENTIONED]);
    expect(CHAT_SCENE_QUERY.length).toBeGreaterThan(0);
  });

  it("opens the picker with the at sign typed into the clicked composer, and narrows it as the query is typed", () => {
    expect(chatComposerFocused(B.editorTap - 1)).toBe(false);
    expect(chatComposerFocused(B.editorTap)).toBe(true);
    expect(chatPickerOpen(B.mentionTyped - 1)).toBe(false);
    expect(chatComposerDraft(B.mentionTyped - 1, QUESTION).query).toBe("");
    expect(chatPickerOpen(B.mentionTyped)).toBe(true);
    expect(chatComposerDraft(B.mentionTyped, QUESTION).query).toBe("@");
    expect(chatPickerNarrowed(B.queryTypeStart - 1)).toBe(false);
    expect(chatComposerDraft(B.queryTypeEnd, QUESTION).query).toBe(`@${CHAT_SCENE_QUERY}`);
    expect(chatPickerNarrowed(B.queryTypeEnd)).toBe(true);
    expect(chatPickerShown(B.pick - 1)).toBe(1);
  });

  it("turns the query into a mention as the agent is accepted, closing the picker", () => {
    expect(chatMentioned(B.pick - 1)).toBe(false);
    expect(chatMentioned(B.pick)).toBe(true);
    expect(chatPickerOpen(B.pick)).toBe(false);
    expect(chatComposerDraft(B.pick, QUESTION)).toEqual({ query: "", mention: true, text: "" });
    expect(chatPickerShown(B.typeStart)).toBe(0);
  });

  it("types the question after the mention, steady caret while typing, and clears the composer as it sends", () => {
    expect(chatComposerDraft((B.typeStart + B.typeEnd) / 2, QUESTION).text.length).toBeGreaterThan(0);
    expect(chatComposerCaretOn((B.typeStart + B.typeEnd) / 2)).toBe(true);
    expect(chatComposerDraft(B.typeEnd, QUESTION)).toEqual({ query: "", mention: true, text: QUESTION });
    expect(chatSent(B.sendTap - 1)).toBe(false);
    expect(chatSent(B.sendTap)).toBe(true);
    expect(chatComposerDraft(B.sendTap, QUESTION)).toEqual({ query: "", mention: false, text: "" });
    expect(chatComposerCaretOn(B.sendTap)).toBe(false);
  });

  it("posts the message, then opens its thread, where the reply starts and the channel shows its reply strip", () => {
    expect(chatMessageIn(B.sendTap - 1)).toBe(0);
    expect(chatThreadShown(B.threadOpen)).toBe(0);
    expect(chatThreadShown(B.threadOpen + CHAT_THREAD_OPEN_MS)).toBe(1);
    expect(chatThreadShown(B.replyStart)).toBe(1);
    expect(chatReplyIn(B.replyStart - 1)).toBe(0);
    expect(chatReplyStripShown(B.replyStart - 1)).toBe(false);
    expect(chatReplyStripShown(B.replyStart)).toBe(true);
    expect(chatWorking(B.sendTap)).toBe(true);
    expect(chatWorking(B.streamEnd)).toBe(false);
  });

  it("thinks, then takes its steps in order, each active until the next starts, and folds them all", () => {
    expect(chatThinkingShown(B.replyStart)).toBe(true);
    expect(chatActiveSince(B.replyStart + 1)).toBe(B.replyStart);
    expect(chatThinkingShown(B.readStart)).toBe(false);
    expect(chatStepState("read", B.readStart - 1)).toBe("hidden");
    expect(chatStepState("read", B.readStart)).toBe("active");
    expect(chatActiveSince(B.readStart + 1)).toBe(B.readStart);
    expect(chatStepState("read", B.searchStart)).toBe("done");
    expect(chatStepState("search", B.searchStart)).toBe("active");
    expect(chatActiveSince(B.searchStart + 1)).toBe(B.searchStart);
    for (const key of CHAT_SCENE_STEPS) expect(chatStepState(key, B.fold)).toBe("done");
    expect(chatStepsFolded(B.fold)).toBe(0);
    expect(chatStepsFolded(B.streamEnd)).toBe(1);
  });

  it("never thinks and shows an active step at once", () => {
    for (const elapsed of moments()) {
      const active = CHAT_SCENE_STEPS.filter((key) => chatStepState(key, elapsed) === "active");
      expect(active.length + (chatThinkingShown(elapsed) ? 1 : 0)).toBeLessThanOrEqual(1);
    }
  });

  it("sweeps the shimmer from 100% toward 0%", () => {
    for (const elapsed of moments()) {
      const position = chatShimmerPosition(elapsed, B.replyStart);
      expect(position).toBeGreaterThanOrEqual(0);
      expect(position).toBeLessThanOrEqual(100);
    }
  });

  it("streams the reply in whole words from the fold, in full once the turn ends", () => {
    expect(chatStreamedReply(B.fold, REPLY)).toBe("");
    const halfway = chatStreamedReply((B.fold + B.streamEnd) / 2, REPLY);
    expect(halfway.length).toBeGreaterThan(0);
    expect(REPLY.startsWith(halfway)).toBe(true);
    expect(REPLY.split(" ")).toContain(halfway.split(" ").at(-1));
    expect(chatStreamedReply(B.streamEnd, REPLY)).toBe(REPLY);
  });

  it("shows the full reply in the open thread with every menu closed in its still frame", () => {
    expect(chatStreamedReply(CHAT_STILL, REPLY)).toBe(REPLY);
    expect(chatThreadShown(CHAT_STILL)).toBe(1);
    expect(chatStepsFolded(CHAT_STILL)).toBe(1);
    expect(chatReplyStripShown(CHAT_STILL)).toBe(true);
    expect(chatPickerShown(CHAT_STILL)).toBe(0);
    expect(chatWorking(CHAT_STILL)).toBe(false);
    expect(Object.values(chatTouches(CHAT_STILL)).every((touch) => touch === 0)).toBe(true);
  });

  it("clicks with the desktop pointer on the beats the touches land on, resting on each control through its press", () => {
    expect(welcomeCursorScriptOrdered(CHAT_CURSOR)).toBe(true);
    const clicks = CHAT_CURSOR.filter((stop) => stop.press);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      ["chat-editor", B.editorTap],
      ["chat-send", B.sendTap],
    ]);
    for (const stop of clicks) {
      const press = stop.press;
      if (!press) continue;
      expect(press.start).toBe(press.end - FILM_TAP_MS);
      for (let elapsed = press.start; elapsed <= press.end; elapsed += 10) expect(welcomeCursorHovered(CHAT_CURSOR, elapsed)).toBe(stop.target);
    }
  });

  it("leaves Claude to the keyboard, waits for the question before Send, and for the thread before resting on it", () => {
    expect(CHAT_CURSOR.some((stop) => stop.target === "chat-pick")).toBe(false);
    expect(CHAT_CURSOR.find((stop) => stop.target === "chat-send")?.depart).toBeGreaterThanOrEqual(B.typeEnd);
    expect(CHAT_CURSOR.find((stop) => stop.target === "chat-thread")?.depart).toBeGreaterThanOrEqual(B.threadOpen + CHAT_THREAD_OPEN_MS);
  });
});
