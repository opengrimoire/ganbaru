package org.opengrimoire.ganbaru.mobile.media

import android.annotation.SuppressLint
import android.app.Activity
import android.graphics.Color
import android.net.Uri
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.webkit.PermissionRequest
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebMessageCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject
import java.io.ByteArrayInputStream
import kotlin.math.roundToInt

/** Name of the message object the player page sees; only the host origin receives it. */
private const val YOUTUBE_HOST_MESSAGE_OBJECT = "ganbaruYouTubeHost"
private const val MAX_YOUTUBE_HOST_MESSAGE_CHARS = 262_144

/** Bounds of a native YouTube host in CSS pixels of the app WebView. */
internal data class YouTubeHostBounds(
  val x: Double,
  val y: Double,
  val width: Double,
  val height: Double,
  val scale: Double,
)

/** A loaded page description. The URL, origin, page, and policy are checked in Rust before they reach Kotlin. */
internal data class YouTubeHostPage(
  val url: String,
  val origin: String,
  val path: String,
  val html: String,
  val contentSecurityPolicy: String,
)

/**
 * Touch-transparent WebView: taps fall through to the app WebView, whose overlay owns play, pause, and fullscreen
 * gestures exactly as it does over the desktop iframe.
 */
@SuppressLint("ViewConstructor")
private class PassThroughWebView(activity: Activity) : WebView(activity) {
  override fun dispatchTouchEvent(event: MotionEvent?): Boolean = false
  override fun onTouchEvent(event: MotionEvent?): Boolean = false
}

private class YouTubeHostInstance(
  val webView: WebView,
  val page: YouTubeHostPage,
  var replyProxy: JavaScriptReplyProxy? = null,
)

/**
 * Hosts the YouTube player page in WebViews separate from the app WebView. They carry no JavaScript interfaces and
 * no document-start scripts; the only bridge is a web message listener limited to the host origin's main frame.
 */
internal class YouTubeHostViews(private val activity: Activity) {
  private val instances = mutableMapOf<String, YouTubeHostInstance>()

  fun open(id: String, page: YouTubeHostPage, bounds: YouTubeHostBounds, visible: Boolean, channel: Channel) {
    check(WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
      "This Android System WebView cannot host YouTube playback"
    }
    close(id)
    val webView = createWebView(page)
    val instance = YouTubeHostInstance(webView, page)
    WebViewCompat.addWebMessageListener(
      webView,
      YOUTUBE_HOST_MESSAGE_OBJECT,
      setOf(page.origin),
    ) { _, message: WebMessageCompat, sourceOrigin: Uri, isMainFrame: Boolean, replyProxy: JavaScriptReplyProxy ->
      if (instances[id] !== instance || !isMainFrame || sourceOrigin.toString() != page.origin) {
        return@addWebMessageListener
      }
      val data = message.data ?: return@addWebMessageListener
      if (data.length > MAX_YOUTUBE_HOST_MESSAGE_CHARS) return@addWebMessageListener
      instance.replyProxy = replyProxy
      channel.send(JSObject().apply {
        put("instance", id)
        put("data", data)
      })
    }
    instances[id] = instance
    contentRoot().addView(webView, FrameLayout.LayoutParams(1, 1))
    applyBounds(webView, bounds, visible)
    webView.loadUrl(page.url)
  }

  fun setBounds(id: String, bounds: YouTubeHostBounds, visible: Boolean) {
    val instance = instances[id] ?: return
    applyBounds(instance.webView, bounds, visible)
  }

  fun post(id: String, message: String) {
    instances[id]?.replyProxy?.postMessage(message)
  }

  fun close(id: String) {
    val instance = instances.remove(id) ?: return
    (instance.webView.parent as? ViewGroup)?.removeView(instance.webView)
    instance.webView.stopLoading()
    instance.webView.destroy()
  }

  fun closeAll() {
    instances.keys.toList().forEach(::close)
  }

  @SuppressLint("SetJavaScriptEnabled")
  private fun createWebView(page: YouTubeHostPage): WebView = PassThroughWebView(activity).apply {
    isFocusable = false
    isFocusableInTouchMode = false
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
    setBackgroundColor(Color.BLACK)
    settings.javaScriptEnabled = true
    settings.domStorageEnabled = true
    settings.mediaPlaybackRequiresUserGesture = false
    settings.javaScriptCanOpenWindowsAutomatically = false
    settings.setSupportMultipleWindows(false)
    settings.setGeolocationEnabled(false)
    settings.allowFileAccess = false
    settings.allowContentAccess = false
    setDownloadListener { _, _, _, _, _ -> }
    webChromeClient = object : WebChromeClient() {
      override fun onPermissionRequest(request: PermissionRequest) = request.deny()
      override fun onShowFileChooser(
        webView: WebView?,
        filePathCallback: ValueCallback<Array<Uri>>?,
        fileChooserParams: FileChooserParams?,
      ): Boolean {
        filePathCallback?.onReceiveValue(null)
        return true
      }
    }
    webViewClient = object : WebViewClient() {
      override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
        request.isForMainFrame && !isHostPage(request.url, page)

      override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
        if (originOf(request.url) != page.origin) return null
        if (!request.isForMainFrame || request.url.path != page.path) {
          return WebResourceResponse("text/plain", "utf-8", 404, "Not Found", emptyMap(), ByteArrayInputStream(ByteArray(0)))
        }
        return WebResourceResponse(
          "text/html",
          "utf-8",
          200,
          "OK",
          mapOf(
            "Content-Security-Policy" to page.contentSecurityPolicy,
            "Cache-Control" to "no-store",
            "Referrer-Policy" to "strict-origin-when-cross-origin",
            "X-Content-Type-Options" to "nosniff",
          ),
          ByteArrayInputStream(page.html.toByteArray(Charsets.UTF_8)),
        )
      }
    }
  }

  private fun isHostPage(url: Uri, page: YouTubeHostPage): Boolean =
    originOf(url) == page.origin && url.path == page.path

  private fun originOf(url: Uri): String {
    val port = if (url.port == -1) "" else ":${url.port}"
    return "${url.scheme}://${url.host}$port"
  }

  private fun contentRoot(): ViewGroup = activity.findViewById(android.R.id.content)

  /** Finds the app WebView so CSS-pixel bounds can be placed relative to its viewport. */
  private fun appWebView(root: ViewGroup): WebView? {
    for (index in 0 until root.childCount) {
      val child = root.getChildAt(index)
      if (child is PassThroughWebView) continue
      if (child is WebView) return child
      if (child is ViewGroup) appWebView(child)?.let { return it }
    }
    return null
  }

  private fun applyBounds(webView: WebView, bounds: YouTubeHostBounds, visible: Boolean) {
    val root = contentRoot()
    val origin = IntArray(2)
    val rootOrigin = IntArray(2)
    appWebView(root)?.getLocationInWindow(origin)
    root.getLocationInWindow(rootOrigin)
    val left = origin[0] - rootOrigin[0] + (bounds.x * bounds.scale).roundToInt()
    val top = origin[1] - rootOrigin[1] + (bounds.y * bounds.scale).roundToInt()
    val width = (bounds.width * bounds.scale).roundToInt().coerceAtLeast(1)
    val height = (bounds.height * bounds.scale).roundToInt().coerceAtLeast(1)
    val params = (webView.layoutParams as? FrameLayout.LayoutParams) ?: FrameLayout.LayoutParams(width, height)
    params.width = width
    params.height = height
    params.leftMargin = left
    params.topMargin = top
    webView.layoutParams = params
    webView.visibility = if (visible) View.VISIBLE else View.INVISIBLE
    webView.bringToFront()
  }
}
