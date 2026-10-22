package org.opengrimoire.ganbaru

import org.junit.Assert.*
import org.junit.Test

class WebViewPermissionPolicyTest {
  private val video = "android.webkit.resource.VIDEO_CAPTURE"
  private val audio = "android.webkit.resource.AUDIO_CAPTURE"

  @Test
  fun onlyCameraCaptureFromTheAppOriginIsAllowed() {
    assertTrue(WebViewPermissionPolicy.allowsCameraCapture("http://tauri.localhost/", arrayOf(video), null))
    assertTrue(WebViewPermissionPolicy.allowsCameraCapture("https://tauri.localhost", arrayOf(video), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("https://evil.example/", arrayOf(video), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("http://tauri.localhost.evil.example/", arrayOf(video), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("http://tauri.localhost:8080/", arrayOf(video), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("http://tauri.localhost/", arrayOf(video, audio), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("http://tauri.localhost/", arrayOf(audio), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("http://tauri.localhost/", emptyArray(), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture(null, arrayOf(video), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("file:///sdcard/a.html", arrayOf(video), null))
  }

  @Test
  fun developmentOriginIsAcceptedOnlyWhenSupplied() {
    val dev = "http://192.168.1.20:1420/index.html"
    assertTrue(WebViewPermissionPolicy.allowsCameraCapture("http://192.168.1.20:1420/", arrayOf(video), dev))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("http://192.168.1.20:1420/", arrayOf(video), null))
    assertFalse(WebViewPermissionPolicy.allowsCameraCapture("http://192.168.1.20:1421/", arrayOf(video), dev))
  }

  @Test
  fun originsAreNormalized() {
    assertEquals("http://tauri.localhost", WebViewPermissionPolicy.origin("HTTP://Tauri.Localhost:80/x?y"))
    assertEquals("https://a.example:8443", WebViewPermissionPolicy.origin("https://a.example:8443/"))
    assertNull(WebViewPermissionPolicy.origin("tauri://localhost/"))
    assertNull(WebViewPermissionPolicy.origin("not a url"))
  }
}
