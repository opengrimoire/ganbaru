package org.opengrimoire.ganbaruai

import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Build
import android.os.Bundle
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.widget.ImageView
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
private const val ANDROID_TRANSITION_BRIDGE_NAME = "GanbaruAndroidTransition"
private const val ANDROID_INSETS_EVENT_NAME = "ganbaru:android-insets"
private const val ANDROID_TRANSITION_FRAME_HELD_EVENT_NAME =
  "ganbaru:android-transition-frame-held"

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

  /** Height of the soft keyboard below the WebView, in CSS pixels, or zero while it is hidden. */
  @JavascriptInterface
  @Synchronized
  fun keyboard(): String = toCssPixels(keyboard).toString()

  private fun toCssPixels(value: Int): Int = (value / density).roundToInt()
}

/**
 * Keeps the WebView above the soft keyboard. The activity draws edge to edge, so the system does not resize the
 * WebView for the keyboard; the bottom margin does instead, which lets the page shrink and the browser keep the
 * focused field visible. While the keyboard slides in on Android 11 and later, the WebView keeps its size until the
 * animation ends so no empty band shows above the moving keyboard; it grows back as soon as the keyboard starts
 * hiding, under the keyboard.
 */
private class KeyboardResizer(private val webView: WebView) {
  private var appliedBottom = 0
  private var pendingBottom: Int? = null
  private var keyboardAnimating = false

  val bottom: Int
    get() = appliedBottom

  fun attach(onApplied: () -> Unit) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
    ViewCompat.setWindowInsetsAnimationCallback(
      webView,
      object : WindowInsetsAnimationCompat.Callback(
        WindowInsetsAnimationCompat.Callback.DISPATCH_MODE_CONTINUE_ON_SUBTREE,
      ) {
        override fun onPrepare(animation: WindowInsetsAnimationCompat) {
          if (animation.typeMask and WindowInsetsCompat.Type.ime() != 0) keyboardAnimating = true
        }

        override fun onProgress(
          insets: WindowInsetsCompat,
          runningAnimations: MutableList<WindowInsetsAnimationCompat>,
        ): WindowInsetsCompat = insets

        override fun onEnd(animation: WindowInsetsAnimationCompat) {
          if (animation.typeMask and WindowInsetsCompat.Type.ime() == 0) return
          keyboardAnimating = false
          val pending = pendingBottom ?: return
          pendingBottom = null
          apply(pending)
          onApplied()
        }
      },
    )
  }

  /** Resize for the keyboard height [keyboardBottom], and report whether the WebView changed size now. */
  fun update(keyboardBottom: Int): Boolean {
    if (keyboardAnimating && keyboardBottom > appliedBottom) {
      pendingBottom = keyboardBottom
      return false
    }
    pendingBottom = null
    return apply(keyboardBottom)
  }

  private fun apply(keyboardBottom: Int): Boolean {
    if (keyboardBottom == appliedBottom) return false
    val params = webView.layoutParams as? ViewGroup.MarginLayoutParams ?: return false
    appliedBottom = keyboardBottom
    params.bottomMargin = keyboardBottom
    webView.layoutParams = params
    return true
  }
}

private class AndroidAppearanceBridge(private val activity: MainActivity) {
  @JavascriptInterface
  fun setLightTheme(lightTheme: Boolean) {
    activity.runOnUiThread {
      WindowCompat.getInsetsController(activity.window, activity.window.decorView).apply {
        isAppearanceLightStatusBars = lightTheme
        isAppearanceLightNavigationBars = lightTheme
      }
    }
  }
}

private class AndroidTransitionBridge(
  private val activity: MainActivity,
  private val webView: WebView,
) {
  private var heldFrame: ImageView? = null
  private var heldBitmap: Bitmap? = null

  @JavascriptInterface
  fun holdCurrentFrame() {
    activity.runOnUiThread {
      if (heldFrame == null) {
        val parent = webView.parent as? ViewGroup
        val width = webView.width
        val height = webView.height
        if (parent != null && width > 0 && height > 0) {
          val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
          webView.draw(Canvas(bitmap))
          val frame = ImageView(activity).apply {
            setImageBitmap(bitmap)
            scaleType = ImageView.ScaleType.FIT_XY
            isClickable = true
            importantForAccessibility = ImageView.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
          }
          parent.addView(
            frame,
            ViewGroup.LayoutParams(
              ViewGroup.LayoutParams.MATCH_PARENT,
              ViewGroup.LayoutParams.MATCH_PARENT,
            ),
          )
          heldBitmap = bitmap
          heldFrame = frame
        }
      }
      webView.evaluateJavascript(
        "window.dispatchEvent(new Event('$ANDROID_TRANSITION_FRAME_HELD_EVENT_NAME'))",
        null,
      )
    }
  }

  @JavascriptInterface
  fun releaseHeldFrame() {
    activity.runOnUiThread {
      heldFrame?.let { frame ->
        (frame.parent as? ViewGroup)?.removeView(frame)
        frame.setImageDrawable(null)
      }
      heldFrame = null
      heldBitmap?.recycle()
      heldBitmap = null
    }
  }
}

class MainActivity : TauriActivity() {
  private val androidInsetsBridge by lazy {
    AndroidInsetsBridge(resources.displayMetrics.density)
  }
  private val androidAppearanceBridge by lazy { AndroidAppearanceBridge(this) }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    if (WebViewFeature.isFeatureSupported(WebViewFeature.ALGORITHMIC_DARKENING)) {
      WebSettingsCompat.setAlgorithmicDarkeningAllowed(webView.settings, false)
    }
    webView.addJavascriptInterface(androidInsetsBridge, ANDROID_INSETS_BRIDGE_NAME)
    webView.addJavascriptInterface(androidAppearanceBridge, ANDROID_APPEARANCE_BRIDGE_NAME)
    webView.addJavascriptInterface(
      AndroidTransitionBridge(this, webView),
      ANDROID_TRANSITION_BRIDGE_NAME,
    )
    val keyboardResizer = KeyboardResizer(webView)
    var systemBars = Insets.NONE
    fun publishInsets() {
      // The keyboard covers the navigation bar, so the WebView keeps only the part of it the keyboard leaves visible.
      val keyboard = keyboardResizer.bottom
      androidInsetsBridge.update(
        top = systemBars.top,
        right = systemBars.right,
        bottom = (systemBars.bottom - keyboard).coerceAtLeast(0),
        left = systemBars.left,
        keyboard = keyboard,
      )
      webView.evaluateJavascript(
        "window.dispatchEvent(new Event('$ANDROID_INSETS_EVENT_NAME'))",
        null,
      )
    }
    keyboardResizer.attach(::publishInsets)
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
      keyboardResizer.update(keyboardBottom)
      publishInsets()
      windowInsets
    }
    ViewCompat.requestApplyInsets(webView)
  }
}
