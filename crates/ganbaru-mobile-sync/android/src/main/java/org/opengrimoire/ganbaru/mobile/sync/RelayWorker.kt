package org.opengrimoire.ganbaru.mobile.sync

import android.content.Context
import android.net.ConnectivityManager
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Runs headless relay passes for WorkManager on any connected network. The pass tells Rust whether
 * the network is metered, so large asset files wait for an unmetered network while records still
 * move. A failed run waits for its next period.
 */
class RelayWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
  override suspend fun doWork(): Result {
    val settings = SyncPreferences(applicationContext).read()
    val configDir = settings.configDir
    if (configDir == null || !RelayScheduling.shouldRun(settings.relay, configDir)) {
      return Result.success()
    }
    val input = RelayScheduling.passInput(metered())
    withContext(Dispatchers.IO) { SyncNative.run(configDir, input) }
    return Result.success()
  }

  /** Whether the active network is metered; an unknown network counts as metered. */
  private fun metered(): Boolean {
    val connectivity = applicationContext.getSystemService(ConnectivityManager::class.java) ?: return true
    return connectivity.isActiveNetworkMetered
  }
}
