import { invoke } from "@tauri-apps/api/core";

export const THEME_JSON_FILE_SAVE_AVAILABLE = true;

export interface ThemeJsonWriteOutcome {
  readonly saved: boolean;
  readonly destination: "downloads" | null;
  readonly fileName: string | null;
}

/** Read theme JSON through the platform-native document picker. */
export function pickThemeJsonFile(): Promise<string | null> {
  return invoke<string | null>("vault_pick_and_read_theme_json");
}

/** Save theme JSON through the platform-native document picker. */
export function saveThemeJsonFile(
  defaultName: string,
  contents: string,
): Promise<ThemeJsonWriteOutcome> {
  return invoke<ThemeJsonWriteOutcome>("vault_pick_and_write_theme_json", {
    defaultName,
    contents,
  });
}
