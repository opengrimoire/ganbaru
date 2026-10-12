package org.opengrimoire.ganbaruai.mobile.media

import androidx.media3.common.Player

/** Only semantic transport controls and decoder reads are exposed to controllers. */
internal object NativeMusicCommands {
  private val supported = setOf(
    Player.COMMAND_PLAY_PAUSE,
    Player.COMMAND_STOP,
    Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
    Player.COMMAND_SEEK_BACK,
    Player.COMMAND_SEEK_FORWARD,
    Player.COMMAND_SEEK_TO_NEXT,
    Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
    Player.COMMAND_SEEK_TO_PREVIOUS,
    Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
    Player.COMMAND_SET_VOLUME,
    Player.COMMAND_SET_SPEED_AND_PITCH,
    Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
    Player.COMMAND_GET_TIMELINE,
    Player.COMMAND_GET_METADATA,
    Player.COMMAND_GET_AUDIO_ATTRIBUTES,
    Player.COMMAND_GET_VOLUME,
    Player.COMMAND_GET_TRACKS,
  )

  fun allows(command: Int): Boolean = command in supported

  fun available(): Player.Commands = Player.Commands.Builder().addAll(*supported.toIntArray()).build()
}
