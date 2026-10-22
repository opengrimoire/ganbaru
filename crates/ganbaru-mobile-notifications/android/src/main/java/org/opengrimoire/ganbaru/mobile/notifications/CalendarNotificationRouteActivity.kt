package org.opengrimoire.ganbaru.mobile.notifications

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * Non-exported target of Calendar reminder taps. Only the app's own immutable PendingIntents can
 * start it, so the event it records cannot come from another app; the exported launcher activity
 * ignores routing extras.
 */
class CalendarNotificationRouteActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    if (savedInstanceState == null) {
      CalendarNotificationScheduler.captureAction(this, intent)
      packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
        launch.flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        startActivity(launch)
      }
    }
    finish()
  }
}
