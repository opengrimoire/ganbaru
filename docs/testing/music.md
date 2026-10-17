# Music desktop acceptance

**Status: Partial.** Automated tests do not establish real audio or operating-system lifecycle behavior; the cases below are checked in the installed desktop app. Behavior is specified in [Music playback](../features/music/playback.md). Android checks live in the [Android acceptance matrix](android.md#music). Background sounds are desktop-only.

- Suspend WebView JavaScript while local audio advances, a timed Calendar event starts or ends, and a Focus phase changes. Queue position, background sounds, tray, and media keys stay correct afterward.
- Overlap two timed events, including a moved recurrence override and an excluded original. The earliest-ending eligible occurrence wins, and a running Focus run blocks Calendar takeover.
- Enter Review across an automatic activation and return; review playback is preserved and automation reconciles afterward. A manual override stays until the next accepted activation.
- Assign only a background sound; main queue ownership is unchanged. Remove a referenced sound; the error is visible and unrelated music continues.
- Restart after automatic background playback; saved intent alone does not start audio.
- Delay source preparation during a phase change, vault switch, and handoff; obsolete preparation never becomes audible and handoff waits or reports its timeout.
