import type { Theme } from "$lib/themes";
import type { Translate } from "./translator.svelte";

export function themeDisplayName(theme: Theme, t: Translate): string {
  if (theme.kind !== "builtin") return theme.displayName;
  if (theme.id === "light") return t("theme.builtInName.light");
  if (theme.id === "dark") return t("theme.builtInName.dark");
  return theme.displayName;
}
