import { darkTheme, lightTheme, type Theme, type ThemeId } from "./definitions";

/** Inputs for resolving the theme a device shows. */
export interface ActiveThemeInputs {
  /** The device's own choice, or undefined when it has none. */
  activeId: ThemeId | undefined;
  /** The light theme of the portable quick-toggle pair, which may name a missing theme. */
  quickToggleLightId: ThemeId | undefined;
  /** The dark theme of the portable quick-toggle pair, which may name a missing theme. */
  quickToggleDarkId: ThemeId | undefined;
  /** Whether the system color scheme is dark. */
  prefersDark: boolean;
  registry: Readonly<Record<ThemeId, Theme>>;
}

/**
 * A quick-toggle choice when it names an existing theme, otherwise the built-in theme for that slot. A linked device
 * may delete the theme a pair names, so this resolves at read time and writes nothing.
 */
export function resolveQuickToggleThemeId(
  id: ThemeId | undefined,
  slot: "light" | "dark",
  registry: Readonly<Record<ThemeId, Theme>>,
): ThemeId {
  if (id !== undefined && Object.hasOwn(registry, id)) return id;
  return slot === "dark" ? darkTheme.id : lightTheme.id;
}

/**
 * The theme a device shows: its own choice when that theme exists, otherwise the resolved quick-toggle theme that
 * matches the system color scheme. Nothing is written, so the device's choice comes back if the theme does.
 */
export function resolveActiveThemeId(inputs: ActiveThemeInputs): ThemeId {
  const { activeId, registry } = inputs;
  if (activeId !== undefined && Object.hasOwn(registry, activeId)) return activeId;
  return inputs.prefersDark
    ? resolveQuickToggleThemeId(inputs.quickToggleDarkId, "dark", registry)
    : resolveQuickToggleThemeId(inputs.quickToggleLightId, "light", registry);
}
