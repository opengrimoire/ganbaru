package org.opengrimoire.ganbaru

import android.net.Uri
import android.view.View
import android.webkit.ConsoleMessage
import android.webkit.GeolocationPermissions
import android.webkit.JsPromptResult
import android.webkit.JsResult
import android.webkit.PermissionRequest
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebView
import java.net.URI
import java.net.URISyntaxException

private const val VIDEO_CAPTURE_RESOURCE = "android.webkit.resource.VIDEO_CAPTURE"
private val BUNDLED_APP_ORIGINS = setOf("http://tauri.localhost", "https://tauri.localhost")

/**
 * Decides which WebView permission requests reach the system prompt.
 *
 * The app needs only the camera, for device-link QR scanning, so only a camera-only request from the app origin is
 * allowed. Debug builds also accept the development server origin, which they already trust for bridge scripts.
 */
internal object WebViewPermissionPolicy {
  /** Returns the normalized `scheme://host[:port]` of an HTTP or HTTPS URL, or null for anything else. */
  fun origin(url: String?): String? {
    val uri = try {
      URI(url ?: return null)
    } catch (_: URISyntaxException) {
      return null
    }
    val scheme = uri.scheme?.lowercase() ?: return null
    val host = uri.host?.lowercase() ?: return null
    val defaultPort = when (scheme) {
      "http" -> 80
      "https" -> 443
      else -> return null
    }
    val port = if (uri.port == -1 || uri.port == defaultPort) "" else ":${uri.port}"
    return "$scheme://$host$port"
  }

  /** Whether a request for [resources] from [requestOrigin] may proceed to the camera permission flow. */
  fun allowsCameraCapture(
    requestOrigin: String?,
    resources: Array<String>?,
    developmentOrigin: String?,
  ): Boolean {
    if (resources.isNullOrEmpty() || resources.any { it != VIDEO_CAPTURE_RESOURCE }) return false
    val origin = origin(requestOrigin) ?: return false
    return origin in BUNDLED_APP_ORIGINS || (developmentOrigin != null && origin == origin(developmentOrigin))
  }
}

/**
 * Wraps the generated chrome client so permission grants follow [WebViewPermissionPolicy] and geolocation is denied.
 *
 * Every other callback the generated client implements is forwarded unchanged; the Android build checks that list
 * against the generated source.
 */
internal class AppOriginChromeClient(
  private val delegate: WebChromeClient,
  private val developmentOrigin: () -> String?,
) : WebChromeClient() {
  override fun onPermissionRequest(request: PermissionRequest?) {
    request ?: return
    val allowed = WebViewPermissionPolicy.allowsCameraCapture(
      request.origin?.toString(),
      request.resources,
      developmentOrigin(),
    )
    if (allowed) delegate.onPermissionRequest(request) else request.deny()
  }

  override fun onPermissionRequestCanceled(request: PermissionRequest?) {
    delegate.onPermissionRequestCanceled(request)
  }

  override fun onGeolocationPermissionsShowPrompt(
    origin: String?,
    callback: GeolocationPermissions.Callback?,
  ) {
    callback?.invoke(origin, false, false)
  }

  override fun onShowCustomView(view: View?, callback: CustomViewCallback?) {
    delegate.onShowCustomView(view, callback)
  }

  override fun onHideCustomView() {
    delegate.onHideCustomView()
  }

  override fun onJsAlert(view: WebView?, url: String?, message: String?, result: JsResult?): Boolean =
    delegate.onJsAlert(view, url, message, result)

  override fun onJsConfirm(view: WebView?, url: String?, message: String?, result: JsResult?): Boolean =
    delegate.onJsConfirm(view, url, message, result)

  override fun onJsPrompt(
    view: WebView?,
    url: String?,
    message: String?,
    defaultValue: String?,
    result: JsPromptResult?,
  ): Boolean = delegate.onJsPrompt(view, url, message, defaultValue, result)

  override fun onShowFileChooser(
    webView: WebView?,
    filePathCallback: ValueCallback<Array<Uri>>?,
    fileChooserParams: FileChooserParams?,
  ): Boolean = delegate.onShowFileChooser(webView, filePathCallback, fileChooserParams)

  override fun onConsoleMessage(consoleMessage: ConsoleMessage?): Boolean =
    delegate.onConsoleMessage(consoleMessage)

  override fun onReceivedTitle(view: WebView?, title: String?) {
    delegate.onReceivedTitle(view, title)
  }
}
