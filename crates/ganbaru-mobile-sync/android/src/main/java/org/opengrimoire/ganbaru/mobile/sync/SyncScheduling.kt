package org.opengrimoire.ganbaru.mobile.sync

/** What one native background pass achieved, as Rust reports it. */
internal enum class PassOutcome(val wire: String) {
  SYNCED("synced"),
  IDLE("idle"),
  UNREACHABLE("unreachable"),
  DELEGATED("delegated"),
  FAILED("failed");

  companion object {
    /** The outcome for a wire value, or [FAILED] for a missing or unknown one. */
    fun fromWire(value: String?): PassOutcome = entries.firstOrNull { it.wire == value } ?: FAILED
  }
}

/** The result of one native background pass. */
internal data class PassResult(val outcome: PassOutcome, val discoveryNames: List<String>) {
  companion object {
    val FAILED = PassResult(PassOutcome.FAILED, emptyList())
  }
}

/** What the worker does after a pass. */
internal enum class PassFollowUp {
  /** The run is done. */
  FINISH,
  /** The run could not reach the coordinator or failed; it may be retried. */
  RETRY,
  /** Browse the local network for the coordinator and run another pass with what it finds. */
  BROWSE,
}

/** How a worker run ends. */
internal enum class RunEnd { SUCCESS, RETRY }

/** How a one-time run is requested while changes wait. */
internal enum class OneTimeRequest {
  /** An expedited job that falls back to ordinary work when the expedited quota is spent. */
  EXPEDITED,
  /**
   * Ordinary work. Before Android 12 WorkManager runs expedited work as a foreground service, which
   * background sync never uses.
   */
  ORDINARY,
}

/** Scheduling decisions for background sync, kept free of Android types so JVM tests cover them. */
internal object SyncScheduling {
  const val PERIODIC_WORK = "ganbaru-sync-periodic"
  const val ONE_TIME_WORK = "ganbaru-sync-now"
  const val PERIODIC_INTERVAL_MINUTES = 15L
  const val RETRY_BACKOFF_SECONDS = 30L
  /** A one-time run is attempted at most this many times. */
  const val MAX_ONE_TIME_ATTEMPTS = 2
  const val DISCOVERY_BROWSE_MILLIS = 3_000L
  const val SERVICE_TYPE = "_ganbaru._tcp"
  /** Android 12, the first release that runs expedited work as a job instead of a foreground service. */
  const val EXPEDITED_JOB_API_LEVEL = 31

  /** A pass runs only for a linked device on a network that can reach a LAN coordinator. */
  fun shouldRun(linked: Boolean, localNetwork: Boolean): Boolean = linked && localNetwork

  /** The next step after a pass; `browsed` is true once the run already browsed. */
  fun followUp(result: PassResult, browsed: Boolean): PassFollowUp = when (result.outcome) {
    PassOutcome.UNREACHABLE ->
      if (!browsed && result.discoveryNames.isNotEmpty()) PassFollowUp.BROWSE else PassFollowUp.RETRY
    PassOutcome.FAILED -> PassFollowUp.RETRY
    PassOutcome.SYNCED, PassOutcome.IDLE, PassOutcome.DELEGATED -> PassFollowUp.FINISH
  }

  /**
   * How a run ends. Only a one-time run retries, and only before its last attempt; a periodic run
   * waits for its next period. `runAttemptCount` counts earlier attempts of this work.
   */
  fun runEnd(followUp: PassFollowUp, oneTime: Boolean, runAttemptCount: Int): RunEnd =
    if (followUp == PassFollowUp.RETRY && oneTime && runAttemptCount + 1 < MAX_ONE_TIME_ATTEMPTS) {
      RunEnd.RETRY
    } else {
      RunEnd.SUCCESS
    }

  /** The kind of one-time request on Android `apiLevel`. */
  fun oneTimeRequest(apiLevel: Int): OneTimeRequest =
    if (apiLevel >= EXPEDITED_JOB_API_LEVEL) OneTimeRequest.EXPEDITED else OneTimeRequest.ORDINARY
}

/** The network a relay run waits for. */
internal enum class RelayNetwork {
  /** Any network with internet access, metered or not. */
  CONNECTED,
}

/**
 * Relay scheduling decisions, kept free of Android types so JVM tests cover them. A relay is a
 * user-configured server on the internet, so relay runs need any connected network rather than
 * the local network a coordinator exchange needs.
 */
internal object RelayScheduling {
  const val PERIODIC_WORK = "ganbaru-sync-relay"
  const val PERIODIC_INTERVAL_MINUTES = 15L

  /** The network every relay run requires. */
  val requiredNetwork: RelayNetwork = RelayNetwork.CONNECTED

  /** A relay run happens only for a vault whose sync space uses a relay. */
  fun shouldRun(relay: Boolean, configDir: String?): Boolean = relay && !configDir.isNullOrEmpty()

  /**
   * The pass input that tells Rust whether the network is metered. On a metered network the pass
   * still moves every record but leaves asset files above the metered bound for a later run.
   */
  fun passInput(metered: Boolean): String = """{"relay":{"metered":$metered}}"""
}

/**
 * Tells network changes apart from the state a callback reports when it is registered, so
 * registering never schedules a pass by itself.
 */
internal class NetworkObservations {
  private var seen = false
  private var last: Pair<Long, Set<String>>? = null

  /** Records the default network and its addresses and reports whether they changed. */
  @Synchronized
  fun observe(network: Long, addresses: Set<String>): Boolean {
    val next = network to addresses
    val changed = seen && last != next
    seen = true
    last = next
    return changed
  }

  /** Records that the default network went away, so the next one counts as a change. */
  @Synchronized
  fun lost() {
    if (seen) last = null
  }
}
