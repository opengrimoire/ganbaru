import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { parseFocusDeviceActivity, type FocusOtherDeviceActivity } from "$lib/pomodoro/device-activity";

/**
 * Read what other linked devices are focusing on, validated before the UI uses it.
 *
 * Kept apart from `$lib/api/focus` because Focus overlay windows import that module and
 * may reach only the commands their narrow permission set allows.
 */
export async function readFocusDeviceActivity(): Promise<FocusOtherDeviceActivity[]> {
  const dbUrl = await ensureDbUrl();
  return parseFocusDeviceActivity(await invoke<unknown>("focus_device_activity", { dbUrl }));
}
