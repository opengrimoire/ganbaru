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
  }

  interface GanbaruAndroidAppearanceBridge {
    setLightTheme(lightTheme: boolean): void;
  }

  interface GanbaruAndroidTransitionBridge {
    holdCurrentFrame(): void;
    releaseHeldFrame(): void;
  }

  interface Window {
    GanbaruAndroidInsets?: GanbaruAndroidInsetsBridge;
    GanbaruAndroidAppearance?: GanbaruAndroidAppearanceBridge;
    GanbaruAndroidTransition?: GanbaruAndroidTransitionBridge;
  }

  const __GANBARU_AI_BUILD_REF__: string;
  const __GANBARU_AI_GITHUB_REPOSITORY__: string;
  const __GANBARU_AI_BUILD_PLATFORM__: "linux" | "windows" | "macos" | "android" | "ios";

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
