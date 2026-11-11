package org.opengrimoire.ganbaru

import android.animation.ValueAnimator
import android.app.UiModeManager
import android.content.Context
import android.database.ContentObserver
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.animation.PathInterpolator
import android.webkit.JavascriptInterface
import android.webkit.WebChromeClient
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsAnimationCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.WebSettingsCompat
import androidx.webkit.WebViewFeature
import kotlin.math.roundToInt

private const val ANDROID_INSETS_BRIDGE_NAME = "GanbaruAndroidInsets"
private const val ANDROID_APPEARANCE_BRIDGE_NAME = "GanbaruAndroidAppearance"
private const val ANDROID_MEDIA_VOLUME_BRIDGE_NAME = "GanbaruAndroidMediaVolume"
private const val ANDROID_INSETS_EVENT_NAME = "ganbaru:android-insets"
private const val ANDROID_MEDIA_VOLUME_EVENT_NAME = "ganbaru:android-media-volume"
private const val APPEARANCE_PREFERENCES_NAME = "ganbaru_appearance"
private const val WINDOW_BACKGROUND_KEY = "window_background"
private const val NIGHT_MODE_KEY = "night_mode"

/** Content canvas (`--cal-bg`) of the default dark theme, painted until the page reports the active theme. */
private const val DEFAULT_WINDOW_BACKGROUND = 0xFF1F2023.toInt()

private class AndroidInsetsBridge(displayDensity: Float) {
  private val density = displayDensity.takeIf { it.isFinite() && it > 0f } ?: 1f
  private var top = 0
  private var right = 0
  private var bottom = 0
  private var left = 0
  private var keyboard = 0

  @Synchronized
  fun update(top: Int, right: Int, bottom: Int, left: Int, keyboard: Int) {
    this.top = top
    this.right = right
    this.bottom = bottom
    this.left = left
    this.keyboard = keyboard
  }

  @JavascriptInterface
  @Synchronized
  fun systemBars(): String = listOf(top, right, bottom, left)
    .joinToString(",") { value -> toCssPixels(value).toString() }

  /**
   * Height the soft keyboard covers from the bottom edge of the WebView, navigation bar included, in CSS pixels, or
   * zero while it is hidden. It changes on every frame while the keyboard slides on Android 11 and later.
   */
  @JavascriptInterface
  @Synchronized
  fun keyboard(): String = toCssPixels(keyboard).toString()

  private fun toCssPixels(value: Int): Int = (value / density).roundToInt()
}

/** Duration of the slide played when the system reports a keyboard change without animating it, in milliseconds. */
private const val KEYBOARD_FALLBACK_DURATION_MS = 250L

/**
 * Follows the soft keyboard without resizing the WebView. The activity draws edge to edge, so the keyboard slides over
 * the page, and the page lifts the surface holding the focused field by the reported height. The height follows every
 * frame of the system keyboard animation on Android 11 and later; when the system reports a change without animating
 * it, the tracker plays a short slide of its own, so the page always moves with the keyboard instead of jumping.
 */
private class KeyboardTracker(private val webView: WebView, private val onChanged: (height: Int) -> Unit) {
  private var height = 0
  private var settledHeight = 0
  private var keyboardAnimating = false
  private var fallback: ValueAnimator? = null

  /** Current keyboard height over the bottom edge of the WebView, in device pixels. */
  val bottom: Int
    get() = height

  fun attach() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
    ViewCompat.setWindowInsetsAnimationCallback(
      webView,
      object : WindowInsetsAnimationCompat.Callback(
        WindowInsetsAnimationCompat.Callback.DISPATCH_MODE_CONTINUE_ON_SUBTREE,
      ) {
        override fun onPrepare(animation: WindowInsetsAnimationCompat) {
          if (animation.typeMask and WindowInsetsCompat.Type.ime() == 0) return
          keyboardAnimating = true
          fallback?.cancel()
        }

        override fun onProgress(
          insets: WindowInsetsCompat,
          runningAnimations: MutableList<WindowInsetsAnimationCompat>,
        ): WindowInsetsCompat {
          val keyboardMoving = runningAnimations.any { it.typeMask and WindowInsetsCompat.Type.ime() != 0 }
          if (keyboardMoving) set(insets.getInsets(WindowInsetsCompat.Type.ime()).bottom)
          return withoutKeyboard(insets)
        }

        override fun onEnd(animation: WindowInsetsAnimationCompat) {
          if (animation.typeMask and WindowInsetsCompat.Type.ime() == 0) return
          keyboardAnimating = false
          set(settledHeight)
        }
      },
    )
  }

  /**
   * Records [keyboardBottom], the height the keyboard settles at. While the system animates the keyboard, its frames
   * report the height instead; otherwise the height slides to [keyboardBottom].
   */
  fun update(keyboardBottom: Int) {
    settledHeight = keyboardBottom
    if (keyboardAnimating || keyboardBottom == height) return
    fallback?.cancel()
    fallback = ValueAnimator.ofInt(height, keyboardBottom).apply {
      duration = KEYBOARD_FALLBACK_DURATION_MS
      interpolator = PathInterpolator(0.2f, 0f, 0f, 1f)
      addUpdateListener { animator -> set(animator.animatedValue as Int) }
      start()
    }
  }

  private fun set(value: Int) {
    if (value == height) return
    height = value
    onChanged(value)
  }
}

/**
 * Returns [insets] with the keyboard zeroed. The page follows the keyboard through the insets bridge, so the WebView
 * must not also shrink its visual viewport, which it does at once to the final keyboard height.
 */
private fun withoutKeyboard(insets: WindowInsetsCompat): WindowInsetsCompat =
  WindowInsetsCompat.Builder(insets).setInsets(WindowInsetsCompat.Type.ime(), Insets.NONE).build()

private class AndroidAppearanceBridge(private val activity: MainActivity) {
  @JavascriptInterface
  fun setLightTheme(lightTheme: Boolean) {
    activity.runOnUiThread {
      WindowCompat.getInsetsController(activity.window, activity.window.decorView).apply {
        isAppearanceLightStatusBars = lightTheme
        isAppearanceLightNavigationBars = lightTheme
      }
      activity.applyNightMode(!lightTheme)
    }
  }

  /** Paints [color], a `#rrggbb` theme surface, behind the WebView now and on the next cold start. */
  @JavascriptInterface
  fun setWindowBackground(color: String) {
    val parsed = try {
      Color.parseColor(color)
    } catch (_: IllegalArgumentException) {
      return
    }
    activity.runOnUiThread { activity.applyWindowBackground(parsed) }
  }
}

/**
 * Exposes the device media volume, which is the Music volume on Android, and reports changes from hardware
 * buttons, the system panel, and other apps while the activity is in the foreground.
 */
private class AndroidMediaVolumeBridge(private val audioManager: AudioManager) {
  private var published: String? = null

  /** Current media volume as `index,max`, or empty when the device does not let apps change it. */
  @JavascriptInterface
  fun state(): String {
    if (audioManager.isVolumeFixed) return ""
    val max = audioManager.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
    if (max <= 0) return ""
    return "${audioManager.getStreamVolume(AudioManager.STREAM_MUSIC)},$max"
  }

  /** Sets the media volume to the nearest device step for [level], from 0 to 1, and returns the new [state]. */
  @JavascriptInterface
  fun setLevel(level: Double): String {
    if (!level.isFinite() || audioManager.isVolumeFixed) return state()
    val max = audioManager.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
    if (max <= 0) return ""
    val index = (level.coerceIn(0.0, 1.0) * max).roundToInt()
    try {
      audioManager.setStreamVolume(AudioManager.STREAM_MUSIC, index, 0)
    } catch (_: SecurityException) {
      // Do Not Disturb can forbid volume changes; the returned state shows the volume that stayed.
    }
    return state().also { markPublished(it) }
  }

  /** Records [current] as the state the page knows, and reports whether it differed. */
  @Synchronized
  fun markPublished(current: String): Boolean {
    if (current == published) return false
    published = current
    return true
  }

  @Synchronized
  fun forgetPublished() {
    published = null
  }
}

class MainActivity : TauriActivity() {
  private val androidInsetsBridge by lazy {
    AndroidInsetsBridge(resources.displayMetrics.density)
  }
  private val androidAppearanceBridge by lazy { AndroidAppearanceBridge(this) }
  private val appearancePreferences by lazy {
    getSharedPreferences(APPEARANCE_PREFERENCES_NAME, Context.MODE_PRIVATE)
  }
  private val androidMediaVolumeBridge by lazy {
    AndroidMediaVolumeBridge(getSystemService(AudioManager::class.java))
  }
  private val mediaVolumeObserver by lazy {
    object : ContentObserver(Handler(Looper.getMainLooper())) {
      override fun onChange(selfChange: Boolean) = publishMediaVolume()
    }
  }
  private var appWebView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    window.setBackgroundDrawable(ColorDrawable(storedWindowBackground()))
  }

  override fun onResume() {
    super.onResume()
    contentResolver.registerContentObserver(Settings.System.CONTENT_URI, true, mediaVolumeObserver)
    androidMediaVolumeBridge.forgetPublished()
    publishMediaVolume()
  }

  override fun onPause() {
    contentResolver.unregisterContentObserver(mediaVolumeObserver)
    super.onPause()
  }

  /** The system stores stream volumes as settings, so a settings change may be a media volume change. */
  private fun publishMediaVolume() {
    val webView = appWebView ?: return
    if (!androidMediaVolumeBridge.markPublished(androidMediaVolumeBridge.state())) return
    webView.evaluateJavascript("window.dispatchEvent(new Event('$ANDROID_MEDIA_VOLUME_EVENT_NAME'))", null)
  }

  /**
   * The page is transparent until the app shell mounts, so the window and WebView show this color instead of
   * their white defaults. The last theme surface is kept so the next launch opens on it.
   */
  private fun storedWindowBackground(): Int =
    appearancePreferences.getInt(WINDOW_BACKGROUND_KEY, DEFAULT_WINDOW_BACKGROUND)

  internal fun applyWindowBackground(color: Int) {
    window.setBackgroundDrawable(ColorDrawable(color))
    appWebView?.setBackgroundColor(color)
    if (storedWindowBackground() != color) {
      appearancePreferences.edit().putInt(WINDOW_BACKGROUND_KEY, color).apply()
    }
  }

  /**
   * Android 12 and later draw the launch splash before the page runs, using the app's persisted night mode, so the
   * app theme's mode is persisted for the next cold start. Earlier versions follow the system mode.
   */
  internal fun applyNightMode(dark: Boolean) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
    val mode = if (dark) UiModeManager.MODE_NIGHT_YES else UiModeManager.MODE_NIGHT_NO
    if (appearancePreferences.getInt(NIGHT_MODE_KEY, UiModeManager.MODE_NIGHT_AUTO) == mode) return
    getSystemService(UiModeManager::class.java)?.setApplicationNightMode(mode) ?: return
    appearancePreferences.edit().putInt(NIGHT_MODE_KEY, mode).apply()
  }

  private fun installPermissionPolicy(webView: WebView) {
    val current = webView.webChromeClient
    if (current is AppOriginChromeClient) return
    webView.webChromeClient = AppOriginChromeClient(current ?: WebChromeClient()) {
      if (BuildConfig.DEBUG) webView.url else null
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    appWebView = webView
    // The generated chrome client is attached after this callback returns, so the wrapper installs on the next turn
    // of the main looper, before any page script can request a permission.
    Handler(Looper.getMainLooper()).post { installPermissionPolicy(webView) }
    webView.setBackgroundColor(storedWindowBackground())
    if (WebViewFeature.isFeatureSupported(WebViewFeature.ALGORITHMIC_DARKENING)) {
      WebSettingsCompat.setAlgorithmicDarkeningAllowed(webView.settings, false)
    }
    webView.addJavascriptInterface(androidInsetsBridge, ANDROID_INSETS_BRIDGE_NAME)
    webView.addJavascriptInterface(androidAppearanceBridge, ANDROID_APPEARANCE_BRIDGE_NAME)
    webView.addJavascriptInterface(androidMediaVolumeBridge, ANDROID_MEDIA_VOLUME_BRIDGE_NAME)
    var systemBars = Insets.NONE
    fun publishInsets(keyboard: Int) {
      androidInsetsBridge.update(
        top = systemBars.top,
        right = systemBars.right,
        bottom = systemBars.bottom,
        left = systemBars.left,
        keyboard = keyboard,
      )
      webView.evaluateJavascript(
        "window.dispatchEvent(new Event('$ANDROID_INSETS_EVENT_NAME'))",
        null,
      )
    }
    val keyboardTracker = KeyboardTracker(webView, ::publishInsets)
    keyboardTracker.attach()
    ViewCompat.setOnApplyWindowInsetsListener(webView) { _, windowInsets ->
      systemBars = windowInsets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      val keyboardVisible = windowInsets.isVisible(WindowInsetsCompat.Type.ime())
      val keyboardBottom = if (keyboardVisible) {
        windowInsets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      } else {
        0
      }
      keyboardTracker.update(keyboardBottom)
      publishInsets(keyboardTracker.bottom)
      withoutKeyboard(windowInsets)
    }
    ViewCompat.requestApplyInsets(webView)
  }
}
