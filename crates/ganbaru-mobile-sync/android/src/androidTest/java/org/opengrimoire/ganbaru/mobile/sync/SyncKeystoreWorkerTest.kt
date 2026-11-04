package org.opengrimoire.ganbaru.mobile.sync

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import java.util.concurrent.TimeUnit
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Checks that a WorkManager worker, which is how a background pass runs, can seal and open a key
 * with the Android Keystore without user authentication, and that a forgotten key reports
 * invalidation instead of returning bytes.
 */
@RunWith(AndroidJUnit4::class)
class SyncKeystoreWorkerTest {
  private val context: Context = ApplicationProvider.getApplicationContext()

  @After
  fun forgetTestKey() {
    SyncKeystore.forget(ALIAS)
  }

  @Test
  fun aWorkerSealsAndOpensAKeyAndAForgottenKeyIsInvalidated() {
    val request = OneTimeWorkRequestBuilder<RoundTripWorker>().build()
    val manager = WorkManager.getInstance(context)
    manager.enqueue(request).result.get(TIMEOUT_SECONDS, TimeUnit.SECONDS)
    val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(TIMEOUT_SECONDS)
    var info = manager.getWorkInfoById(request.id).get(TIMEOUT_SECONDS, TimeUnit.SECONDS)
    while (info?.state?.isFinished != true && System.nanoTime() < deadline) {
      Thread.sleep(POLL_MILLIS)
      info = manager.getWorkInfoById(request.id).get(TIMEOUT_SECONDS, TimeUnit.SECONDS)
    }
    assertEquals(WorkInfo.State.SUCCEEDED, info?.state)
    assertEquals(STATUS_OK.toInt(), info!!.outputData.getInt(KEY_WRAP_STATUS, -1))
    assertEquals(STATUS_OK.toInt(), info.outputData.getInt(KEY_UNWRAP_STATUS, -1))
    assertArrayEquals(SECRET, info.outputData.getByteArray(KEY_OPENED))
    assertEquals(STATUS_INVALIDATED.toInt(), info.outputData.getInt(KEY_FORGOTTEN_STATUS, -1))
  }

  /** Runs the round trip on the WorkManager executor, as the background pass does. */
  class RoundTripWorker(context: Context, parameters: WorkerParameters) :
    Worker(context, parameters) {
    override fun doWork(): Result {
      val aad = AAD.copyOf()
      val sealed = SyncKeystore.wrap(ALIAS, aad, SECRET.copyOf())
      val envelope = sealed.copyOfRange(1, sealed.size)
      val opened = SyncKeystore.unwrap(ALIAS, aad, envelope)
      SyncKeystore.forget(ALIAS)
      val forgotten = SyncKeystore.unwrap(ALIAS, aad, envelope)
      return Result.success(
        workDataOf(
          KEY_WRAP_STATUS to sealed[0].toInt(),
          KEY_UNWRAP_STATUS to opened[0].toInt(),
          KEY_OPENED to opened.copyOfRange(1, opened.size),
          KEY_FORGOTTEN_STATUS to forgotten[0].toInt(),
        ),
      )
    }
  }

  private companion object {
    const val ALIAS = "ganbaru-sync-keys:instrumented-test"
    const val TIMEOUT_SECONDS = 30L
    const val POLL_MILLIS = 100L
    const val STATUS_OK: Byte = 0
    const val STATUS_INVALIDATED: Byte = 2
    const val KEY_WRAP_STATUS = "wrapStatus"
    const val KEY_UNWRAP_STATUS = "unwrapStatus"
    const val KEY_OPENED = "opened"
    const val KEY_FORGOTTEN_STATUS = "forgottenStatus"
    val SECRET = byteArrayOf(1, 2, 3, 4, 5, 6, 7, 8)
    val AAD = "GKWR".toByteArray(Charsets.US_ASCII)
  }
}
