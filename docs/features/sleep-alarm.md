# Sleep alarm

Sleep alarm is a planned mobile feature for dependable wake-up delivery and an optional transition into the morning diary, Music, and morning distraction rules.

## Platform boundary

The alarm belongs on the phone likely to remain near the user overnight. Desktop does not require the user to leave a computer running to provide this feature.

The Android design requires a native alarm, notification, audio, reboot, permission, and exact-delivery contract before implementation. It remains useful with graceful fallback when exact access is unavailable and never bypasses system silent or Do Not Disturb policy without explicit platform support and user control.

## Morning flow

1. A native alarm delivers at the configured time.
2. The user snoozes or dismisses it through explicit controls.
3. Dismissal can open the optional [morning diary](diary.md).
4. An optional wake-up [Music](music/README.md) assignment can start.
5. Planned [morning distraction rules](distractions/rules-and-activation.md) can activate.

Every follow-up is separately configurable. Dismissing an alarm does not force diary disclosure, Music, or blocker activation that the user did not enable.

## Evening flow

Setting the next alarm can offer the optional evening diary and a neutral wind-down review. The alarm remains saved even if the diary is skipped.

## Sleep-window estimate

Alarm configuration time is not bedtime and must not be labeled sleep duration. The feature can record:

- Explicit intended bedtime or sleep-window start.
- Alarm time.
- Snooze and dismissal time.
- Optional user correction and self-reported sleep quality.

The interval from intended bedtime to dismissal is a rough sleep-window estimate, not measured sleep. It does not know sleep onset, wake periods, or sleep quality. UI and AI context must preserve that distinction.

## Privacy

Sleep intent and diary data remain private personal information. Project participants, reports, channels, and agents do not receive it through project access. An explicit personal planning action can use a coarse derived capacity signal after showing what will be shared with the selected provider.
