const STORAGE_KEY = "ganbaru-ai-calendar-zoom";
export const CALENDAR_ZOOM_FRAME_EVENT = "ganbaru-ai-calendar-zoom-frame";
const DEFAULT_HOUR_HEIGHT = 50;
export const CALENDAR_ZOOM_PERCENT_LEVELS = [50, 75, 100, 125, 150, 200, 300, 400] as const;
const ZOOM_LEVELS: readonly number[] = CALENDAR_ZOOM_PERCENT_LEVELS.map(
  (percent) => (DEFAULT_HOUR_HEIGHT * percent) / 100,
);
const DEFAULT_INDEX = CALENDAR_ZOOM_PERCENT_LEVELS.indexOf(100);
const ANIMATION_DURATION_MS = 150;

function findClosestIndex(height: number): number {
  let best = 0;
  let bestDistance = Math.abs(ZOOM_LEVELS[0] - height);
  for (let i = 1; i < ZOOM_LEVELS.length; i++) {
    const distance = Math.abs(ZOOM_LEVELS[i] - height);
    if (distance < bestDistance) {
      best = i;
      bestDistance = distance;
    }
  }
  return best;
}

function loadSaved(): number {
  if (typeof localStorage === "undefined") return ZOOM_LEVELS[DEFAULT_INDEX];
  const saved = localStorage.getItem(STORAGE_KEY);
  if (!saved) return ZOOM_LEVELS[DEFAULT_INDEX];
  const parsed = parseFloat(saved);
  if (Number.isNaN(parsed)) return ZOOM_LEVELS[DEFAULT_INDEX];
  return parsed;
}

function deriveGridMinutes(height: number): number {
  if (height >= 120) return 5;
  if (height >= 60) return 10;
  if (height >= 40) return 15;
  return 30;
}

export function calendarZoomGridMinutesForPercent(percent: number): number {
  return deriveGridMinutes((DEFAULT_HOUR_HEIGHT * percent) / 100);
}

function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}

let levelIndex = findClosestIndex(loadSaved());
let hourHeight = $state(ZOOM_LEVELS[levelIndex]);

let scrollRef: HTMLElement | null = null;
let stickyHeaderHeight = 0;
let zoomRaf = 0;
let gestureActive = $state(false);
let commitTimer = 0;

// Animation state
let animating = $state(false);
let animStartTime = 0;
let animFromHeight = 0;
let animToHeight = 0;
let animFromScroll = 0;
let animToScroll = 0;

function computeScrollForHeight(
  height: number,
  centerMinute: number,
  viewportHeight: number,
): number {
  const centerOffset = (viewportHeight - stickyHeaderHeight) / 2;
  const targetScrollTop = (centerMinute / 60) * height - centerOffset;
  const maxScroll = Math.max(0, 24 * height - viewportHeight);
  return Math.max(0, Math.min(targetScrollTop, maxScroll));
}

function animateTick() {
  const scrollContainer = scrollRef;
  if (!scrollContainer) {
    zoomRaf = 0;
    animating = false;
    return;
  }

  const elapsed = performance.now() - animStartTime;
  const t = Math.min(1, elapsed / ANIMATION_DURATION_MS);
  const eased = easeOutCubic(t);

  const currentHeight = animFromHeight + (animToHeight - animFromHeight) * eased;
  const currentScroll = animFromScroll + (animToScroll - animFromScroll) * eased;

  scrollContainer.style.setProperty("--hour-h", String(currentHeight));
  scrollContainer.scrollTop = currentScroll;
  scrollContainer.dispatchEvent(new CustomEvent(CALENDAR_ZOOM_FRAME_EVENT));

  if (t < 1) {
    zoomRaf = requestAnimationFrame(animateTick);
  } else {
    zoomRaf = 0;
    animating = false;
  }
}

function startOrRetargetAnimation(toHeight: number) {
  const scrollContainer = scrollRef;
  if (!scrollContainer) return;

  const viewportHeight = scrollContainer.clientHeight;
  const centerOffset = (viewportHeight - stickyHeaderHeight) / 2;

  // Start from the rendered height, which may be mid-animation.
  const fromHeight = getRenderedHourHeight(scrollContainer);
  const fromScroll = scrollContainer.scrollTop;

  // Keep the minute at the viewport center fixed across the zoom.
  const centerMinute = (fromScroll + centerOffset) / fromHeight * 60;
  const toScroll = computeScrollForHeight(toHeight, centerMinute, viewportHeight);

  animFromHeight = fromHeight;
  animToHeight = toHeight;
  animFromScroll = fromScroll;
  animToScroll = toScroll;
  animStartTime = performance.now();
  animating = true;

  if (!zoomRaf) {
    zoomRaf = requestAnimationFrame(animateTick);
  }
}

function commitZoom() {
  commitTimer = 0;

  if (animating) {
    commitTimer = window.setTimeout(commitZoom, 30);
    return;
  }

  gestureActive = false;
  const scrollContainer = scrollRef;
  const finalHeight = ZOOM_LEVELS[levelIndex];

  // Snap to the exact level in case the last frame landed short of it.
  if (scrollContainer) {
    scrollContainer.style.setProperty("--hour-h", String(finalHeight));
    scrollContainer.dispatchEvent(new CustomEvent(CALENDAR_ZOOM_FRAME_EVENT));
  }

  hourHeight = finalHeight;

  // Notify components that update only after a zoom settles.
  if (scrollContainer) {
    scrollContainer.dispatchEvent(new CustomEvent("zoomcommit", { bubbles: true }));
  }
}

function persist(height: number) {
  localStorage.setItem(STORAGE_KEY, String(height));
}

function getRenderedHourHeight(scrollContainer: HTMLElement): number {
  const currentHeightValue = scrollContainer.style.getPropertyValue("--hour-h");
  const currentHeight = currentHeightValue ? parseFloat(currentHeightValue) : Number.NaN;
  return Number.isFinite(currentHeight) && currentHeight > 0 ? currentHeight : hourHeight;
}

function setZoomIndex(targetIndex: number): void {
  if (targetIndex === levelIndex) return;

  levelIndex = targetIndex;
  const nextHeight = ZOOM_LEVELS[targetIndex];
  persist(nextHeight);

  if (scrollRef) {
    gestureActive = true;
    startOrRetargetAnimation(nextHeight);
    clearTimeout(commitTimer);
    commitTimer = window.setTimeout(commitZoom, ANIMATION_DURATION_MS + 50);
  } else {
    hourHeight = nextHeight;
  }
}

export function getCalendarZoom() {
  return {
    get hourHeight() {
      return hourHeight;
    },
    get gridMinutes() {
      return deriveGridMinutes(hourHeight);
    },
    get isAnimating() {
      return gestureActive || animating;
    },
    /** Register scroll container so buttons/keyboard can animate. */
    registerScrollContainer(container: HTMLElement, stickyHeight: number) {
      scrollRef = container;
      stickyHeaderHeight = stickyHeight;
    },
    reset() {
      const defaultHeight = ZOOM_LEVELS[DEFAULT_INDEX];
      if (zoomRaf) {
        cancelAnimationFrame(zoomRaf);
        zoomRaf = 0;
      }
      clearTimeout(commitTimer);
      commitTimer = 0;
      animating = false;
      levelIndex = DEFAULT_INDEX;
      persist(defaultHeight);

      if (scrollRef && getRenderedHourHeight(scrollRef) !== defaultHeight) {
        gestureActive = true;
        startOrRetargetAnimation(defaultHeight);
        commitTimer = window.setTimeout(commitZoom, ANIMATION_DURATION_MS + 50);
      } else {
        gestureActive = false;
        hourHeight = defaultHeight;
        if (scrollRef) {
          scrollRef.style.setProperty("--hour-h", String(defaultHeight));
          scrollRef.dispatchEvent(new CustomEvent(CALENDAR_ZOOM_FRAME_EVENT));
        }
      }
    },
    get canZoomIn() {
      return hourHeight < ZOOM_LEVELS[ZOOM_LEVELS.length - 1];
    },
    get canZoomOut() {
      return hourHeight > ZOOM_LEVELS[0];
    },
    get isDefault() {
      return hourHeight === ZOOM_LEVELS[DEFAULT_INDEX];
    },
    /** Percent relative to the default hour height. Default row = 100%. */
    get zoomPercent() {
      return Math.round((hourHeight / ZOOM_LEVELS[DEFAULT_INDEX]) * 100);
    },
    setZoomPercent(percent: number) {
      setZoomIndex(findClosestIndex((DEFAULT_HOUR_HEIGHT * percent) / 100));
    },
    /** Zoom one level with smooth animation. */
    zoomStep(direction: 1 | -1) {
      const targetIndex = Math.max(
        0,
        Math.min(ZOOM_LEVELS.length - 1, levelIndex + direction),
      );
      setZoomIndex(targetIndex);
    },
  };
}
