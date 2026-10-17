# Packaged audio assets

Ganbaru uses short packaged sounds for Calendar reminders, Pomodoro attention and completion states, and future response-completion cues. This document defines development and maintenance rules for those assets.

## Packaged format

Files under `apps/client/static/sfx/` use:

- WAV container.
- Signed 16-bit little-endian PCM.
- Stereo channels.
- 48 kHz sample rate.

Downloaded or recorded source material can use another format, but it must be converted before packaging.

The fixed format avoids inconsistent runtime decoding and unnecessary low-quality resampling on the normal desktop output path. User-owned Music files retain their own source format and are not subject to this rule.

## Conversion

Use FFmpeg with the high-quality `soxr` resampler:

```bash
ffmpeg -y -hide_banner -loglevel error -i input.ext -af aresample=resampler=soxr:precision=28:dither_method=triangular_hp -ar 48000 -ac 2 -c:a pcm_s16le apps/client/static/sfx/name.wav
```

Verify the result:

```bash
file apps/client/static/sfx/*
```

Expected shape:

```text
RIFF (little-endian) data, WAVE audio, Microsoft PCM, 16 bit, stereo 48000 Hz
```

## Playback boundary

Packaged attention sounds use a dedicated Rust app-sound service, separate from the user's Music player. The service starts lazily, retains one output stream, queues work through a bounded worker, and recovers from output-device errors.

Notification sounds do not change Music source, queue, pause, seek, rate, mute, or saved volume.

Pomodoro terminal completion can temporarily duck and pause active Music because the completion cue is the primary event-end signal. It restores prior playback and volume only when that orchestration still owns the temporary change. Saved user volume is never replaced by the ducked value.

## Maintenance

- Keep all packaged attention sounds in the declared format.
- Validate new assets through the actual app-sound path, not only an external player.
- Keep source attribution and licensing synchronized with user-visible credits.
- Do not replace packaged attention sounds with browser-generated notification sounds.
- Test output-device recovery and Music ducking when changing the sound service.
