package org.opengrimoire.ganbaruai.mobile.notifications

import android.content.Context
import android.content.Intent

private const val ACTION_DISTRACTIONS_PHASE = "app.ganbaru.intent.action.DISTRACTIONS_PHASE"
private const val ACTION_DISTRACTIONS_PHASE_CLEAR =
  "app.ganbaru.intent.action.DISTRACTIONS_PHASE_CLEAR"
private const val INTERNAL_GUARDIAN_PERMISSION_SUFFIX = ".permission.INTERNAL_GUARDIAN"

internal object DistractionsPhaseBridge {
  fun publish(
    context: Context,
    projection: PomodoroNotificationProjection,
    activePhase: PomodoroNotificationPhase? = activePhase(projection),
  ) {
    val phase = activePhase ?: run {
      clear(context)
      return
    }
    context.sendBroadcast(Intent(ACTION_DISTRACTIONS_PHASE).apply {
      setPackage(context.packageName)
      putExtra("runId", projection.runId)
      putExtra("phase", phase.phase)
      putExtra("running", projection.isRunning)
      putExtra(
        "validUntilEpochMs",
        if (projection.isRunning) phase.endsAtEpochMs else projection.eventEndsAtEpochMs,
      )
    }, context.packageName + INTERNAL_GUARDIAN_PERMISSION_SUFFIX)
  }

  fun clear(context: Context) {
    context.sendBroadcast(Intent(ACTION_DISTRACTIONS_PHASE_CLEAR).apply {
      setPackage(context.packageName)
    }, context.packageName + INTERNAL_GUARDIAN_PERMISSION_SUFFIX)
  }

  private fun activePhase(
    projection: PomodoroNotificationProjection,
  ): PomodoroNotificationPhase? = acceptedPomodoroPhase(projection, System.currentTimeMillis())
}
