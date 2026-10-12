import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/**
 * Single source of truth for the webview zoom level. Steps through a
 * Chrome-like set of discrete levels rather than a continuous slider so
 * each tick produces a crisp, predictable step. The active level persists
 * to localStorage so zoom survives reloads.
 */

const STORAGE_KEY = "ganbaru-ai-zoom";
export const APP_ZOOM_LEVELS: readonly number[] = Object.freeze([
  0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3,
]);
const DEFAULT_INDEX = APP_ZOOM_LEVELS.indexOf(1);

function findClosestIndex(level: number): number {
  let best = 0;
  let bestDistance = Math.abs(APP_ZOOM_LEVELS[0] - level);
  for (let i = 1; i < APP_ZOOM_LEVELS.length; i++) {
    const distance = Math.abs(APP_ZOOM_LEVELS[i] - level);
    if (distance < bestDistance) {
      best = i;
      bestDistance = distance;
    }
  }
  return best;
}

function loadSavedIndex(): number {
  if (typeof localStorage === "undefined") return DEFAULT_INDEX;
  const saved = localStorage.getItem(STORAGE_KEY);
  if (!saved) return DEFAULT_INDEX;
  const parsed = parseFloat(saved);
  if (Number.isNaN(parsed)) return DEFAULT_INDEX;
  return findClosestIndex(parsed);
}

const initialIndex = loadSavedIndex();
let index = $state<number>(initialIndex);

function applyZoom() {
  getCurrentWebviewWindow().setZoom(APP_ZOOM_LEVELS[index]);
}

function persist() {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(STORAGE_KEY, String(APP_ZOOM_LEVELS[index]));
}

// Apply saved zoom at module load so there's no flash at the default level.
// Read the const, not the $state, to keep this outside Svelte reactivity.
if (initialIndex !== DEFAULT_INDEX) {
  getCurrentWebviewWindow().setZoom(APP_ZOOM_LEVELS[initialIndex]);
}

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
    zoomIn() {
      if (index >= APP_ZOOM_LEVELS.length - 1) return;
      index++;
      persist();
      applyZoom();
    },
    zoomOut() {
      if (index <= 0) return;
      index--;
      persist();
      applyZoom();
    },
    reset() {
      index = DEFAULT_INDEX;
      persist();
      applyZoom();
    },
    setLevel(level: number) {
      const nextIndex = findClosestIndex(level);
      if (nextIndex === index) return;
      index = nextIndex;
      persist();
      applyZoom();
    },
    /** Re-assert the current zoom level (counteracts external changes). */
    reapply() {
      applyZoom();
    },
  };
}
