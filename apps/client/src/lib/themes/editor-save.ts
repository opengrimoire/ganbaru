import type { UserThemeWrite } from "$lib/api/themes";

/** A part of a saved theme that linked devices replicate and merge on its own. */
export type ThemeSaveGroup = "name" | "icon" | "content";

/** Every group a theme save can change, in display order. */
export const THEME_SAVE_GROUPS: readonly ThemeSaveGroup[] = ["name", "icon", "content"];

/**
 * What saving an editor session should do.
 *
 * - `unchanged`: the session changed nothing, so nothing is written.
 * - `deleted`: the session changed the theme, but the stored theme is gone.
 * - `write`: `write` is the stored theme with the groups the session changed
 *   taken from the editor. `conflicts` lists the groups that another device
 *   also changed to a different value since the session opened.
 */
export type ThemeSavePlan =
  | { kind: "unchanged" }
  | { kind: "deleted" }
  | { kind: "write"; write: UserThemeWrite; conflicts: readonly ThemeSaveGroup[] };

/**
 * How the person settles a save that conflicts with a remote edit: write the
 * editor's groups over the remote ones, or save the editor buffer as a new theme.
 */
export type ThemeSaveChoice = "overwrite" | "copy";

/**
 * The result of saving an editor session.
 *
 * - `saved`: the theme was written, or there was nothing to write.
 * - `conflict`: nothing was written because another device changed `groups`
 *   too; the session stays open until the person picks a {@link ThemeSaveChoice}.
 * - `savedAsCopy`: the buffer became the new theme `id` named `displayName`,
 *   because the original was deleted or because the person chose a copy.
 */
export type ThemeSaveOutcome =
  | { kind: "saved" }
  | { kind: "conflict"; groups: readonly ThemeSaveGroup[] }
  | { kind: "savedAsCopy"; id: string; displayName: string; reason: "deleted" | "copy" };

/** The three versions of a theme a save compares. */
export interface ThemeSaveInputs {
  /** The stored theme when the session opened, or undefined when none was recorded. */
  base: UserThemeWrite | undefined;
  /** The editor buffer. */
  edited: UserThemeWrite;
  /** The stored theme now, or undefined when it was deleted. */
  stored: UserThemeWrite | undefined;
}

function contentKey(theme: UserThemeWrite): string {
  const tokens = [...theme.tokens]
    .map((token) => [token.kind, token.key, token.value, token.isolated] as const)
    .sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
  const palette = [...theme.palette]
    .map((entry) => [entry.slot, entry.value] as const)
    .sort((a, b) => a[0] - b[0]);
  return JSON.stringify([
    theme.blendCanvas,
    theme.derivationEngineVersion,
    theme.calendarDefaultMode,
    theme.calendarDefaultCustom,
    tokens,
    palette,
  ]);
}

/** A comparable value of one group of a theme. */
export function themeSaveGroupKey(theme: UserThemeWrite, group: ThemeSaveGroup): string {
  switch (group) {
    case "name":
      return theme.displayName;
    case "icon":
      return theme.iconLabel;
    case "content":
      return contentKey(theme);
  }
}

function withGroupFrom(
  target: UserThemeWrite,
  source: UserThemeWrite,
  group: ThemeSaveGroup,
): UserThemeWrite {
  switch (group) {
    case "name":
      return { ...target, displayName: source.displayName };
    case "icon":
      return { ...target, iconLabel: source.iconLabel };
    case "content":
      return {
        ...target,
        blendCanvas: source.blendCanvas,
        derivationEngineVersion: source.derivationEngineVersion,
        calendarDefaultMode: source.calendarDefaultMode,
        calendarDefaultCustom: source.calendarDefaultCustom,
        tokens: source.tokens,
        palette: source.palette,
      };
  }
}

/**
 * Decide how an editor session saves an existing theme. Only the groups the
 * session changed are written, so a remote edit to another group survives the
 * save. A group that both the session and another device changed to different
 * values is a conflict the person settles before anything is written. Without
 * a recorded base, the stored theme stands in for it, so nothing conflicts.
 */
export function planThemeSave({ base, edited, stored }: ThemeSaveInputs): ThemeSavePlan {
  const reference = base ?? stored;
  const changed = THEME_SAVE_GROUPS.filter(
    (group) => reference === undefined || themeSaveGroupKey(edited, group) !== themeSaveGroupKey(reference, group),
  );
  if (changed.length === 0) return { kind: "unchanged" };
  if (stored === undefined) return { kind: "deleted" };
  const conflicts = changed.filter((group) => {
    const storedKey = themeSaveGroupKey(stored, group);
    return reference !== undefined
      && storedKey !== themeSaveGroupKey(reference, group)
      && storedKey !== themeSaveGroupKey(edited, group);
  });
  const write = changed.reduce((next, group) => withGroupFrom(next, edited, group), stored);
  return { kind: "write", write, conflicts };
}
