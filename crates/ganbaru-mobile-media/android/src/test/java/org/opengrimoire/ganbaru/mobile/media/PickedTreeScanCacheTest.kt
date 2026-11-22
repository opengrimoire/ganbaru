package org.opengrimoire.ganbaru.mobile.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PickedTreeScanCacheTest {
  private val tree = "content://com.android.externalstorage.documents/tree/primary%3AMusic"
  private val fullRefresh = TreeScanRequest(tree, 5_000, 48)
  private var now = 1_000L
  private val cache = PickedTreeScanCache<String>(reuseWindowMs = 30_000) { now }

  @Test
  fun oneFilePickIsNotReusedByAFullRefreshOfTheSameFolder() {
    cache.store(TreeScanRequest(tree, 1, 48), "one file")

    assertNull(cache.take(fullRefresh))
  }

  @Test
  fun matchingRefreshReusesThePickedScanOnce() {
    cache.store(fullRefresh, "full")

    assertEquals("full", cache.take(fullRefresh))
    assertNull(cache.take(fullRefresh))
  }

  @Test
  fun scanForAnotherRequestIsKeptForItsOwnRefresh() {
    cache.store(fullRefresh, "full")

    assertNull(cache.take(fullRefresh.copy(treeUri = "${tree}Other")))
    assertNull(cache.take(fullRefresh.copy(maxDepth = 12)))
    assertEquals("full", cache.take(fullRefresh))
  }

  @Test
  fun expiredScanIsDroppedInsteadOfAnsweringALaterRefresh() {
    cache.store(fullRefresh, "full")
    now += 30_001

    assertNull(cache.take(fullRefresh))
    now = 1_000
    assertNull(cache.take(fullRefresh))
  }

  @Test
  fun clockMovingBackwardsTreatsTheScanAsExpired() {
    cache.store(fullRefresh, "full")
    now -= 1

    assertNull(cache.take(fullRefresh))
  }

  @Test
  fun newPickClearsTheHeldScan() {
    cache.store(fullRefresh, "full")
    cache.clear()

    assertNull(cache.take(fullRefresh))
  }
}
