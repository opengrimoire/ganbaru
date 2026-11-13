/**
 * Timing for the welcome scene on Chat. In the Novel project's #general channel, an at sign typed into the composer
 * opens the reference picker, a short query narrows it to Claude, and Enter accepts Claude, which turns the query into a
 * mention and shows the assignment it will create. The question is typed after the mention and sent, which opens its
 * reply thread, where Claude thinks, reads the chapter, searches the manuscript, folds that work into one line, and
 * streams its reply.
 */

import { cursorClick, cursorRest, WELCOME_CURSOR_TRAVEL_MS, type WelcomeCursorScript } from "./welcome-cursor";
import { easeInOutCubic, filmCaretOn, filmSpan, filmTap, filmTyped } from "./welcome-film";
import { PROJECTS_CURSOR_REST_ANCHOR } from "./welcome-projects-scene";

/** The agents in the channel, by the names their teammate identities carry, and the model company each one runs on. */
export const CHAT_SCENE_AGENTS = [
  { key: "claude", name: "Claude", familyId: "claude" },
  { key: "codex", name: "Codex", familyId: "codex" },
] as const;

/** One of the channel's agents. */
export type ChatSceneAgent = (typeof CHAT_SCENE_AGENTS)[number];

/** The agent the scene mentions. */
export const CHAT_SCENE_MENTIONED: ChatSceneAgent = CHAT_SCENE_AGENTS[0];

/** How many letters of the mentioned agent's name are typed after the at sign to narrow the picker to it. */
export const CHAT_SCENE_QUERY_LENGTH = 2;

/** The query typed after the at sign, as a person types a name: its first letters, in lowercase. */
export const CHAT_SCENE_QUERY = CHAT_SCENE_MENTIONED.name.slice(0, CHAT_SCENE_QUERY_LENGTH).toLowerCase();

/** The tool steps of the turn, in the order Claude takes them. */
export type ChatSceneStepKey = "read" | "search";
export const CHAT_SCENE_STEPS: readonly ChatSceneStepKey[] = ["read", "search"];

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const CHAT_BEATS = {
  /** The composer is clicked, and an at sign opens the reference picker. */
  editorTap: 900,
  mentionTyped: 1400,
  /**
   * A query narrows the picker to Claude, and Claude is accepted, which turns the query into a mention: with Enter, as
   * the picker completes its highlighted row from the keyboard, or with a tap on a phone.
   */
  queryTypeStart: 2100,
  queryTypeEnd: 2400,
  pick: 2900,
  /** The question is typed after the mention and sent, which posts it to the channel. */
  typeStart: 3200,
  typeEnd: 4600,
  sendTap: 5300,
  /** The message's reply thread opens beside the channel, or over it where there is no room for both. */
  threadOpen: 5600,
  /** Claude's reply starts with a thought, reads the chapter, then searches the manuscript. */
  replyStart: 6100,
  readStart: 6700,
  searchStart: 7500,
  /** The work folds into one line, the reply streams in, and the turn ends. */
  fold: 8300,
  streamEnd: 10100,
  end: 11100,
} as const;

const B = CHAT_BEATS;

/** How long the last frame rests before the scene starts over. */
export const CHAT_END_HOLD_MS = 900;

/** The frame shown as a still with reduced motion: the reply in full under its folded work, in the open thread. */
export const CHAT_STILL = B.end;

/** How long the reference picker and the assignment preview take to rise in, and the picker to fade out. */
export const CHAT_PICKER_ENTER_MS = 160;
export const CHAT_PICKER_EXIT_MS = 100;

/** How long the reply thread takes to open, as the workspace moves its panels. */
export const CHAT_THREAD_OPEN_MS = 320;

/** How long a new message takes to rise into its feed. */
export const CHAT_MESSAGE_IN_MS = 140;

/** How long the turn's steps take to fold into their summary line. */
export const CHAT_STEPS_FOLD_MS = 260;

/** How long one sweep of an active step's shimmer takes, and how far into a sweep a step starts. */
export const CHAT_SHIMMER_SWEEP_MS = 1500;
const CHAT_SHIMMER_LEAD_MS = 250;

/** The seconds the summary line says the turn worked for. */
export const CHAT_SCENE_WORKED_SECONDS = 8;

/** Whether the composer holds the caret: from the click into it until Send. */
export function chatComposerFocused(elapsed: number): boolean {
  return elapsed >= B.editorTap && elapsed < B.sendTap;
}

/** Whether the composer's caret shows: steady while something is typed into it, and blinking in between. */
export function chatComposerCaretOn(elapsed: number): boolean {
  if (!chatComposerFocused(elapsed)) return false;
  if (elapsed >= B.typeStart) return filmCaretOn(elapsed, B.typeEnd);
  if (elapsed >= B.queryTypeStart) return filmCaretOn(elapsed, B.queryTypeEnd);
  return filmCaretOn(elapsed, B.mentionTyped);
}

/** Whether Claude has been picked, which turns the at sign and its query into a mention. */
export function chatMentioned(elapsed: number): boolean {
  return elapsed >= B.pick;
}

/** Whether the message has been sent. */
export function chatSent(elapsed: number): boolean {
  return elapsed >= B.sendTap;
}

/** The composer's text: what it holds before the mention, and what follows the mention once Claude is picked. */
export interface ChatSceneDraft {
  /** The at sign and its query while the picker is open, or nothing. */
  query: string;
  /** Whether the composer holds the mention. */
  mention: boolean;
  /** As much of the question as follows the mention. */
  text: string;
}

/** The composer's text at `elapsed`: the at sign and its query, then the mention and the question, and nothing once sent. */
export function chatComposerDraft(elapsed: number, question: string): ChatSceneDraft {
  if (chatSent(elapsed)) return { query: "", mention: false, text: "" };
  if (chatMentioned(elapsed)) return { query: "", mention: true, text: filmTyped(elapsed, question, B.typeStart, B.typeEnd) };
  if (elapsed < B.mentionTyped) return { query: "", mention: false, text: "" };
  return { query: `@${filmTyped(elapsed, CHAT_SCENE_QUERY, B.queryTypeStart, B.queryTypeEnd)}`, mention: false, text: "" };
}

/** Whether the reference picker is open: from the at sign until Claude is picked. */
export function chatPickerOpen(elapsed: number): boolean {
  return elapsed >= B.mentionTyped && elapsed < B.pick;
}

/** How far the reference picker shows, from 0 to 1: it rises in from the at sign and fades out as Claude is picked. */
export function chatPickerShown(elapsed: number): number {
  const enter = easeInOutCubic(filmSpan(elapsed, B.mentionTyped, B.mentionTyped + CHAT_PICKER_ENTER_MS));
  const exit = filmSpan(elapsed, B.pick, B.pick + CHAT_PICKER_EXIT_MS);
  return enter * (1 - exit);
}

/** Whether the picker lists only what matches the typed query: Claude, once a letter of its name follows the at sign. */
export function chatPickerNarrowed(elapsed: number): boolean {
  return chatComposerDraft(elapsed, "").query.length > 1;
}

/** How far the sent message has risen into the channel, from 0 to 1. */
export function chatMessageIn(elapsed: number): number {
  return filmSpan(elapsed, B.sendTap, B.sendTap + CHAT_MESSAGE_IN_MS);
}

/** How far the reply thread has opened, from 0 to 1. */
export function chatThreadShown(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.threadOpen, B.threadOpen + CHAT_THREAD_OPEN_MS));
}

/** How far Claude's reply row has risen into the thread, from 0 to 1. */
export function chatReplyIn(elapsed: number): number {
  return filmSpan(elapsed, B.replyStart, B.replyStart + CHAT_MESSAGE_IN_MS);
}

/** Whether the channel message shows its reply strip: from when Claude's reply starts in its thread. */
export function chatReplyStripShown(elapsed: number): boolean {
  return elapsed >= B.replyStart;
}

/** Whether Claude is working, from Send until the reply has streamed in, which turns the thread's Send into Stop. */
export function chatWorking(elapsed: number): boolean {
  return chatSent(elapsed) && elapsed < B.streamEnd;
}

/** Where the turn is before its steps fold: thinking, or at a step, done or not. */
export type ChatSceneStepState = "hidden" | "active" | "done";

/** Whether the turn shows its thought, which the first step replaces. */
export function chatThinkingShown(elapsed: number): boolean {
  return elapsed >= B.replyStart && elapsed < B.readStart;
}

/** When each step starts, and when it is done: at the next step, or as the work folds. */
const STEP_SPANS: Readonly<Record<ChatSceneStepKey, { start: number; done: number }>> = {
  read: { start: B.readStart, done: B.searchStart },
  search: { start: B.searchStart, done: B.fold },
};

/** A step's state at `elapsed`. */
export function chatStepState(key: ChatSceneStepKey, elapsed: number): ChatSceneStepState {
  const span = STEP_SPANS[key];
  if (elapsed < span.start) return "hidden";
  return elapsed < span.done ? "active" : "done";
}

/** How far the steps have folded into their summary line, from 0 to 1. */
export function chatStepsFolded(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.fold, B.fold + CHAT_STEPS_FOLD_MS));
}

/**
 * An active line's shimmer: the background position of its highlight, from 100% to 0%, as it sweeps across the text
 * from `since`, starting a little way into its first sweep so the highlight is soon on the text.
 */
export function chatShimmerPosition(elapsed: number, since: number): number {
  const into = (Math.max(0, elapsed - since) + CHAT_SHIMMER_LEAD_MS) % CHAT_SHIMMER_SWEEP_MS;
  return 100 * (1 - easeInOutCubic(into / CHAT_SHIMMER_SWEEP_MS));
}

/** When the line that is active at `elapsed` became active: the thought, or a step. */
export function chatActiveSince(elapsed: number): number {
  if (chatThinkingShown(elapsed)) return B.replyStart;
  const step = CHAT_SCENE_STEPS.find((key) => chatStepState(key, elapsed) === "active");
  return step ? STEP_SPANS[step].start : 0;
}

/** As much of `reply` as has streamed in, a whole word at a time, from the fold until the turn ends. */
export function chatStreamedReply(elapsed: number, reply: string): string {
  const words = reply.split(" ");
  const shown = Math.floor(filmSpan(elapsed, B.fold, B.streamEnd) * words.length);
  return words.slice(0, shown).join(" ");
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface ChatSceneTouches {
  editor: number;
  pick: number;
  send: number;
}

/** A phone scene's touches at `elapsed`. */
export function chatTouches(elapsed: number): ChatSceneTouches {
  return {
    editor: filmTap(elapsed, B.editorTap),
    pick: filmTap(elapsed, B.pick),
    send: filmTap(elapsed, B.sendTap),
  };
}

/** When the desktop pointer settles over the open thread: once the thread has opened under it. */
const CHAT_CURSOR_THREAD_REST = B.threadOpen + CHAT_THREAD_OPEN_MS + WELCOME_CURSOR_TRAVEL_MS;

/**
 * The desktop pointer's path through the scene: it clicks into the composer and waits while the picker opens and
 * narrows, stays there while Enter accepts Claude and the question is typed, then clicks Send and rests
 * over the thread as Claude replies.
 */
export const CHAT_CURSOR: WelcomeCursorScript = [
  cursorRest("chat-panel", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
  cursorClick("chat-editor", B.editorTap, { glyph: "text", after: 0 }),
  cursorClick("chat-send", B.sendTap, { after: B.typeEnd }),
  cursorRest("chat-thread", CHAT_CURSOR_THREAD_REST, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
];
