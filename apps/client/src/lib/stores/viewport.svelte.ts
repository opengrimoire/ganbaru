import {
  classifyViewport,
  isSizeClassAtLeast,
  type ViewportSizeClass,
} from "$lib/utils/responsive";
import {
  ANDROID_INSETS_EVENT_NAME,
  calculateKeyboardInset,
  keyboardSafeAreaOverlap,
  parseAndroidKeyboardHeight,
  parseAndroidSystemBarInsets,
  resolvePointerCapabilities,
  viewportOrientation,
  type SystemBarInsets,
  type ViewportOrientation,
} from "$lib/viewport";
import { KEYBOARD_SURFACE_ATTRIBUTE, keyboardSurfaceMode, keyboardSurfaceOf } from "$lib/mobile/keyboard";

const DEFAULT_WIDTH = 1200;
const DEFAULT_HEIGHT = 800;

let width = $state(DEFAULT_WIDTH);
let height = $state(DEFAULT_HEIGHT);
let layoutWidth = $state(DEFAULT_WIDTH);
let layoutHeight = $state(DEFAULT_HEIGHT);
let offsetTop = $state(0);
let offsetLeft = $state(0);
let keyboardInset = $state(0);
let nativeKeyboardHeight = $state(0);
let visibleHeight = $state(DEFAULT_HEIGHT);
let coarsePointer = $state(false);
let hoverAvailable = $state(true);
let initialized = false;
let keyboardSurface: HTMLElement | null = null;

const sizeClass = $derived(classifyViewport(width, height));
const isShort = $derived(height < 480);
const canShowDenseChrome = $derived(isSizeClassAtLeast(sizeClass, "regular") && !isShort);
const orientation = $derived(viewportOrientation(width, height));
const keyboardOpen = $derived(keyboardInset > 0 || nativeKeyboardHeight > 0);

interface ViewportSnapshot {
  width: number;
  height: number;
  layoutWidth: number;
  layoutHeight: number;
  offsetTop: number;
  offsetLeft: number;
  keyboardInset: number;
  coarsePointer: boolean;
  hoverAvailable: boolean;
}

function readMediaQuery(query: string): MediaQueryList | null {
  if (typeof window.matchMedia !== "function") return null;
  try {
    return window.matchMedia(query);
  } catch {
    return null;
  }
}

function readViewport(): ViewportSnapshot {
  if (typeof window === "undefined") {
    return {
      width: DEFAULT_WIDTH,
      height: DEFAULT_HEIGHT,
      layoutWidth: DEFAULT_WIDTH,
      layoutHeight: DEFAULT_HEIGHT,
      offsetTop: 0,
      offsetLeft: 0,
      keyboardInset: 0,
      coarsePointer: false,
      hoverAvailable: true,
    };
  }
  const visualViewport = window.visualViewport;
  const nextLayoutWidth = window.innerWidth;
  const nextLayoutHeight = window.innerHeight;
  const nextOffsetTop = visualViewport?.offsetTop ?? 0;
  const pointerCapabilities = resolvePointerCapabilities(
    readMediaQuery("(pointer: coarse)"),
    readMediaQuery("(hover: hover)"),
    window.navigator.maxTouchPoints,
  );
  return {
    width: visualViewport?.width ?? nextLayoutWidth,
    height: visualViewport?.height ?? nextLayoutHeight,
    layoutWidth: nextLayoutWidth,
    layoutHeight: nextLayoutHeight,
    offsetTop: nextOffsetTop,
    offsetLeft: visualViewport?.offsetLeft ?? 0,
    keyboardInset: visualViewport
      ? calculateKeyboardInset(nextLayoutHeight, visualViewport.height, nextOffsetTop)
      : 0,
    ...pointerCapabilities,
  };
}

function readAndroidSystemBarInsets(): SystemBarInsets | null {
  try {
    return parseAndroidSystemBarInsets(window.GanbaruAndroidInsets?.systemBars());
  } catch {
    return null;
  }
}

function readAndroidKeyboardHeight(): number {
  try {
    return parseAndroidKeyboardHeight(window.GanbaruAndroidInsets?.keyboard()) ?? 0;
  } catch {
    return 0;
  }
}

function applyAndroidSystemBarInsets(root: HTMLElement, insets: SystemBarInsets | null): void {
  if (insets === null) return;
  root.style.setProperty("--safe-area-top", `${insets.top}px`);
  root.style.setProperty("--safe-area-right", `${insets.right}px`);
  root.style.setProperty("--safe-area-bottom", `${insets.bottom}px`);
  root.style.setProperty("--safe-area-left", `${insets.left}px`);
}

/** Mark `next` as the surface following the keyboard, measuring whether it lifts before it starts to follow. */
function applyKeyboardSurface(next: HTMLElement | null, viewportHeight: number): void {
  if (next === keyboardSurface) return;
  keyboardSurface?.removeAttribute(KEYBOARD_SURFACE_ATTRIBUTE);
  keyboardSurface = next;
  next?.setAttribute(KEYBOARD_SURFACE_ATTRIBUTE, keyboardSurfaceMode(next, viewportHeight));
}

/**
 * Read the viewport and publish it as CSS variables. While the soft keyboard is open, the surface holding focus
 * follows it: `--keyboard-inset` is how far the keyboard reaches above the bottom safe area, the page shrinks by
 * `--page-keyboard-inset` when it holds focus, and a modal surface holding focus takes `--keyboard-surface-height`
 * as its `--visual-viewport-height`. Everything behind that surface keeps its size.
 */
function refreshViewport(): void {
  const next = readViewport();
  const systemBars = readAndroidSystemBarInsets();
  const androidKeyboard = readAndroidKeyboardHeight();
  const overlap = keyboardSafeAreaOverlap(androidKeyboard, systemBars?.bottom ?? 0);
  const nextKeyboardInset = Math.max(next.keyboardInset, overlap);
  const surfaceHeight = Math.max(0, next.height - overlap);
  width = next.width;
  height = next.height;
  layoutWidth = next.layoutWidth;
  layoutHeight = next.layoutHeight;
  offsetTop = next.offsetTop;
  offsetLeft = next.offsetLeft;
  keyboardInset = nextKeyboardInset;
  nativeKeyboardHeight = androidKeyboard;
  visibleHeight = Math.max(0, next.height - androidKeyboard);
  coarsePointer = next.coarsePointer;
  hoverAvailable = next.hoverAvailable;

  const keyboardIsOpen = nextKeyboardInset > 0 || androidKeyboard > 0;
  applyKeyboardSurface(keyboardIsOpen ? keyboardSurfaceOf(document.activeElement) : null, next.layoutHeight);

  const root = document.documentElement;
  root.style.setProperty("--visual-viewport-width", `${next.width}px`);
  root.style.setProperty("--visual-viewport-height", `${keyboardSurface ? next.height : surfaceHeight}px`);
  root.style.setProperty("--visual-viewport-offset-top", `${next.offsetTop}px`);
  root.style.setProperty("--visual-viewport-offset-left", `${next.offsetLeft}px`);
  root.style.setProperty("--keyboard-inset", `${nextKeyboardInset}px`);
  root.style.setProperty("--keyboard-surface-height", `${surfaceHeight}px`);
  root.style.setProperty("--page-keyboard-inset", `${keyboardSurface ? 0 : nextKeyboardInset}px`);
  applyAndroidSystemBarInsets(root, systemBars);
  root.dataset.orientation = viewportOrientation(next.width, next.height);
  root.dataset.input = next.coarsePointer ? "coarse" : "fine";
}

function initializeViewportTracking(): void {
  if (initialized || typeof window === "undefined") return;
  initialized = true;
  refreshViewport();

  let rafId = 0;
  const onResize = () => {
    if (rafId !== 0) return;
    rafId = requestAnimationFrame(() => {
      rafId = 0;
      refreshViewport();
    });
  };

  window.addEventListener("resize", onResize);
  window.addEventListener(ANDROID_INSETS_EVENT_NAME, onResize);
  window.visualViewport?.addEventListener("resize", onResize);
  window.visualViewport?.addEventListener("scroll", onResize);
  document.addEventListener("focusin", onResize);
  document.addEventListener("focusout", onResize);
  for (const query of ["(pointer: coarse)", "(hover: hover)"]) {
    const mediaQuery = readMediaQuery(query);
    if (typeof mediaQuery?.addEventListener === "function") {
      mediaQuery.addEventListener("change", onResize);
    } else if (typeof mediaQuery?.addListener === "function") {
      mediaQuery.addListener(onResize);
    }
  }
}

export function getViewport() {
  initializeViewportTracking();
  return {
    get width(): number {
      return width;
    },
    get height(): number {
      return height;
    },
    get layoutWidth(): number {
      return layoutWidth;
    },
    get layoutHeight(): number {
      return layoutHeight;
    },
    get offsetTop(): number {
      return offsetTop;
    },
    get offsetLeft(): number {
      return offsetLeft;
    },
    /** How far an open soft keyboard reaches above the bottom safe area, or zero while it is hidden. */
    get keyboardInset(): number {
      return keyboardInset;
    },
    /** Whether a soft keyboard is open, including while it slides in or out. */
    get keyboardOpen(): boolean {
      return keyboardOpen;
    },
    /** Viewport coordinate of the top edge of an open soft keyboard, or the visual viewport bottom while it is hidden. */
    get visibleBottom(): number {
      return offsetTop + visibleHeight;
    },
    get orientation(): ViewportOrientation {
      return orientation;
    },
    get coarsePointer(): boolean {
      return coarsePointer;
    },
    get hoverAvailable(): boolean {
      return hoverAvailable;
    },
    get sizeClass(): ViewportSizeClass {
      return sizeClass;
    },
    get isShort(): boolean {
      return isShort;
    },
    get canShowDenseChrome(): boolean {
      return canShowDenseChrome;
    },
    atLeast(minimum: ViewportSizeClass): boolean {
      return isSizeClassAtLeast(sizeClass, minimum);
    },
    below(minimum: ViewportSizeClass): boolean {
      return !isSizeClassAtLeast(sizeClass, minimum);
    },
  };
}
