/**
 * Mobile interface-scale levels. The lower bound keeps controls usable on a
 * phone, while the upper levels provide meaningful whole-interface
 * enlargement without relying only on text scaling.
 */
export const APP_ZOOM_LEVELS: readonly number[] = Object.freeze([
  0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5,
]);

const STORAGE_KEY = "ganbaru-zoom";
const DEFAULT_INDEX = APP_ZOOM_LEVELS.indexOf(1);

function findClosestIndex(level: number): number {
  let best = 0;
  let bestDistance = Math.abs(APP_ZOOM_LEVELS[0] - level);
  for (let index = 1; index < APP_ZOOM_LEVELS.length; index++) {
    const distance = Math.abs(APP_ZOOM_LEVELS[index] - level);
    if (distance < bestDistance) {
      best = index;
      bestDistance = distance;
    }
  }
  return best;
}

function loadSavedIndex(): number {
  if (typeof localStorage === "undefined") return DEFAULT_INDEX;
  const saved = localStorage.getItem(STORAGE_KEY);
  if (!saved) return DEFAULT_INDEX;
  const parsed = Number.parseFloat(saved);
  return Number.isFinite(parsed) ? findClosestIndex(parsed) : DEFAULT_INDEX;
}

function applyZoom(level: number): void {
  if (typeof document === "undefined") return;
  document.documentElement.style.zoom = String(level);
  document.documentElement.style.setProperty(
    "--mobile-interface-scale-inverse",
    String(1 / level),
  );
}

function persist(level: number): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(STORAGE_KEY, String(level));
}

const initialIndex = loadSavedIndex();
let index = $state(initialIndex);
applyZoom(APP_ZOOM_LEVELS[initialIndex]);

/** Return the mobile interface-scale controller. */
export function getZoom() {
  return {
    get level(): number {
      return APP_ZOOM_LEVELS[index];
    },
    get percent(): number {
      return Math.round(APP_ZOOM_LEVELS[index] * 100);
    },
    get canZoomIn(): boolean {
      return index < APP_ZOOM_LEVELS.length - 1;
    },
    get canZoomOut(): boolean {
      return index > 0;
    },
    get isDefault(): boolean {
      return index === DEFAULT_INDEX;
    },
    zoomIn(): void {
      if (index >= APP_ZOOM_LEVELS.length - 1) return;
      index++;
      persist(APP_ZOOM_LEVELS[index]);
      applyZoom(APP_ZOOM_LEVELS[index]);
    },
    zoomOut(): void {
      if (index <= 0) return;
      index--;
      persist(APP_ZOOM_LEVELS[index]);
      applyZoom(APP_ZOOM_LEVELS[index]);
    },
    reset(): void {
      index = DEFAULT_INDEX;
      persist(APP_ZOOM_LEVELS[index]);
      applyZoom(APP_ZOOM_LEVELS[index]);
    },
    setLevel(level: number): void {
      const nextIndex = findClosestIndex(level);
      if (nextIndex === index) return;
      index = nextIndex;
      persist(APP_ZOOM_LEVELS[index]);
      applyZoom(APP_ZOOM_LEVELS[index]);
    },
    reapply(): void {
      applyZoom(APP_ZOOM_LEVELS[index]);
    },
  };
}
