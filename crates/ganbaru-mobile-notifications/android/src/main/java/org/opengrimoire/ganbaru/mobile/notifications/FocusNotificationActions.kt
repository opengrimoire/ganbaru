package org.opengrimoire.ganbaru.mobile.notifications

import android.app.Activity
import android.app.Notification
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.os.Bundle
import android.os.SystemClock
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject

internal const val FOCUS_NOTIFICATION_RESUME_ACTION = "resume"
private const val FOCUS_ACTION_REQUEST_CODE = 1_500_000_004
private const val FOCUS_ACTION_STORE = "GANBARU_FOCUS_NOTIFICATION_ACTION_STORE"
private const val PENDING_ACTION = "action"
private const val PENDING_RUN_ID = "runId"
private const val PENDING_PHASE_ID = "phaseId"
private const val PENDING_PRESSED_AT = "pressedAtElapsedMs"
private const val EXTRA_ACTION = "ganbaruFocusNotificationAction"
private const val EXTRA_RUN_ID = "ganbaruFocusNotificationRunId"
private const val EXTRA_PHASE_ID = "ganbaruFocusNotificationPhaseId"

/** A pressed action waiting for a cold app start is dropped after this long, so a late launch never resumes. */
private const val PENDING_ACTION_LIFETIME_MS = 60_000L

/** A notification action bound to the run and phase it displayed. Rust revalidates both before acting. */
internal data class FocusNotificationAction(
  val action: String,
  val runId: String,
  val phaseId: String,
) {
  /** Whether the action names a known command and bounded identifiers. */
  fun isValid(): Boolean = action == FOCUS_NOTIFICATION_RESUME_ACTION
    && runId.isNotBlank() && runId.length <= 128
    && phaseId.isNotBlank() && phaseId.length <= 128
}

/** Build the immutable Resume action for a paused ongoing notification. */
internal fun focusResumeAction(
  context: Context,
  label: String,
  icon: Int,
  runId: String,
  phaseId: String,
): Notification.Action {
  val intent = Intent(context, FocusNotificationActionActivity::class.java).apply {
    putExtra(EXTRA_ACTION, FOCUS_NOTIFICATION_RESUME_ACTION)
    putExtra(EXTRA_RUN_ID, runId)
    putExtra(EXTRA_PHASE_ID, phaseId)
  }
  val pendingIntent = PendingIntent.getActivity(
    context,
    FOCUS_ACTION_REQUEST_CODE,
    intent,
    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
  )
  return Notification.Action.Builder(Icon.createWithResource(context, icon), label, pendingIntent)
    .build()
}

/**
 * Process-wide route from notification actions to the Rust Focus owner. While the owner is
 * attached, actions reach it directly; otherwise the latest action waits briefly for the next
 * attachment after a cold start.
 */
internal object FocusNotificationActionBridge {
  private var channel: Channel? = null

  /** Attach the Rust callback and deliver a fresh action pressed before it was ready. */
  @Synchronized
  fun attach(context: Context, callback: Channel) {
    channel = callback
    takePending(context)?.let { send(callback, it) }
  }

  /** Deliver an action now, or keep it for the next attachment. Returns whether it was delivered. */
  @Synchronized
  fun deliver(context: Context, action: FocusNotificationAction): Boolean {
    val callback = channel
    if (callback != null) {
      send(callback, action)
      return true
    }
    store(context).edit()
      .putString(PENDING_ACTION, action.action)
      .putString(PENDING_RUN_ID, action.runId)
      .putString(PENDING_PHASE_ID, action.phaseId)
      .putLong(PENDING_PRESSED_AT, SystemClock.elapsedRealtime())
      .apply()
    return false
  }

  private fun takePending(context: Context): FocusNotificationAction? {
    val preferences = store(context)
    val action = FocusNotificationAction(
      action = preferences.getString(PENDING_ACTION, null) ?: "",
      runId = preferences.getString(PENDING_RUN_ID, null) ?: "",
      phaseId = preferences.getString(PENDING_PHASE_ID, null) ?: "",
    )
    val pressedAt = preferences.getLong(PENDING_PRESSED_AT, Long.MIN_VALUE)
    preferences.edit().clear().apply()
    val age = SystemClock.elapsedRealtime() - pressedAt
    return action.takeIf { it.isValid() && pressedAt != Long.MIN_VALUE && age in 0..PENDING_ACTION_LIFETIME_MS }
  }

  private fun send(callback: Channel, action: FocusNotificationAction) {
    callback.send(JSObject().apply {
      put("action", action.action)
      put("runId", action.runId)
      put("phaseId", action.phaseId)
    })
  }

  private fun store(context: Context) =
    context.getSharedPreferences(FOCUS_ACTION_STORE, Context.MODE_PRIVATE)
}

/**
 * Non-exported target of ongoing Focus notification actions. Only the app's own immutable
 * PendingIntents can start it. It runs in the main process, where the Rust Focus owner lives,
 * and opens the app only when that owner is not running yet.
 */
class FocusNotificationActionActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    if (savedInstanceState == null) {
      val action = FocusNotificationAction(
        action = intent?.getStringExtra(EXTRA_ACTION) ?: "",
        runId = intent?.getStringExtra(EXTRA_RUN_ID) ?: "",
        phaseId = intent?.getStringExtra(EXTRA_PHASE_ID) ?: "",
      )
      if (action.isValid() && !FocusNotificationActionBridge.deliver(applicationContext, action)) {
        packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
          launch.flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
          startActivity(launch)
        }
      }
    }
    finish()
  }
}
