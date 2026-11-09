import { describe, expect, it } from "vitest";
import { welcomeCursorScriptOrdered } from "./welcome-cursor";
import {
  CHAT_BEATS,
  CHAT_CURSOR,
  CHAT_SCENE_EFFORTS,
  CHAT_SCENE_PICKED_EFFORT,
  CHAT_SCENE_START_EFFORT,
  CHAT_SCENE_STEPS,
  chatActiveSince,
  chatComposerCaretOn,
  chatComposerDraft,
  chatEffort,
  chatKnobStop,
  chatPickerOpen,
  chatPickerShown,
  chatReplyIn,
  chatSceneEnd,
  chatSceneStill,
  chatSendTap,
  chatSent,
  chatShimmerPosition,
  chatStepState,
  chatStepsFolded,
  chatStreamedReply,
  chatStripShown,
  chatThinkingShown,
  chatTouches,
  chatUltra,
  chatUserMessageIn,
  chatWorking,
  type ChatSceneStepState,
} from "./welcome-chat-scene";

const B = CHAT_BEATS;
const PROMPT = "Read chapter 2 and tell me what Mateo knows about Clara.";
const REPLY = "In chapter 2, Mateo sees Clara leave the station with the letter.";

/** Every step of a scene, for properties that must hold throughout. */
function moments(end: number, step = 10): number[] {
  return Array.from({ length: Math.floor(end / step) + 1 }, (_, index) => index * step);
}

/** The order a step's states may come in. */
const STATE_RANK: Record<ChatSceneStepState, number> = { hidden: 0, active: 1, done: 2 };

describe("welcome chat scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
    expect(new Set(beats).size).toBe(beats.length);
  });

  it("walks its pointer through the targets in time", () => {
    expect(welcomeCursorScriptOrdered(CHAT_CURSOR)).toBe(true);
    expect(CHAT_CURSOR.map((stop) => stop.target)).toEqual(["chat-panel", "chat-editor", "chat-model", `chat-effort-${CHAT_SCENE_PICKED_EFFORT}`, "chat-send"]);
  });

  it("keeps every progress within its range on both shells", () => {
    for (const mobile of [false, true]) {
      for (const at of moments(chatSceneEnd(mobile))) {
        for (const value of [chatPickerShown(at, mobile), chatUltra(at, mobile), chatStripShown(at, mobile), chatUserMessageIn(at, mobile), chatReplyIn(at, mobile), chatStepsFolded(at, mobile), chatTouches(at).send]) {
          expect(value).toBeGreaterThanOrEqual(0);
          expect(value).toBeLessThanOrEqual(1);
        }
        const stop = chatKnobStop(at, mobile);
        expect(stop).toBeGreaterThanOrEqual(CHAT_SCENE_EFFORTS.indexOf(CHAT_SCENE_START_EFFORT));
        expect(stop).toBeLessThanOrEqual(CHAT_SCENE_EFFORTS.length - 1);
      }
    }
  });

  it("types the prompt with a pointer, then clears it as it sends", () => {
    expect(chatComposerDraft(B.typeStart - 1, PROMPT, false)).toBe("");
    const midway = chatComposerDraft((B.typeStart + B.typeEnd) / 2, PROMPT, false);
    expect(midway.length).toBeGreaterThan(0);
    expect(PROMPT.startsWith(midway)).toBe(true);
    expect(chatComposerDraft(B.typeEnd, PROMPT, false)).toBe(PROMPT);
    expect(chatComposerDraft(B.sendTap, PROMPT, false)).toBe("");
    expect(chatComposerCaretOn(B.editorTap - 1, false)).toBe(false);
    expect(chatComposerCaretOn(B.typeStart, false)).toBe(true);
    expect(chatComposerCaretOn(B.sendTap, false)).toBe(false);
  });

  it("raises the effort from its start to Max through the open picker", () => {
    expect(chatPickerOpen(B.modelTap - 1, false)).toBe(false);
    expect(chatPickerOpen(B.effortTap, false)).toBe(true);
    expect(chatPickerOpen(B.sendTap, false)).toBe(false);
    expect(chatPickerShown(B.end, false)).toBe(0);
    expect(chatEffort(B.effortTap - 1, false)).toBe(CHAT_SCENE_START_EFFORT);
    expect(chatEffort(B.effortTap, false)).toBe(CHAT_SCENE_PICKED_EFFORT);
    expect(chatKnobStop(B.effortTap - 1, false)).toBe(CHAT_SCENE_EFFORTS.indexOf(CHAT_SCENE_START_EFFORT));
    expect(chatKnobStop(B.sendTap, false)).toBe(CHAT_SCENE_EFFORTS.indexOf(CHAT_SCENE_PICKED_EFFORT));
    expect(chatUltra(B.effortTap - 1, false)).toBe(0);
    expect(chatUltra(B.sendTap, false)).toBe(1);
  });

  it("starts a phone with the prompt written at Max and the picker closed", () => {
    expect(chatComposerDraft(0, PROMPT, true)).toBe(PROMPT);
    expect(chatComposerCaretOn(0, true)).toBe(false);
    expect(chatEffort(0, true)).toBe(CHAT_SCENE_PICKED_EFFORT);
    expect(chatUltra(0, true)).toBe(1);
    for (const at of moments(chatSceneEnd(true))) expect(chatPickerShown(at, true)).toBe(0);
    expect(chatSendTap(true)).toBeGreaterThan(0);
    expect(chatSceneEnd(true)).toBeLessThan(chatSceneEnd(false));
  });

  it("touches Send on a phone right before it sends", () => {
    const send = chatSendTap(true);
    expect(chatTouches(send - 1).send).toBeGreaterThan(0);
    expect(chatTouches(send + 1).send).toBe(0);
    expect(chatSent(send - 1, true)).toBe(false);
    expect(chatSent(send, true)).toBe(true);
  });

  it("thinks, then runs each step in turn before they fold", () => {
    for (const mobile of [false, true]) {
      const ranks = CHAT_SCENE_STEPS.map(() => 0);
      for (const at of moments(chatSceneEnd(mobile))) {
        const states = CHAT_SCENE_STEPS.map((key) => chatStepState(key, at, mobile));
        states.forEach((state, index) => {
          expect(STATE_RANK[state]).toBeGreaterThanOrEqual(ranks[index]);
          ranks[index] = STATE_RANK[state];
        });
        expect(states.filter((state) => state === "active").length).toBeLessThanOrEqual(1);
        if (chatThinkingShown(at, mobile)) expect(states.every((state) => state === "hidden")).toBe(true);
      }
      expect(ranks.every((rank) => rank === STATE_RANK.done)).toBe(true);
    }
  });

  it("works from Send until the reply has streamed in whole", () => {
    for (const mobile of [false, true]) {
      const send = chatSendTap(mobile);
      expect(chatWorking(send - 1, mobile)).toBe(false);
      expect(chatWorking(send, mobile)).toBe(true);
      expect(chatWorking(chatSceneEnd(mobile), mobile)).toBe(false);
      expect(chatStreamedReply(chatSceneEnd(mobile), REPLY, mobile)).toBe(REPLY);
      let shown = 0;
      for (const at of moments(chatSceneEnd(mobile))) {
        const streamed = chatStreamedReply(at, REPLY, mobile);
        expect(REPLY.startsWith(streamed)).toBe(true);
        expect(streamed.length).toBeGreaterThanOrEqual(shown);
        if (streamed.length > 0) expect(chatStepsFolded(at, mobile)).toBeGreaterThan(0);
        shown = streamed.length;
      }
    }
  });

  it("folds the strip away and docks the composer once sent", () => {
    expect(chatStripShown(B.sendTap - 1, false)).toBe(1);
    expect(chatStripShown(B.end, false)).toBe(0);
    expect(chatStripShown(0, true)).toBe(1);
  });

  it("sweeps its shimmer within the text from when the line became active", () => {
    for (const at of moments(chatSceneEnd(false))) {
      const position = chatShimmerPosition(at, chatActiveSince(at, false));
      expect(position).toBeGreaterThanOrEqual(0);
      expect(position).toBeLessThanOrEqual(100);
    }
    expect(chatActiveSince(B.replyStart, false)).toBe(B.replyStart);
    expect(chatActiveSince(B.searchStart, false)).toBe(B.searchStart);
  });

  it("holds the finished reply under its folded work as the still", () => {
    for (const mobile of [false, true]) {
      const still = chatSceneStill(mobile);
      expect(chatStepsFolded(still, mobile)).toBe(1);
      expect(chatStreamedReply(still, REPLY, mobile)).toBe(REPLY);
      expect(chatComposerDraft(still, PROMPT, mobile)).toBe("");
    }
  });
});
