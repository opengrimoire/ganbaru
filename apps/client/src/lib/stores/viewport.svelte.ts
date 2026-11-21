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
import {
  KEYBOARD_REVEAL_MARGIN_PX,
  KEYBOARD_SHIFT_PROPERTY,
  KEYBOARD_SURFACE_ATTRIBUTE,
  keyboardSurfaceMode,
  keyboardSurfaceOf,
  keyboardSurfaceShift,
  type KeyboardSurfaceMode,
  type VerticalSpan,
} from "$lib/mobile/keyboard";

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
/** The surface following the soft keyboard, with its dialog's span measured before it started to follow. */
interface KeyboardSurface {
  layer: HTMLElement;
  mode: KeyboardSurfaceMode;
  dialog: VerticalSpan;
}

let keyboardSurface: KeyboardSurface | null = null;
let bottomChromeHeight = $state(0);
/** System bar and display cutout insets reported by the Android shell; zero where no shell reports them. */
let systemBarInsets = $state<SystemBarInsets>({ top: 0, right: 0, bottom: 0, left: 0 });

const sizeClass = $derived(classifyViewport(width, height));
const isShort = $derived(height < 480);
const canShowDenseChrome = $derived(isSizeClassAtLeast(sizeClass, "regular") && !isShort);
const orientation = $derived(viewportOrientation(width, height));
const keyboardOpen = $derived(keyboardInset > 0 || nativeKeyboardHeight > 0);
const bottomChromeCovered = $derived(keyboardOpen && keyboardInset >= bottomChromeHeight);

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

/** Whether the Android shell reports the keyboard height, so the page follows the keyboard on every frame. */
function hasAndroidInsetsBridge(): boolean {
  return typeof window !== "undefined" && window.GanbaruAndroidInsets !== undefined;
}

/**
 * Mark `next` as the surface following the keyboard, measuring how it follows before it starts to. `viewportHeight`
 * and `safeBottom` are the viewport height and bottom safe area at rest.
 */
function applyKeyboardSurface(next: HTMLElement | null, viewportHeight: number, safeBottom: number): void {
  if (next === (keyboardSurface?.layer ?? null)) return;
  keyboardSurface?.layer.removeAttribute(KEYBOARD_SURFACE_ATTRIBUTE);
  keyboardSurface?.layer.style.removeProperty(KEYBOARD_SHIFT_PROPERTY);
  keyboardSurface = null;
  if (!next) return;
  const dialog = document.activeElement?.closest('[aria-modal="true"]') ?? next;
  const { top, bottom } = dialog.getBoundingClientRect();
  keyboardSurface = { layer: next, mode: keyboardSurfaceMode(dialog, viewportHeight, safeBottom), dialog: { top, bottom } };
  next.setAttribute(KEYBOARD_SURFACE_ATTRIBUTE, keyboardSurface.mode);
}

/** Move a shifted surface up just far enough that its dialog stays clear of a keyboard whose top is `keyboardTop`. */
function applyKeyboardShift(keyboardTop: number, safeTop: number): void {
  if (keyboardSurface?.mode !== "shift") return;
  const shift = keyboardSurfaceShift(
    keyboardSurface.dialog,
    keyboardTop,
    safeTop + KEYBOARD_REVEAL_MARGIN_PX,
    KEYBOARD_REVEAL_MARGIN_PX,
  );
  keyboardSurface.layer.style.setProperty(KEYBOARD_SHIFT_PROPERTY, `${shift}px`);
}

/**
 * Read the viewport and publish it as CSS variables. While the soft keyboard is open, the surface holding focus
 * follows it: `--keyboard-inset` is how far the keyboard reaches above the bottom safe area, the page shrinks by
 * `--page-keyboard-inset` when it holds focus once the keyboard covers the bottom chrome, a lifted modal surface takes `--keyboard-surface-height` as its
 * `--visual-viewport-height`, and a shifted one moves up by its `--keyboard-shift`. Everything behind that surface
 * keeps its size. `--visual-viewport-full-height` ignores the keyboard, so a sheet can paint the space under it and
 * stay solid when the keyboard hides before the shell reports it. On Android the keyboard height comes from the shell
 * alone, which reports it on every frame of the keyboard slide.
 */
function refreshViewport(): void {
  const next = readViewport();
  const systemBars = readAndroidSystemBarInsets();
  const androidKeyboard = readAndroidKeyboardHeight();
  const overlap = keyboardSafeAreaOverlap(androidKeyboard, systemBars?.bottom ?? 0);
  const nextKeyboardInset = hasAndroidInsetsBridge() ? overlap : Math.max(next.keyboardInset, overlap);
  const surfaceHeight = Math.max(0, next.height - overlap);
  const nextVisibleHeight = Math.max(0, next.height - androidKeyboard);
  width = next.width;
  height = next.height;
  layoutWidth = next.layoutWidth;
  layoutHeight = next.layoutHeight;
  offsetTop = next.offsetTop;
  offsetLeft = next.offsetLeft;
  keyboardInset = nextKeyboardInset;
  nativeKeyboardHeight = androidKeyboard;
  visibleHeight = nextVisibleHeight;
  coarsePointer = next.coarsePointer;
  hoverAvailable = next.hoverAvailable;

  const keyboardIsOpen = nextKeyboardInset > 0 || androidKeyboard > 0;
  applyKeyboardSurface(
    keyboardIsOpen ? keyboardSurfaceOf(document.activeElement) : null,
    next.layoutHeight,
    systemBars?.bottom ?? 0,
  );
  applyKeyboardShift(next.offsetTop + nextVisibleHeight, systemBars?.top ?? 0);

  const root = document.documentElement;
  root.style.setProperty("--visual-viewport-width", `${next.width}px`);
  root.style.setProperty("--visual-viewport-height", `${keyboardSurface ? next.height : surfaceHeight}px`);
  root.style.setProperty("--visual-viewport-full-height", `${next.height}px`);
  root.style.setProperty("--visual-viewport-offset-top", `${next.offsetTop}px`);
  root.style.setProperty("--visual-viewport-offset-left", `${next.offsetLeft}px`);
  root.style.setProperty("--keyboard-inset", `${nextKeyboardInset}px`);
  root.style.setProperty("--keyboard-surface-height", `${surfaceHeight}px`);
  const pageFollows = keyboardSurface === null && keyboardIsOpen && nextKeyboardInset >= bottomChromeHeight;
  root.style.setProperty("--page-keyboard-inset", `${pageFollows ? nextKeyboardInset : 0}px`);
  applyAndroidSystemBarInsets(root, systemBars);
  if (systemBars !== null) systemBarInsets = systemBars;
  root.dataset.orientation = viewportOrientation(next.width, next.height);
  root.dataset.input = next.coarsePointer ? "coarse" : "fine";
}

/**
 * Start tracking the viewport, keyboard, and Android system bar insets into root CSS variables. Idempotent; bootstraps
 * call it before mounting any screen so screens shown before the app shell also respect the system bars.
 */
export function initializeViewportTracking(): void {
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
    /**
     * Whether an open soft keyboard covers the bottom chrome set by {@link setKeyboardBottomChrome}. The chrome stays
     * in place under a keyboard that is still sliding over it, and the page starts to follow only once it is covered.
     */
    get bottomChromeCovered(): boolean {
      return bottomChromeCovered;
    },
    /** Viewport coordinate of the top edge of an open soft keyboard, or the visual viewport bottom while it is hidden. */
    get visibleBottom(): number {
      return offsetTop + visibleHeight;
    },
    /** Layout viewport space taken by the system bars and display cutouts, such as the status and navigation bars. */
    get systemBarInsets(): SystemBarInsets {
      return systemBarInsets;
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

/**
 * Set the height of the chrome along the bottom safe area, such as the bottom tab bar, in viewport pixels. The soft
 * keyboard slides over that chrome before the page follows it, so the page content stays still until the keyboard
 * covers the chrome and never drops below where the chrome ends.
 */
export function setKeyboardBottomChrome(height: number): void {
  const next = Number.isFinite(height) ? Math.max(0, height) : 0;
  if (next === bottomChromeHeight) return;
  bottomChromeHeight = next;
  if (initialized) refreshViewport();
}
