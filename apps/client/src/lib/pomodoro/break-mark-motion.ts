import type { SegmentPhase, SegmentStatus, TimelineBand } from "$lib/calendar/types";

/**
 * Eases the Calendar rail's planned break marks into their replanned places, so a mark slides when an adjacent event,
 * an overlap, or a Focus setting changes where its break falls, instead of jumping there.
 *
 * Marks are matched by their owner event and their order among that event's breaks, so the third break of an event
 * stays the third break while it moves. A planned mark that moves with its owner's own start (a dragged event) moves
 * with it at once, like the rail and the event block, and only eases the part of the change the move did not cause.
 * Recorded and active breaks are history and always sit exactly where they ran. Marks of an event that appears or
 * disappears come and go with its rail, while a break that an existing event gains or loses fades in or out. In-flight
 * planned marks are clipped to their owner, so a mark never shows outside the event it belongs to.
 */

/** Where an owner event sits on the rendered day, in minutes. */
export interface BreakMarkOwner {
  startMinute: number;
  endMinute: number;
}

/** A break mark to draw at this moment. */
export interface BreakMark {
  key: string;
  topMinute: number;
  heightMinutes: number;
  phase: SegmentPhase;
  status: SegmentStatus;
  opacity: number;
}

/** A break band with the identity it keeps while it moves. */
export interface KeyedBreakBand {
  key: string;
  band: TimelineBand;
}

/** How long a moving mark takes to close most of the distance to its place, in seconds. */
const POSITION_SMOOTH_SECONDS = 0.16;
/** How long a fading mark takes to mostly appear or disappear, in seconds. */
const OPACITY_SMOOTH_SECONDS = 0.1;
/** A mark this close to its place, and this slow, has arrived. */
const SETTLE_MINUTES = 0.005;
const SETTLE_MINUTES_PER_SECOND = 0.05;
const SETTLE_OPACITY = 0.01;
/** A resting mark moves this little by jumping, so the per-second drift of a plan that follows now never animates. */
const DRIFT_MINUTES = 0.1;
/** Longer gaps between steps, such as a hidden page, advance the motion by this much at most. */
const MAX_STEP_SECONDS = 0.1;

interface Spring {
  value: number;
  velocity: number;
  target: number;
}

interface MarkState {
  ownerId: string;
  phase: SegmentPhase;
  status: SegmentStatus;
  anchored: boolean;
  /** The owner's start when the mark last saw it, to carry an anchored mark along with its owner. */
  ownerStart: number;
  top: Spring;
  end: Spring;
  opacity: Spring;
}

/**
 * Keys every break band by its owner and its order among the owner's breaks in time, counting recorded, active, and
 * planned breaks alike, so a planned break keeps its key when it starts and when it is recorded.
 */
export function keyBreakBands(bands: readonly TimelineBand[]): KeyedBreakBand[] {
  const breaks = bands
    .filter((band) => band.phase !== "focus")
    .sort((left, right) => left.topMinute - right.topMinute);
  const counts = new Map<string, number>();
  return breaks.map((band) => {
    const ordinal = counts.get(band.ownerId) ?? 0;
    counts.set(band.ownerId, ordinal + 1);
    return { key: `${band.ownerId}#${ordinal}`, band };
  });
}

function spring(value: number): Spring {
  return { value, velocity: 0, target: value };
}

function settled(state: Spring): boolean {
  return state.value === state.target && state.velocity === 0;
}

function jump(state: Spring, target: number): void {
  state.value = target;
  state.velocity = 0;
  state.target = target;
}

/**
 * Advances a critically damped spring toward its target. Retargeting keeps the velocity, so a target that keeps
 * changing, as during a drag, bends the motion instead of restarting it.
 */
function smoothDamp(state: Spring, smoothSeconds: number, seconds: number, settleDistance: number, settleSpeed: number): void {
  if (settled(state)) return;
  const omega = 2 / smoothSeconds;
  const x = omega * seconds;
  const decay = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x);
  const change = state.value - state.target;
  const temp = (state.velocity + omega * change) * seconds;
  let velocity = (state.velocity - omega * temp) * decay;
  let value = state.target + (change + temp) * decay;
  // Never pass the target: a spring that crosses it is done.
  if ((state.target - state.value > 0) === (value > state.target)) {
    value = state.target;
    velocity = 0;
  }
  if (Math.abs(value - state.target) < settleDistance && Math.abs(velocity) < settleSpeed) {
    value = state.target;
    velocity = 0;
  }
  state.value = value;
  state.velocity = velocity;
}

/** The moving break marks of one Calendar day column. */
export class BreakMarkMotion {
  #marks = new Map<string, MarkState>();
  #owners: ReadonlyMap<string, BreakMarkOwner> = new Map();

  /**
   * Gives the marks their new places from a freshly planned set of bands.
   *
   * @param bands The day's timeline bands; focus bands are ignored.
   * @param owners Where each Pomodoro-enabled event on the day sits now.
   * @param instant Whether every mark should jump to its place, as when the user prefers less motion.
   */
  retarget(bands: readonly TimelineBand[], owners: ReadonlyMap<string, BreakMarkOwner>, instant = false): void {
    const previousOwners = this.#owners;
    this.#owners = owners;
    const seen = new Set<string>();

    for (const { key, band } of keyBreakBands(bands)) {
      seen.add(key);
      const top = band.topMinute;
      const end = band.topMinute + band.heightMinutes;
      const ownerStart = owners.get(band.ownerId)?.startMinute ?? top;
      const existing = this.#marks.get(key);

      if (!existing || instant || band.status !== "planned" || existing.status !== "planned") {
        const fadeIn = !existing && !instant && band.status === "planned" && previousOwners.has(band.ownerId);
        this.#marks.set(key, {
          ownerId: band.ownerId,
          phase: band.phase,
          status: band.status,
          anchored: band.anchored,
          ownerStart,
          top: spring(top),
          end: spring(end),
          opacity: fadeIn ? { value: 0, velocity: 0, target: 1 } : spring(1),
        });
        continue;
      }

      if (existing.anchored && band.anchored) shift(existing, ownerStart - existing.ownerStart);
      const resting = settled(existing.top) && settled(existing.end);
      if (resting && Math.abs(top - existing.top.value) < DRIFT_MINUTES && Math.abs(end - existing.end.value) < DRIFT_MINUTES) {
        jump(existing.top, top);
        jump(existing.end, end);
      } else {
        existing.top.target = top;
        existing.end.target = end;
      }
      existing.opacity.target = 1;
      existing.phase = band.phase;
      existing.anchored = band.anchored;
      existing.ownerStart = ownerStart;
    }

    for (const [key, mark] of this.#marks) {
      if (seen.has(key)) continue;
      const owner = owners.get(mark.ownerId);
      if (!owner || instant || mark.status !== "planned") {
        this.#marks.delete(key);
        continue;
      }
      if (mark.anchored) shift(mark, owner.startMinute - mark.ownerStart);
      mark.ownerStart = owner.startMinute;
      mark.opacity.target = 0;
    }
  }

  /** Advances every moving mark by the given time and drops marks that finished fading out. */
  step(seconds: number): void {
    const dt = Math.min(MAX_STEP_SECONDS, Math.max(0, seconds));
    if (dt === 0) return;
    for (const [key, mark] of this.#marks) {
      smoothDamp(mark.top, POSITION_SMOOTH_SECONDS, dt, SETTLE_MINUTES, SETTLE_MINUTES_PER_SECOND);
      smoothDamp(mark.end, POSITION_SMOOTH_SECONDS, dt, SETTLE_MINUTES, SETTLE_MINUTES_PER_SECOND);
      smoothDamp(mark.opacity, OPACITY_SMOOTH_SECONDS, dt, SETTLE_OPACITY, SETTLE_OPACITY);
      if (mark.opacity.target === 0 && settled(mark.opacity)) this.#marks.delete(key);
    }
  }

  /** Whether any mark is still moving or fading. */
  get moving(): boolean {
    for (const mark of this.#marks.values()) {
      if (!settled(mark.top) || !settled(mark.end) || !settled(mark.opacity)) return true;
    }
    return false;
  }

  /** The marks to draw now, in time order, without marks that are fully clipped or invisible. */
  marks(): BreakMark[] {
    const result: BreakMark[] = [];
    for (const [key, mark] of this.#marks) {
      let top = mark.top.value;
      let end = mark.end.value;
      if (mark.status === "planned") {
        const owner = this.#owners.get(mark.ownerId);
        if (!owner) continue;
        top = Math.max(top, owner.startMinute);
        end = Math.min(end, owner.endMinute);
      }
      const opacity = Math.min(1, Math.max(0, mark.opacity.value));
      if (end <= top || opacity <= 0) continue;
      result.push({ key, topMinute: top, heightMinutes: end - top, phase: mark.phase, status: mark.status, opacity });
    }
    return result.sort((left, right) => left.topMinute - right.topMinute);
  }

  /** Forgets every mark and owner, so the next set of bands appears in place, as for another day. */
  clear(): void {
    this.#marks.clear();
    this.#owners = new Map();
  }
}

/** Moves a mark and its destination by the same amount, as when its owner moved. */
function shift(mark: MarkState, minutes: number): void {
  if (minutes === 0) return;
  mark.top.value += minutes;
  mark.top.target += minutes;
  mark.end.value += minutes;
  mark.end.target += minutes;
}
