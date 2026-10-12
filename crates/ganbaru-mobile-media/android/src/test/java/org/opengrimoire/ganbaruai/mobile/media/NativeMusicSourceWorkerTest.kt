package org.opengrimoire.ganbaruai.mobile.media

import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.*
import org.junit.Test

class NativeMusicSourceWorkerTest {
  @Test
  fun blockedProviderRetainsOneWorkerWhileReplacementRequestsKeepOnlyTheLatestSource() {
    val worker = newMusicSourceWorker()
    val entered = CountDownLatch(1)
    val release = CountDownLatch(1)
    val executed = Collections.synchronizedList(mutableListOf<Int>())
    try {
      worker.execute { entered.countDown(); release.await(); executed.add(0) }
      assertTrue(entered.await(5, TimeUnit.SECONDS))
      for (source in 1..100) worker.execute { executed.add(source) }
      assertEquals(1, worker.poolSize)
      assertEquals(1, worker.activeCount)
      assertEquals(1, worker.queue.size)
      worker.shutdown()
      release.countDown()
      assertTrue(worker.awaitTermination(5, TimeUnit.SECONDS))
      assertEquals(listOf(0, 100), executed.toList())
      assertEquals(1, worker.largestPoolSize)
    } finally { release.countDown(); worker.shutdownNow() }
  }
}
