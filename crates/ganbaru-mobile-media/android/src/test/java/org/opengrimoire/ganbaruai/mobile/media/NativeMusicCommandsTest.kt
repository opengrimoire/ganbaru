package org.opengrimoire.ganbaruai.mobile.media

import androidx.media3.common.Player
import org.junit.Assert.*
import org.junit.Test

class NativeMusicCommandsTest {
  @Test
  fun trustedControllersCannotReplaceOrPrepareTheNativeOwnersQueue() {
    for (command in listOf(
      Player.COMMAND_CHANGE_MEDIA_ITEMS, Player.COMMAND_SET_MEDIA_ITEM,
      Player.COMMAND_PREPARE, Player.COMMAND_RELEASE,
      Player.COMMAND_SET_REPEAT_MODE, Player.COMMAND_SET_SHUFFLE_MODE,
      Player.COMMAND_SET_AUDIO_ATTRIBUTES, Player.COMMAND_SET_TRACK_SELECTION_PARAMETERS,
    )) assertFalse("Direct decoder command $command must be unavailable", NativeMusicCommands.allows(command))
    assertFalse(NativeMusicCommands.allows(Int.MAX_VALUE))
  }

  @Test
  fun controllerTransportAndDisplayReadsRemainAvailable() {
    for (command in listOf(
      Player.COMMAND_PLAY_PAUSE, Player.COMMAND_STOP,
      Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_PREVIOUS,
      Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
      Player.COMMAND_SET_VOLUME, Player.COMMAND_SET_SPEED_AND_PITCH,
      Player.COMMAND_GET_CURRENT_MEDIA_ITEM, Player.COMMAND_GET_METADATA,
    )) assertTrue("Semantic control or read $command must remain available", NativeMusicCommands.allows(command))
  }
}
