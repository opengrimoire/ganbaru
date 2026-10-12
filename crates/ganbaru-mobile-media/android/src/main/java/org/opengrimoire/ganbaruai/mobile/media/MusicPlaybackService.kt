package org.opengrimoire.ganbaruai.mobile.media

import android.app.PendingIntent
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.Player
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackParameters
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import app.tauri.plugin.JSObject
import org.json.JSONObject

class MusicPlaybackService : MediaSessionService() {
  private var mediaSession: MediaSession? = null
  private lateinit var decoder: ExoPlayer
  private val main = Handler(Looper.getMainLooper())
  private val sourceResolutionTimeoutMs = 30_000L
  private var resolutionDeadlineMs: Long? = null
  private var sessionId: String? = null
  private var sourceIdentity: String? = null
  private var generation = 0L
  private var sequence = 0L
  private var intendedVolume = 0.8f
  private var muted = false
  private var applying = false
  private var loading = false
  private var destroyed = false
  private var desiredAutoplay = false
  private var loadPositionMs = 0L
  private var loadEpoch = 0L
  private var sourceError: String? = null
  private val observer = object : Runnable {
    override fun run() {
      if (destroyed) return
      expireResolution()
      reportObservation()
      main.postDelayed(this, 250)
    }
  }

  override fun onCreate() {
    super.onCreate()
    val audioAttributes = AudioAttributes.Builder()
      .setUsage(C.USAGE_MEDIA)
      .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
      .build()
    val player = ExoPlayer.Builder(this)
      .setAudioAttributes(audioAttributes, true)
      .setHandleAudioBecomingNoisy(true)
      .setWakeMode(C.WAKE_MODE_LOCAL)
      .build()
    decoder = player
    player.repeatMode = Player.REPEAT_MODE_OFF
    player.addListener(object : Player.Listener {
      override fun onEvents(player: Player, events: Player.Events) { reportObservation() }
    })
    val sessionBuilder = MediaSession.Builder(this, SessionControls(player))
    packageManager.getLaunchIntentForPackage(packageName)?.let { launchIntent ->
      launchIntent.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
      sessionBuilder.setSessionActivity(
        PendingIntent.getActivity(
          this,
          0,
          launchIntent,
          PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        ),
      )
    }
    mediaSession = sessionBuilder.build()
    NativeMusicSession.created(this)
    main.post(observer)
  }

  /** Executes one generation-tagged native effect; no selection policy lives here. */
  fun applyNativeEffect(effect: JSONObject) {
    check(!destroyed) { "Android Music service was destroyed" }
    val kind = effect.getString("kind")
    val incomingGeneration = effect.getLong("generation")
    check(kind == "load" || kind == "stop" || incomingGeneration == generation) { "Android Music effect generation changed" }
    check((kind != "load" && kind != "stop") || incomingGeneration >= generation) { "Android Music effect is older than the decoder" }
    applying = true
    try {
      when (kind) {
        "load" -> {
          loadEpoch++
          generation = incomingGeneration
          sequence = 0
          sessionId = effect.getString("sessionId")
          val source = effect.getJSONObject("source")
          sourceIdentity = source.getString("identity")
          sourceError = null
          loading = true
          resolutionDeadlineMs = SystemClock.elapsedRealtime() + sourceResolutionTimeoutMs
          desiredAutoplay = effect.getBoolean("autoplay")
          loadPositionMs = effect.getLong("positionMs")
          decoder.stop()
          decoder.clearMediaItems()
          intendedVolume = effect.getDouble("volume").toFloat()
          muted = effect.getBoolean("muted")
          decoder.volume = if (muted) 0f else intendedVolume
          decoder.setPlaybackSpeed(effect.getDouble("rate").toFloat())
          val capturedGeneration = generation
          val capturedLoadEpoch = loadEpoch
          val deliveryId = effect.getLong("deliveryId")
          NativeMusicSession.resolveSource {
            try {
              NativeMusicAuthority.requireCurrent(deliveryId)
              val uri = NativeMusicSession.resolveUri(applicationContext, source.getString("path"))
              main.post {
                if (destroyed || capturedGeneration != generation || capturedLoadEpoch != loadEpoch) return@post
                if (expireResolution()) return@post
                resolutionDeadlineMs = null
                if (!NativeMusicAuthority.isDeliveryCurrent(deliveryId)) {
                  loading = false
                  desiredAutoplay = false
                  sessionId?.let { NativeMusicSession.reportUnavailable(it, generation, NativeMusicInterruption.SourceAuthorityChanged) }
                  return@post
                }
                applying = true
                try {
                  val item = MediaItem.Builder().setUri(uri).setMediaId(source.getString("identity"))
                    .setMediaMetadata(MediaMetadata.Builder().setTitle(source.getString("title")).build()).build()
                  decoder.playWhenReady = desiredAutoplay
                  decoder.setMediaItem(item, loadPositionMs)
                  decoder.prepare()
                  loading = false
                } catch (error: Exception) { sourceError = error.message ?: "Android media load failed"; loading = false }
                finally { applying = false; reportObservation() }
              }
            } catch (error: Exception) {
              main.post {
                if (!destroyed && capturedGeneration == generation && capturedLoadEpoch == loadEpoch) {
                  if (expireResolution()) return@post
                  resolutionDeadlineMs = null
                  loading = false
                  if (!NativeMusicAuthority.isDeliveryCurrent(deliveryId)) {
                    desiredAutoplay = false
                    sessionId?.let { NativeMusicSession.reportUnavailable(it, generation, NativeMusicInterruption.SourceAuthorityChanged) }
                  } else { sourceError = error.message ?: "Android media access failed"; reportObservation() }
                }
              }
            }
          }
        }
        "play" -> { desiredAutoplay = true; decoder.play() }
        "pause" -> { desiredAutoplay = false; decoder.pause() }
        "stop" -> { generation = maxOf(generation, incomingGeneration); loadEpoch++; resolutionDeadlineMs = null; desiredAutoplay = false; decoder.stop(); decoder.clearMediaItems(); sessionId = null; sourceIdentity = null; loading = false }
        "seek" -> { loadPositionMs = effect.getLong("positionMs"); if (!loading) decoder.seekTo(loadPositionMs) }
        "settings" -> {
          intendedVolume = effect.getDouble("volume").toFloat()
          muted = effect.getBoolean("muted")
          decoder.volume = if (muted) 0f else intendedVolume
          decoder.setPlaybackSpeed(effect.getDouble("rate").toFloat())
        }
        else -> throw IllegalArgumentException("Android Music effect kind is invalid")
      }
    } catch (error: Exception) { resolutionDeadlineMs = null; sourceError = error.message ?: "Android media effect failed"; loading = false; throw error }
    finally { applying = false; reportObservation() }
  }

  /** Elapsed realtime includes sleep; a late callback cannot outrun the observer. */
  private fun expireResolution(): Boolean {
    val deadline = resolutionDeadlineMs ?: return false
    if (SystemClock.elapsedRealtime() < deadline) return false
    resolutionDeadlineMs = null
    loadEpoch++
    loading = false
    desiredAutoplay = false
    sessionId?.let { NativeMusicSession.reportUnavailable(it, generation, NativeMusicInterruption.SourceResolutionTimeout) }
    return true
  }

  private fun reportObservation() {
    if (destroyed || applying || !::decoder.isInitialized) return
    val session = sessionId ?: return
    val identity = sourceIdentity ?: return
    val error = sourceError ?: decoder.playerError?.message
    val status = when {
      error != null -> "error"
      loading || decoder.playbackState == Player.STATE_BUFFERING -> "loading"
      decoder.playbackState == Player.STATE_ENDED -> "ended"
      decoder.isPlaying -> "playing"
      decoder.playbackState == Player.STATE_READY && !decoder.playWhenReady -> "paused"
      decoder.playbackState == Player.STATE_READY -> "ready"
      else -> "idle"
    }
    val duration = decoder.duration.takeIf { !loading && it != C.TIME_UNSET && it >= 0 }
    NativeMusicSession.reportObservation(JSObject().apply {
      put("sessionId", session); put("generation", generation); put("sequence", ++sequence)
      put("sourceIdentity", identity); put("status", status)
      put("positionMs", if (loading) loadPositionMs else decoder.currentPosition.coerceAtLeast(0))
      put("durationMs", duration ?: JSONObject.NULL); put("error", error ?: JSONObject.NULL)
    })
  }

  /** Trusted MediaSession controls are semantic intent, never a second queue owner. */
  private inner class SessionControls(player: Player) : ForwardingPlayer(player) {
    private fun intent(kind: String, field: String? = null, value: Any? = null) {
      if (destroyed) return
      NativeMusicSession.reportControl(JSObject().apply { put("kind", kind); if (field != null) put(field, value) })
    }
    override fun getAvailableCommands(): Player.Commands = NativeMusicCommands.available()
    override fun isCommandAvailable(command: Int): Boolean = NativeMusicCommands.allows(command)
    override fun play() = intent("play")
    override fun pause() = intent("pause")
    override fun setPlayWhenReady(playWhenReady: Boolean) = intent(if (playWhenReady) "play" else "pause")
    override fun stop() = intent("stop")
    override fun seekTo(positionMs: Long) = intent("seek", "positionMs", positionMs.coerceAtLeast(0))
    override fun seekTo(mediaItemIndex: Int, positionMs: Long) = seekTo(positionMs)
    override fun seekToNext() = intent("next")
    override fun seekToNextMediaItem() = intent("next")
    override fun seekToPrevious() = intent("previous")
    override fun seekToPreviousMediaItem() = intent("previous")
    override fun seekBack() = intent("seek-by", "deltaMs", -seekBackIncrement)
    override fun seekForward() = intent("seek-by", "deltaMs", seekForwardIncrement)
    override fun setVolume(volume: Float) = intent("volume", "volume", volume)
    override fun setPlaybackParameters(playbackParameters: PlaybackParameters) = intent("rate", "rate", playbackParameters.speed)
    override fun setPlaybackSpeed(speed: Float) = intent("rate", "rate", speed)
  }

  override fun onGetSession(
    controllerInfo: MediaSession.ControllerInfo,
  ): MediaSession? {
    val session = mediaSession ?: return null
    return if (controllerInfo.packageName == packageName || controllerInfo.isTrusted) session else null
  }

  override fun onDestroy() {
    main.removeCallbacks(observer)
    destroyed = true
    desiredAutoplay = false
    resolutionDeadlineMs = null
    mediaSession?.run {
      player.release()
      release()
    }
    mediaSession = null
    // Absence must mean the decoder has finished releasing, not only that
    // destruction started, because native vault quiescence observes isActive.
    NativeMusicSession.destroyed(this)
    sessionId?.let { NativeMusicSession.reportUnavailable(it, generation) }
    super.onDestroy()
  }
}
