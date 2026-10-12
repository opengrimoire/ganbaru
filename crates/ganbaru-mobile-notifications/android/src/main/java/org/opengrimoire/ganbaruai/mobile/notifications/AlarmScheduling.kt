package org.opengrimoire.ganbaruai.mobile.notifications

import android.app.AlarmManager
import android.app.PendingIntent

internal fun AlarmManager.scheduleRtcWakeupAllowingIdle(
  triggerAtMillis: Long,
  operation: PendingIntent,
) {
  if (ExactAlarmCapability.isGranted(this)) {
    setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAtMillis, operation)
  } else {
    setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAtMillis, operation)
  }
}
