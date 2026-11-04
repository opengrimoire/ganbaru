package org.opengrimoire.ganbaru.mobile.media

import org.junit.Assert.*
import org.junit.Test

class NativeMusicGrantTest {
  private val artwork = "content://com.android.providers.media.documents/document/image%3A42"
  private val tree = "content://com.android.externalstorage.documents/tree/primary%3AMusic"

  @Test
  fun directUriNeedsItsOwnPersistedGrantOrAnEnclosingTreeGrant() {
    val grants = listOf(artwork, tree)
    assertTrue(isCoveredByReadGrant(artwork, grants))
    assertTrue(isCoveredByReadGrant("$tree/document/primary%3AMusic%2Fcover.jpg", grants))
    assertFalse(isCoveredByReadGrant("content://org.opengrimoire.ganbaru.fileprovider/files/ganbaru.sqlite", grants))
    assertFalse(isCoveredByReadGrant("content://com.android.providers.media.documents/document/image%3A421", grants))
    assertFalse(isCoveredByReadGrant("${tree}Other/document/primary%3AMusicOther%2Fa.mp3", grants))
    assertFalse(isCoveredByReadGrant("$artwork/document/x", grants))
    assertFalse(isCoveredByReadGrant(artwork, emptyList()))
  }
}
