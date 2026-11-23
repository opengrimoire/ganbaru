/** How often the status is read again while it is shown and the app is visible. */
const STATUS_REFRESH_MS = 15_000;

/**
 * Runs `onChange` whenever the automatic backup status may have changed. The mobile bundle keeps the Tauri event module
 * out, so this reads again on a timer and whenever the app becomes visible.
 *
 * @param onChange - Called with no arguments when the status should be read again.
 * @returns A promise of the function that stops.
 */
export function listenForAutomaticBackupChanges(onChange: () => void): Promise<() => void> {
  const refreshIfVisible = (): void => {
    if (document.visibilityState === "visible") onChange();
  };
  const timer = setInterval(refreshIfVisible, STATUS_REFRESH_MS);
  document.addEventListener("visibilitychange", refreshIfVisible);
  return Promise.resolve(() => {
    clearInterval(timer);
    document.removeEventListener("visibilitychange", refreshIfVisible);
  });
}
