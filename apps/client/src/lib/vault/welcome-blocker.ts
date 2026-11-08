/**
 * Shared pieces of the distraction blocker's welcome scenes: the parody apps they draw, what the browser and the phone
 * show, and the timing of the motions both scenes use. Every value is a pure function of the milliseconds elapsed since
 * a scene started, so a frame is a pure function of time.
 */

import { cursorClick, cursorRest, type WelcomeCursorStop } from "./welcome-cursor";
import { easeInOutCubic, filmSpan } from "./welcome-film";

/** The parody apps the scenes draw, each with a website of the same name. */
export type BlockerApp = "y" | "bluedit" | "egogram";

/** The parody apps' names, which stay the same in every language. */
export const BLOCKER_APP_NAMES: Readonly<Record<BlockerApp, string>> = { y: "Y", bluedit: "Bluedit", egogram: "Egogram" };

/** The parody apps' websites. */
export const BLOCKER_APP_HOSTS: Readonly<Record<BlockerApp, string>> = {
  y: "y.com",
  bluedit: "bluedit.com",
  egogram: "egogram.com",
};

/** The parody mail site the scenes show as work, which the blocker never blocks, and its website. */
export const BLOCKER_MAIL = { name: "Qnail", host: "qnail.com" } as const;

/** Why a site or an app is blocked: a running Focus phase, or a daily limit that ran out. */
export type BlockerReason = "focus" | "limit";

/**
 * What the browser's active tab shows: the mail inbox, a new tab's empty page, a site's feed scrolled from 0 to 1, or
 * the extension's block page for a site.
 */
export type BlockerBrowserPage =
  | { kind: "mail" }
  | { kind: "blank" }
  | { kind: "feed"; app: BlockerApp; scroll: number }
  | { kind: "block"; app: BlockerApp; reason: "focus"; minutesLeft: number }
  | { kind: "block"; app: BlockerApp; reason: "limit" };

/** What a browser tab carries: the mail inbox, an empty new tab, a site, or the extension's block page. */
export type BlockerTabContent = "mail" | "blank" | "blocked" | BlockerApp;

/** A browser tab, and whether it is the active one. */
export interface BlockerTab {
  content: BlockerTabContent;
  active: boolean;
}

/**
 * What the browser's address bar shows: a site, text being typed with its caret drawn or not as it blinks, or the
 * extension's page.
 */
export type BlockerAddress =
  | { kind: "host"; host: string }
  | { kind: "typed"; text: string; caret: boolean }
  | { kind: "extension" };

/**
 * One frame of the browser: its tabs, its address bar, the active tab's page and how far it has faded in, how far a
 * page has loaded, from 0 to 1 (0 when none loads). The mouse pointer's clicks on it are each scene's cursor script.
 */
export interface BlockerBrowserFrame {
  tabs: readonly BlockerTab[];
  address: BlockerAddress;
  page: BlockerBrowserPage;
  pageIn: number;
  loading: number;
}

/** What a tab showing `page` carries. */
export function blockerTabOf(page: BlockerBrowserPage): BlockerTabContent {
  switch (page.kind) {
    case "mail":
      return "mail";
    case "blank":
      return "blank";
    case "feed":
      return page.app;
    case "block":
      return "blocked";
  }
}

/** What the address bar shows for `page` while nothing is typed. */
export function blockerAddressOf(page: BlockerBrowserPage): BlockerAddress {
  switch (page.kind) {
    case "mail":
      return { kind: "host", host: BLOCKER_MAIL.host };
    case "blank":
      return { kind: "typed", text: "", caret: false };
    case "feed":
      return { kind: "host", host: BLOCKER_APP_HOSTS[page.app] };
    case "block":
      return { kind: "extension" };
  }
}

/** A browser with a single tab showing `page`. */
export function blockerSingleTab(page: BlockerBrowserPage, pageIn: number): BlockerBrowserFrame {
  return { tabs: [{ content: blockerTabOf(page), active: true }], address: blockerAddressOf(page), page, pageIn, loading: 0 };
}

/**
 * The `data-cursor` names of the browser's places the mouse pointer goes to: the active tab's page, the address bar,
 * and the new tab button. {@link blockerCursorClose} names each tab's close button.
 */
export const BLOCKER_CURSOR = {
  page: "browser-page",
  address: "browser-address",
  newTab: "browser-new-tab",
} as const;

/** The `data-cursor` name of the close button of the tab carrying `content`. */
export function blockerCursorClose(content: BlockerTabContent): string {
  return `browser-close-${content}`;
}

/** Where the pointer clicks the address bar: toward its start, where the text is, with the text beam. */
const ADDRESS_ANCHOR = [0.3, 0.5] as const;

/**
 * A click on the address bar landing at `tap`, after `after` when given, as the browser's text field takes it: the
 * pointer shows the text beam over the field, as a browser shows it.
 */
export function blockerAddressClick(tap: number, after?: number): WelcomeCursorStop {
  return cursorClick(BLOCKER_CURSOR.address, tap, { anchor: ADDRESS_ANCHOR, glyph: "text", after });
}

/** The pointer resting on the page at `arrive`, at `anchor` on it, as it does while a page scrolls under the wheel. */
export function blockerPageRest(arrive: number, anchor: readonly [x: number, y: number]): WelcomeCursorStop {
  return cursorRest(BLOCKER_CURSOR.page, arrive, { anchor });
}

/** The prefix of `text` typed by `elapsed`, one character at a time from `start` to `end`. */
export function blockerTyped(text: string, start: number, end: number, elapsed: number): string {
  return text.slice(0, Math.floor(text.length * filmSpan(elapsed, start, end)));
}

/** How long the text caret stays on, and off, as it blinks. */
const CARET_BLINK_MS = 530;

/** Whether the text caret is drawn at this moment, as it blinks. */
export function blockerCaretOn(elapsed: number): boolean {
  return Math.floor(elapsed / CARET_BLINK_MS) % 2 === 0;
}

/** The app open on the phone, how far it has opened from its icon, and how far its feed has scrolled, each from 0 to 1. */
export interface BlockerPhoneApp {
  app: BlockerApp;
  open: number;
  scroll: number;
}

/** The phone's block notification, for which app and why, and how far it has slid in, from 0 to 1. */
export interface BlockerNotice {
  app: BlockerApp;
  reason: BlockerReason;
  shown: number;
}

/** A move of the camera on a narrow canvas, toward the browser (0) or the phone (1). */
export interface BlockerPanMove {
  start: number;
  to: 0 | 1;
}

/** How long an app takes to open from its icon, and to shrink back into it. */
export const BLOCKER_APP_OPEN_MS = 320;
const APP_CLOSE_MS = 320;
/** How long a notification takes to slide in, and away. */
const NOTICE_IN_MS = 280;
const NOTICE_OUT_MS = 220;
/** How long a page takes to fade in once it replaces another. */
const PAGE_FADE_MS = 220;
/** How long the camera takes to move between the devices on a narrow canvas. */
const PAN_MS = 650;

/** Where the camera points on a narrow canvas, from 0 (the browser) to 1 (the phone), after `moves` in time order. */
export function blockerPan(elapsed: number, moves: readonly BlockerPanMove[]): number {
  let value = 0;
  for (const move of moves) {
    value += (move.to - value) * easeInOutCubic(filmSpan(elapsed, move.start, move.start + PAN_MS));
  }
  return value;
}

/**
 * How far an app tapped at `tap` has opened, from 0 to 1: it opens from its icon, and shrinks back into it from `home`
 * when given. 0 before the tap and once it is closed.
 */
export function blockerAppOpen(elapsed: number, tap: number, home?: number): number {
  if (elapsed < tap) return 0;
  const opened = easeInOutCubic(filmSpan(elapsed, tap, tap + BLOCKER_APP_OPEN_MS));
  const closed = home === undefined ? 0 : easeInOutCubic(filmSpan(elapsed, home, home + APP_CLOSE_MS));
  return opened * (1 - closed);
}

/** How far a notification posted at `posted` has slid in, from 0 to 1, until it slides away from `dismissed` when given. */
export function blockerNoticeShown(elapsed: number, posted: number, dismissed?: number): number {
  const shown = easeInOutCubic(filmSpan(elapsed, posted, posted + NOTICE_IN_MS));
  const gone = dismissed === undefined ? 0 : easeInOutCubic(filmSpan(elapsed, dismissed, dismissed + NOTICE_OUT_MS));
  return shown * (1 - gone);
}

/** How far a page has faded in since the last of `changes` it has passed, from 0 to 1; 1 before the first. */
export function blockerPageIn(elapsed: number, changes: readonly number[]): number {
  const last = changes.filter((change) => change <= elapsed).at(-1);
  return last === undefined ? 1 : easeInOutCubic(filmSpan(elapsed, last, last + PAGE_FADE_MS));
}
