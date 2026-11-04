package org.opengrimoire.ganbaru.mobile.distractions

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * Non-exported target of blocked-app notification taps. Only the app's own immutable
 * PendingIntents can start it, so the settings target it records cannot come from another app;
 * the exported launcher activity ignores routing extras.
 */
class DistractionsNotificationRouteActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    if (savedInstanceState == null) {
      DistractionsNotificationActionStore.capture(this, intent)
      packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
        launch.flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        startActivity(launch)
      }
    }
    finish()
  }
}
