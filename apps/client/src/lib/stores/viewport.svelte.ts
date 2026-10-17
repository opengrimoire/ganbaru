import {
  classifyViewport,
  isSizeClassAtLeast,
  type ViewportSizeClass,
} from "$lib/utils/responsive";
import {
  ANDROID_INSETS_EVENT_NAME,
  calculateKeyboardInset,
  parseAndroidKeyboardHeight,
  parseAndroidSystemBarInsets,
  resolvePointerCapabilities,
  viewportOrientation,
  type SystemBarInsets,
  type ViewportOrientation,
} from "$lib/viewport";

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
let coarsePointer = $state(false);
let hoverAvailable = $state(true);
let initialized = false;

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

function applyAndroidSystemBarInsets(root: HTMLElement): void {
  const insets = readAndroidSystemBarInsets();
  if (insets === null) return;
  root.style.setProperty("--safe-area-top", `${insets.top}px`);
  root.style.setProperty("--safe-area-right", `${insets.right}px`);
  root.style.setProperty("--safe-area-bottom", `${insets.bottom}px`);
  root.style.setProperty("--safe-area-left", `${insets.left}px`);
}

function refreshViewport(): void {
  const next = readViewport();
  width = next.width;
  height = next.height;
  layoutWidth = next.layoutWidth;
  layoutHeight = next.layoutHeight;
  offsetTop = next.offsetTop;
  offsetLeft = next.offsetLeft;
  keyboardInset = next.keyboardInset;
  nativeKeyboardHeight = readAndroidKeyboardHeight();
  coarsePointer = next.coarsePointer;
  hoverAvailable = next.hoverAvailable;

  const root = document.documentElement;
  root.style.setProperty("--visual-viewport-width", `${next.width}px`);
  root.style.setProperty("--visual-viewport-height", `${next.height}px`);
  root.style.setProperty("--visual-viewport-offset-top", `${next.offsetTop}px`);
  root.style.setProperty("--visual-viewport-offset-left", `${next.offsetLeft}px`);
  root.style.setProperty("--keyboard-inset", `${next.keyboardInset}px`);
  applyAndroidSystemBarInsets(root);
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
    /** Height of a soft keyboard that overlays the page without resizing it; zero where the shell resizes instead. */
    get keyboardInset(): number {
      return keyboardInset;
    },
    /** Whether a soft keyboard is open, whether it overlays the page or the shell resized the page above it. */
    get keyboardOpen(): boolean {
      return keyboardOpen;
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
