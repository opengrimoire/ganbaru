/**
 * Timing for the welcome scene about the app itself. The vault fills in on a computer, a phone is linked and the
 * same items travel to it straight from the computer, an edit on the phone travels back, and a sweep then shows the
 * code underneath both screens before it closes again. Every moment is derived from the milliseconds elapsed since
 * the scene started, so a frame is a pure function of time and the reduced-motion still is simply the last frame.
 */

import { easeInOutCubic, filmSpan } from "./welcome-film";

/** How many items the vault holds, one per main feature. */
export const ABOUT_ITEM_COUNT = 4;

/** The item edited on the phone and synced back to the computer. */
export const ABOUT_EDITED_ITEM = 2;

/** Moments of the scene, in milliseconds from its start. */
export const ABOUT_BEATS = {
  /** The computer's vault fills in, one item after another. */
  itemsStart: 300,
  /** The phone arrives, and the link between the two devices draws itself. */
  phoneIn: 1700,
  linkIn: 2250,
  /** Each item travels from the computer to the phone. */
  syncStart: 2800,
  /** An item edited on the phone travels back. */
  editStart: 5000,
  /** The sweep opens both screens onto their code, holds, and closes them again. */
  sweepIn: 7100,
  sweepOut: 9100,
  end: 10400,
} as const;

const ITEM_STAGGER_MS = 280;
const ITEM_IN_MS = 420;
const PHONE_IN_MS = 500;
const LINK_IN_MS = 500;
const SYNC_STAGGER_MS = 380;
const PACKET_MS = 650;
const ARRIVE_MS = 260;
const EDIT_MS = 420;
const HIGHLIGHT_IN_MS = 200;
const HIGHLIGHT_OUT_MS = 500;
const SWEEP_MS = 1100;

/** When item `index` leaves the computer for the phone. */
function syncDeparture(index: number): number {
  return ABOUT_BEATS.syncStart + index * SYNC_STAGGER_MS;
}

/** When the edit leaves the phone, once the phone has shown it. */
const REPLY_DEPARTURE = ABOUT_BEATS.editStart + EDIT_MS;
/** When the edit reaches the computer. */
const REPLY_ARRIVAL = REPLY_DEPARTURE + PACKET_MS;

/** How far item `index` has appeared on the computer, from 0 to 1. */
export function aboutComputerItem(elapsed: number, index: number): number {
  const start = ABOUT_BEATS.itemsStart + index * ITEM_STAGGER_MS;
  return easeInOutCubic(filmSpan(elapsed, start, start + ITEM_IN_MS));
}

/** How far the phone has arrived, from 0 to 1. */
export function aboutPhone(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, ABOUT_BEATS.phoneIn, ABOUT_BEATS.phoneIn + PHONE_IN_MS));
}

/** How much of the link between the devices is drawn, from the computer's side, from 0 to 1. */
export function aboutLink(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, ABOUT_BEATS.linkIn, ABOUT_BEATS.linkIn + LINK_IN_MS));
}

/** How far item `index` has appeared on the phone, from 0 to 1: it shows once it arrives. */
export function aboutPhoneItem(elapsed: number, index: number): number {
  const arrival = syncDeparture(index) + PACKET_MS;
  return easeInOutCubic(filmSpan(elapsed, arrival, arrival + ARRIVE_MS));
}

/**
 * Where each change travelling between the devices is, from 0 at the computer to 1 at the phone. Only changes on the
 * way are listed.
 */
export function aboutPackets(elapsed: number): number[] {
  const packets: number[] = [];
  for (let index = 0; index < ABOUT_ITEM_COUNT; index += 1) {
    const departure = syncDeparture(index);
    if (elapsed > departure && elapsed < departure + PACKET_MS) {
      packets.push(easeInOutCubic(filmSpan(elapsed, departure, departure + PACKET_MS)));
    }
  }
  if (elapsed > REPLY_DEPARTURE && elapsed < REPLY_ARRIVAL) {
    packets.push(1 - easeInOutCubic(filmSpan(elapsed, REPLY_DEPARTURE, REPLY_ARRIVAL)));
  }
  return packets;
}

/** The edited item on each device, from 0 (as it was) to 1 (edited), and how strongly it is highlighted. */
export function aboutEdit(elapsed: number): { phone: number; computer: number; highlight: number } {
  const settled = REPLY_ARRIVAL + EDIT_MS;
  return {
    phone: easeInOutCubic(filmSpan(elapsed, ABOUT_BEATS.editStart, REPLY_DEPARTURE)),
    computer: easeInOutCubic(filmSpan(elapsed, REPLY_ARRIVAL, settled)),
    highlight: filmSpan(elapsed, ABOUT_BEATS.editStart, ABOUT_BEATS.editStart + HIGHLIGHT_IN_MS)
      * (1 - filmSpan(elapsed, settled, settled + HIGHLIGHT_OUT_MS)),
  };
}

/** How far across the scene the code shows, from 0 (none) to 1 (all of it), as the sweep opens and closes it. */
export function aboutCodeReveal(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, ABOUT_BEATS.sweepIn, ABOUT_BEATS.sweepIn + SWEEP_MS))
    - easeInOutCubic(filmSpan(elapsed, ABOUT_BEATS.sweepOut, ABOUT_BEATS.sweepOut + SWEEP_MS));
}

/** Whether the sweep line is moving across the scene. */
export function aboutSweeping(elapsed: number): boolean {
  const reveal = aboutCodeReveal(elapsed);
  return reveal > 0 && reveal < 1;
}
