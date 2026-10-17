export const BUILD_PLATFORMS = ["linux", "windows", "macos", "android", "ios"] as const;
export type BuildPlatform = (typeof BUILD_PLATFORMS)[number];

export type DesktopBuildPlatform = Extract<BuildPlatform, "linux" | "windows" | "macos">;
export type PlatformShell = "desktop" | "mobile";

export const PLATFORM_CAPABILITIES = [
  "view.calendar",
  "view.projects",
  "view.notes",
  "view.chat",
  "view.music",
  "chat.local-execution",
  "content.managed-image-downloads",
  "notes.external-image-references",
  "music.context-assignments",
  "music.local-file-reveal",
  "music.local-item-repair",
  "music.local-root-relink-plans",
  "music.local-root-reselection",
  "music.soundscapes",
  "notifications.native-scheduling",
  "pomodoro.native-idle-detection",
  "projects.working-folders",
  "storage.native-file-picker",
  "notes.file-export",
  "runtime.desktop-pomodoro-effects",
  "system.android-back",
  "window.desktop-title-bar",
  "window.detached-views",
] as const;
export type PlatformCapability = (typeof PLATFORM_CAPABILITIES)[number];

export interface PlatformProfile {
  readonly platform: BuildPlatform;
  readonly shell: PlatformShell;
  readonly capabilities: readonly PlatformCapability[];
}

export interface PlatformDatasetTarget {
  readonly dataset: {
    platform?: string;
    shell?: string;
  };
}

const BUILD_PLATFORM_SET = new Set<string>(BUILD_PLATFORMS);
const MOBILE_BUILD_PLATFORMS = new Set<BuildPlatform>(["android", "ios"]);

const DESKTOP_CAPABILITIES = Object.freeze([
  "view.calendar",
  "view.projects",
  "view.notes",
  "view.chat",
  "view.music",
  "chat.local-execution",
  "content.managed-image-downloads",
  "music.context-assignments",
  "music.local-file-reveal",
  "music.local-item-repair",
  "music.local-root-relink-plans",
  "music.soundscapes",
  "notifications.native-scheduling",
  "pomodoro.native-idle-detection",
  "projects.working-folders",
  "storage.native-file-picker",
  "notes.file-export",
  "runtime.desktop-pomodoro-effects",
  "window.desktop-title-bar",
  "window.detached-views",
] satisfies PlatformCapability[]);
const BASE_MOBILE_CAPABILITIES = Object.freeze([
  "view.calendar",
  "view.projects",
  "view.notes",
  "view.chat",
] satisfies PlatformCapability[]);
const ANDROID_CAPABILITIES = Object.freeze([
  ...BASE_MOBILE_CAPABILITIES,
  "view.music",
  "music.context-assignments",
  "music.local-root-reselection",
  "notifications.native-scheduling",
  "system.android-back",
] satisfies PlatformCapability[]);

export const PLATFORM_CAPABILITY_REGISTRY = Object.freeze({
  linux: DESKTOP_CAPABILITIES,
  windows: DESKTOP_CAPABILITIES,
  macos: DESKTOP_CAPABILITIES,
  android: ANDROID_CAPABILITIES,
  ios: BASE_MOBILE_CAPABILITIES,
} satisfies Readonly<Record<BuildPlatform, readonly PlatformCapability[]>>);

/** Return whether an unknown value is a supported Tauri build platform. */
export function isBuildPlatform(value: unknown): value is BuildPlatform {
  return typeof value === "string" && BUILD_PLATFORM_SET.has(value);
}

/** Resolve an injected build target without ever falling back to a mobile shell. */
export function resolveBuildPlatform(
  value: unknown,
  desktopFallback: DesktopBuildPlatform = "linux",
): BuildPlatform {
  return isBuildPlatform(value) ? value : desktopFallback;
}

/** Create the immutable frontend profile for a validated build platform. */
export function platformProfileFor(platform: BuildPlatform): PlatformProfile {
  const mobile = MOBILE_BUILD_PLATFORMS.has(platform);
  return Object.freeze({
    platform,
    shell: mobile ? "mobile" : "desktop",
    capabilities: PLATFORM_CAPABILITY_REGISTRY[platform],
  });
}

/** Return whether a platform profile exposes a frontend capability. */
export function platformHasCapability(
  profile: PlatformProfile,
  capability: PlatformCapability,
): boolean {
  return profile.capabilities.includes(capability);
}

const injectedBuildPlatform = typeof __GANBARU_BUILD_PLATFORM__ === "undefined"
  ? undefined
  : __GANBARU_BUILD_PLATFORM__;

export const BUILD_PLATFORM = resolveBuildPlatform(injectedBuildPlatform);
export const BUILD_PLATFORM_PROFILE = platformProfileFor(BUILD_PLATFORM);

/** Stamp the platform contract onto the root document before the Svelte shell mounts. */
export function applyPlatformProfileToDocument(
  profile: PlatformProfile = BUILD_PLATFORM_PROFILE,
  target: PlatformDatasetTarget = document.documentElement,
): void {
  target.dataset.platform = profile.platform;
  target.dataset.shell = profile.shell;
}
