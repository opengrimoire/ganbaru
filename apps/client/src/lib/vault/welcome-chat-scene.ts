/**
 * Timing for the welcome scene on Chat. With a pointer, a prompt is typed into the Novel project's composer, the model
 * picker raises Claude's effort from High to Max, and Send starts the turn: Claude reads the chapter, searches the
 * manuscript, folds that work into one line, and streams its reply. A phone starts with the prompt typed and shows the
 * turn from its Send onward.
 */

import { cursorClick, cursorRest, type WelcomeCursorScript } from "./welcome-cursor";
import { easeInOutCubic, filmCaretOn, filmSpan, filmTap, filmTyped } from "./welcome-film";
import { PROJECTS_CURSOR_REST_ANCHOR } from "./welcome-projects-scene";

/** The Claude model the scene talks to, by the name the model picker shows for it. */
export const CHAT_SCENE_MODEL_NAME = "Opus 5.5";

/** The model's effort levels, in the order the effort ladder lays them out, as the provider reports them. */
export const CHAT_SCENE_EFFORTS = ["low", "medium", "high", "xhigh", "max"] as const;

/** One of the model's effort levels. */
export type ChatSceneEffort = (typeof CHAT_SCENE_EFFORTS)[number];

/** The effort the composer starts at, and the one the scene picks. */
export const CHAT_SCENE_START_EFFORT: ChatSceneEffort = "high";
export const CHAT_SCENE_PICKED_EFFORT: ChatSceneEffort = "max";

/** The tool steps of the turn, in the order Claude takes them. */
export type ChatSceneStepKey = "read" | "search";
export const CHAT_SCENE_STEPS: readonly ChatSceneStepKey[] = ["read", "search"];

/** Moments of the desktop scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const CHAT_BEATS = {
  /** The composer is clicked, and the prompt is typed into it. */
  editorTap: 1100,
  typeStart: 1300,
  typeEnd: 2700,
  /** The model picker opens, Max is picked on its effort ladder, and Send closes it and sends the prompt. */
  modelTap: 3400,
  effortTap: 4200,
  sendTap: 5000,
  /** Claude's reply starts with a thought, reads the chapter, then searches the manuscript. */
  replyStart: 5400,
  readStart: 6000,
  searchStart: 6800,
  /** The work folds into one line, the reply streams in, and the turn ends. */
  fold: 7600,
  streamEnd: 9400,
  end: 10400,
} as const;

const B = CHAT_BEATS;

/** How much earlier a phone's turn plays than the desktop one, since its scene starts with the prompt typed. */
const MOBILE_LEAD_MS = B.sendTap - 900;

/** How long the last frame rests before the scene starts over. */
export const CHAT_END_HOLD_MS = 900;

/** How long the model picker takes to rise in and to fade out. */
export const CHAT_PICKER_ENTER_MS = 180;
export const CHAT_PICKER_EXIT_MS = 120;

/** How long the effort knob takes to slide to its stop, and how long the ladder takes to turn to the Max colors. */
export const CHAT_KNOB_SLIDE_MS = 240;
export const CHAT_ULTRA_FADE_MS = 300;

/** How long a new message takes to rise into the timeline, as the timeline animates sent messages. */
export const CHAT_MESSAGE_IN_MS = 140;

/** How long the working folder strip takes to fold away once the first message sends. */
export const CHAT_STRIP_FOLD_MS = 240;

/** How long the turn's steps take to fold into their summary line. */
export const CHAT_STEPS_FOLD_MS = 260;

/** How long one sweep of an active step's shimmer takes, and how far into a sweep a step starts. */
export const CHAT_SHIMMER_SWEEP_MS = 1500;
const CHAT_SHIMMER_LEAD_MS = 250;

/** The seconds the summary line says the turn worked for. */
export const CHAT_SCENE_WORKED_SECONDS = 8;

/** A desktop beat as it plays on `mobile`'s shell; a phone has no beats before Send. */
function beat(at: number, mobile: boolean): number {
  return mobile ? at - MOBILE_LEAD_MS : at;
}

/** How long a run of the scene lasts on a phone or with a pointer. */
export function chatSceneEnd(mobile: boolean): number {
  return beat(B.end, mobile);
}

/** The frame shown as a still with reduced motion: the reply in full under its folded work. */
export function chatSceneStill(mobile: boolean): number {
  return chatSceneEnd(mobile);
}

/** When Send's tap lands. */
export function chatSendTap(mobile: boolean): number {
  return beat(B.sendTap, mobile);
}

/** Whether the prompt has been sent. */
export function chatSent(elapsed: number, mobile: boolean): boolean {
  return elapsed >= chatSendTap(mobile);
}

/** As much of `prompt` as the composer holds: typed a character at a time with a pointer, whole on a phone, and gone once sent. */
export function chatComposerDraft(elapsed: number, prompt: string, mobile: boolean): string {
  if (chatSent(elapsed, mobile)) return "";
  return mobile ? prompt : filmTyped(elapsed, prompt, B.typeStart, B.typeEnd);
}

/** Whether the composer's caret shows: from the click into it, steady while typing, then blinking until Send. */
export function chatComposerCaretOn(elapsed: number, mobile: boolean): boolean {
  if (mobile || elapsed < B.editorTap || chatSent(elapsed, mobile)) return false;
  return filmCaretOn(elapsed, B.typeEnd);
}

/** How far the model picker shows, from 0 to 1: it rises in from its tap and fades out as Send is clicked. */
export function chatPickerShown(elapsed: number, mobile: boolean): number {
  if (mobile) return 0;
  const enter = easeInOutCubic(filmSpan(elapsed, B.modelTap, B.modelTap + CHAT_PICKER_ENTER_MS));
  const exit = filmSpan(elapsed, B.sendTap, B.sendTap + CHAT_PICKER_EXIT_MS);
  return enter * (1 - exit);
}

/** Whether the model picker is open, as its trigger shows it. */
export function chatPickerOpen(elapsed: number, mobile: boolean): boolean {
  return !mobile && elapsed >= B.modelTap && elapsed < B.sendTap;
}

/** The effort the composer is set to: Max on a phone, and with a pointer once its stop is clicked. */
export function chatEffort(elapsed: number, mobile: boolean): ChatSceneEffort {
  return mobile || elapsed >= B.effortTap ? CHAT_SCENE_PICKED_EFFORT : CHAT_SCENE_START_EFFORT;
}

/** Eases a 0 to 1 progress so it starts fast and settles, as the effort knob slides. */
function easeOutCubic(progress: number): number {
  return 1 - (1 - progress) ** 3;
}

/**
 * The effort knob's place on the ladder, as a fractional stop index: it slides from High to Max as Max is clicked.
 * A phone starts at Max.
 */
export function chatKnobStop(elapsed: number, mobile: boolean): number {
  const from = CHAT_SCENE_EFFORTS.indexOf(CHAT_SCENE_START_EFFORT);
  const to = CHAT_SCENE_EFFORTS.indexOf(CHAT_SCENE_PICKED_EFFORT);
  if (mobile) return to;
  return from + (to - from) * easeOutCubic(filmSpan(elapsed, B.effortTap, B.effortTap + CHAT_KNOB_SLIDE_MS));
}

/** How far the ladder has turned to its Max colors, from 0 to 1, since Max is the ladder's top stop. */
export function chatUltra(elapsed: number, mobile: boolean): number {
  if (mobile) return 1;
  return filmSpan(elapsed, B.effortTap, B.effortTap + CHAT_ULTRA_FADE_MS);
}

/** Whether the effort knob is held, from the press on Max until it lets go, which grows the knob a little. */
export function chatKnobHeld(elapsed: number, mobile: boolean): boolean {
  return !mobile && filmTap(elapsed, B.effortTap) > 0;
}

/** How far the working folder strip shows over the composer, from 0 to 1: it folds away as the first message sends. */
export function chatStripShown(elapsed: number, mobile: boolean): number {
  const send = chatSendTap(mobile);
  return 1 - easeInOutCubic(filmSpan(elapsed, send, send + CHAT_STRIP_FOLD_MS));
}

/** How far the sent message has risen into the timeline, from 0 to 1. */
export function chatUserMessageIn(elapsed: number, mobile: boolean): number {
  const send = chatSendTap(mobile);
  return filmSpan(elapsed, send, send + CHAT_MESSAGE_IN_MS);
}

/** How far Claude's reply row has risen into the timeline, from 0 to 1. */
export function chatReplyIn(elapsed: number, mobile: boolean): number {
  const start = beat(B.replyStart, mobile);
  return filmSpan(elapsed, start, start + CHAT_MESSAGE_IN_MS);
}

/** Whether the turn is running, from Send until the reply has streamed in, which turns Send into Stop. */
export function chatWorking(elapsed: number, mobile: boolean): boolean {
  return chatSent(elapsed, mobile) && elapsed < beat(B.streamEnd, mobile);
}

/** Where the turn is before its steps fold: thinking, or at a step, done or not. */
export type ChatSceneStepState = "hidden" | "active" | "done";

/** The state of the turn's thought, which the first step replaces. */
export function chatThinkingShown(elapsed: number, mobile: boolean): boolean {
  return elapsed >= beat(B.replyStart, mobile) && elapsed < beat(B.readStart, mobile);
}

/** When each step starts, and when it is done: at the next step, or as the work folds. */
const STEP_SPANS: Readonly<Record<ChatSceneStepKey, { start: number; done: number }>> = {
  read: { start: B.readStart, done: B.searchStart },
  search: { start: B.searchStart, done: B.fold },
};

/** A step's state at `elapsed`. */
export function chatStepState(key: ChatSceneStepKey, elapsed: number, mobile: boolean): ChatSceneStepState {
  const span = STEP_SPANS[key];
  if (elapsed < beat(span.start, mobile)) return "hidden";
  return elapsed < beat(span.done, mobile) ? "active" : "done";
}

/** How far the steps have folded into their summary line, from 0 to 1. */
export function chatStepsFolded(elapsed: number, mobile: boolean): number {
  const fold = beat(B.fold, mobile);
  return easeInOutCubic(filmSpan(elapsed, fold, fold + CHAT_STEPS_FOLD_MS));
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
export function chatActiveSince(elapsed: number, mobile: boolean): number {
  if (chatThinkingShown(elapsed, mobile)) return beat(B.replyStart, mobile);
  const step = CHAT_SCENE_STEPS.find((key) => chatStepState(key, elapsed, mobile) === "active");
  return step ? beat(STEP_SPANS[step].start, mobile) : 0;
}

/** As much of `reply` as has streamed in, a whole word at a time, from the fold until the turn ends. */
export function chatStreamedReply(elapsed: number, reply: string, mobile: boolean): string {
  const words = reply.split(" ");
  const shown = Math.floor(filmSpan(elapsed, beat(B.fold, mobile), beat(B.streamEnd, mobile)) * words.length);
  return words.slice(0, shown).join(" ");
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface ChatSceneTouches {
  send: number;
}

/** A phone scene's touches at `elapsed`. */
export function chatTouches(elapsed: number): ChatSceneTouches {
  return { send: filmTap(elapsed, chatSendTap(true)) };
}

/**
 * The desktop pointer's path through the scene: it clicks into the composer and rests there while the prompt is typed,
 * opens the model picker, clicks Max on the effort ladder, then clicks Send.
 */
export const CHAT_CURSOR: WelcomeCursorScript = [
  cursorRest("chat-panel", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
  cursorClick("chat-editor", B.editorTap, { glyph: "text" }),
  cursorClick("chat-model", B.modelTap, { after: B.typeEnd }),
  cursorClick("chat-effort-max", B.effortTap, { after: B.modelTap + CHAT_PICKER_ENTER_MS }),
  cursorClick("chat-send", B.sendTap, { after: B.effortTap }),
];
