package org.opengrimoire.ganbaruai.mobile.notifications

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

internal data class DeviceLocalTimeFact(
  val epochMs: Long,
  val dateKey: String,
  val dateString: String,
  val hour: Int,
)

/** Historical instants use each date's actual offset, including timezone transitions. */
internal object DeviceLocalTimeFacts {
  const val MAX_INSTANTS = 4096
  private val dateStringFormatter = DateTimeFormatter.ofPattern("EEE MMM dd uuuu", Locale.US)

  fun resolve(instants: List<Long>, zone: ZoneId = ZoneId.systemDefault()): List<DeviceLocalTimeFact> {
    require(instants.size <= MAX_INSTANTS) { "Device local time request exceeds its instant limit" }
    return instants.map { epochMs ->
      val local = Instant.ofEpochMilli(epochMs).atZone(zone)
      DeviceLocalTimeFact(epochMs, local.toLocalDate().toString(), local.format(dateStringFormatter), local.hour)
    }
  }
}
