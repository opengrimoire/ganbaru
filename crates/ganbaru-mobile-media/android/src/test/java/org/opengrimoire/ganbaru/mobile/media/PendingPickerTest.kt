package org.opengrimoire.ganbaru.mobile.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class PendingPickerTest {
  private val picker = PendingPicker<String>("A music folder picker is already active")

  @Test
  fun secondRequestIsRefusedWhileOneIsActive() {
    picker.begin(1, "first")

    val error = assertThrows(IllegalStateException::class.java) { picker.begin(2, "second") }
    assertEquals("A music folder picker is already active", error.message)
  }

  @Test
  fun refusedRequestCannotClearTheActivePick() {
    picker.begin(1, "first")
    assertThrows(IllegalStateException::class.java) { picker.begin(2, "second") }

    picker.release(2)
    assertNull(picker.take(2))
    assertEquals("first", picker.take(1))
  }

  @Test
  fun owningCallbackTakesTheRequestOnceAndFreesThePicker() {
    picker.begin(1, "first")

    assertEquals("first", picker.take(1))
    assertNull(picker.take(1))
    picker.begin(2, "second")
    assertEquals("second", picker.take(2))
  }

  @Test
  fun releaseByTheOwnerFreesThePicker() {
    picker.begin(1, "first")

    picker.release(1)
    assertNull(picker.take(1))
    picker.begin(2, "second")
  }

  @Test
  fun takeWithoutAnActivePickReturnsNull() {
    assertNull(picker.take(1))
  }
}
