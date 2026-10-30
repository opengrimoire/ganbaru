/// <reference types="svelte" />
/// <reference types="vite/client" />

// `main-desktop.ts` and `main-mobile.ts` bind @js-temporal/polyfill to
// globalThis at boot so the rest of the app can use `Temporal.*` without
// importing it per module. This declaration tells TypeScript the global
// exists. Replace it with the standard ES type once browsers ship Temporal.
import type { Temporal as TemporalPolyfill } from "@js-temporal/polyfill";

declare global {
  interface GanbaruAndroidInsetsBridge {
    systemBars(): string;
    keyboard(): string;
  }

  interface GanbaruAndroidAppearanceBridge {
    setLightTheme(lightTheme: boolean): void;
    /** Persists the `#rrggbb` surface color painted behind the WebView, including on the next cold start. */
    setWindowBackground(color: string): void;
  }

  interface GanbaruAndroidMediaVolumeBridge {
    /** Device media volume as `index,max`, or empty when apps cannot change it. */
    state(): string;
    /** Sets the media volume to the nearest device step for a level from 0 to 1 and returns the new state. */
    setLevel(level: number): string;
  }

  interface Window {
    GanbaruAndroidInsets?: GanbaruAndroidInsetsBridge;
    GanbaruAndroidAppearance?: GanbaruAndroidAppearanceBridge;
    GanbaruAndroidMediaVolume?: GanbaruAndroidMediaVolumeBridge;
  }

  const __GANBARU_BUILD_REF__: string;
  const __GANBARU_GITHUB_REPOSITORY__: string;
  const __GANBARU_BUILD_PLATFORM__: "linux" | "windows" | "macos" | "android" | "ios";

  // eslint-disable-next-line no-var
  var Temporal: typeof TemporalPolyfill;
  // Expose as a namespace alias too, so call sites can write
  // `Temporal.PlainDate` for both the value and the type.
  namespace Temporal {
    export type PlainDate = TemporalPolyfill.PlainDate;
    export type PlainDateTime = TemporalPolyfill.PlainDateTime;
    export type ZonedDateTime = TemporalPolyfill.ZonedDateTime;
    export type Instant = TemporalPolyfill.Instant;
    export type Duration = TemporalPolyfill.Duration;
  }
}
