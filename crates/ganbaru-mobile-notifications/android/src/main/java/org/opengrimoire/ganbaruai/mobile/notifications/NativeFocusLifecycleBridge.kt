package org.opengrimoire.ganbaruai.mobile.notifications

import android.os.SystemClock
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject

/** Native Activity evidence. Calendar alarms and notification taps do not update it. */
internal class NativeFocusLifecycleBridge {
  private var channel: Channel? = null
  private var foreground = false
  private var sequence = 0L

  @Synchronized
  fun attach(callback: Channel) {
    channel = callback
    publish()
  }

  @Synchronized
  fun observe(isForeground: Boolean) {
    foreground = isForeground
    sequence += 1
    publish()
  }

  private fun publish() {
    channel?.send(JSObject().apply {
      put("sequence", sequence)
      put("foreground", foreground)
      put("observedAtMs", System.currentTimeMillis())
      put("elapsedRealtimeMs", SystemClock.elapsedRealtime())
    })
  }
}
