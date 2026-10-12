package org.opengrimoire.ganbaruai.mobile.media

import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** One process-owned resolver and one latest queued source, even across service replacement. */
internal fun newMusicSourceWorker(): ThreadPoolExecutor = ThreadPoolExecutor(
  1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1),
  ThreadPoolExecutor.DiscardOldestPolicy(),
)
