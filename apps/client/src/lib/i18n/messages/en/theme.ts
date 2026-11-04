export const theme = {
  builtInName: {
    light: "Light default",
    dark: "Dark default",
  },
  sync: {
    conflicts: (count: number) =>
      `${count} ${count === 1 ? "theme was" : "themes were"} edited on two devices. Open ${count === 1 ? "it" : "them"} in the theme settings to choose what to keep.`,
    untitled: "Untitled theme",
    restored: "Theme restored",
    restoreFailed: "The theme could not be restored",
  },
  conflict: {
    badge: "Conflict",
    title: "This theme was edited on two devices",
    hint: "Choose the version to keep. To keep both, duplicate this theme first.",
    loading: "Loading versions",
    loadFailed: "The versions could not be loaded",
    palette: "Event palette",
    shown: "Shown now",
    keepThis: "Keep this",
    useOther: "Use this",
    versionFrom: (device: string, time: string) => `${device}, ${time}`,
    thisDevice: "This device",
    otherDevice: "Another device",
    resolved: "This conflict was already resolved",
    failed: "The conflict could not be resolved",
  },
} as const;
