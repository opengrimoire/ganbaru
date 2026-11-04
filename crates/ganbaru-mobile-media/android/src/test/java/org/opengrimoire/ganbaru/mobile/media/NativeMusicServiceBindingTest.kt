package org.opengrimoire.ganbaru.mobile.media

import org.junit.Assert.*
import org.junit.Test

class NativeMusicServiceBindingTest {
  private class Fixture(var accepts: Boolean = true) {
    var binds = 0
    var unbinds = 0
    var rejectsUnregisteredUnbind = false
    var refusesWithSecurityException = false
    val binding = NativeMusicServiceBinding(
      {
        binds += 1
        if (refusesWithSecurityException) throw SecurityException("binding denied")
        accepts
      },
      {
        unbinds += 1
        if (rejectsUnregisteredUnbind) throw IllegalArgumentException("Service not registered")
      },
    )
  }

  @Test
  fun aRunningServiceHoldsExactlyOneBindingUntilReleased() {
    val fixture = Fixture()
    assertTrue(fixture.binding.hold())
    assertTrue(fixture.binding.hold())
    assertEquals(1, fixture.binds)
    assertTrue(fixture.binding.isHeld)
    fixture.binding.release()
    fixture.binding.release()
    assertEquals(1, fixture.unbinds)
    assertFalse(fixture.binding.isHeld)
  }

  @Test
  fun aRestartedServiceBindsAgainAfterRelease() {
    val fixture = Fixture()
    fixture.binding.hold()
    fixture.binding.release()
    assertTrue(fixture.binding.hold())
    assertEquals(2, fixture.binds)
    assertTrue(fixture.binding.isHeld)
  }

  @Test
  fun aRefusedBindingIsCleanedUpAndLeavesPlaybackUnprotectedRatherThanFailing() {
    val fixture = Fixture(accepts = false)
    fixture.rejectsUnregisteredUnbind = true
    assertFalse(fixture.binding.hold())
    assertEquals(1, fixture.unbinds)
    assertFalse(fixture.binding.isHeld)
    fixture.binding.release()
    assertEquals(1, fixture.unbinds)
    fixture.accepts = true
    fixture.rejectsUnregisteredUnbind = false
    assertTrue(fixture.binding.hold())
  }

  @Test
  fun aDeniedBindingDoesNotThrowIntoServiceCreation() {
    val fixture = Fixture()
    fixture.refusesWithSecurityException = true
    assertFalse(fixture.binding.hold())
    assertFalse(fixture.binding.isHeld)
    assertEquals(1, fixture.unbinds)
  }
}
