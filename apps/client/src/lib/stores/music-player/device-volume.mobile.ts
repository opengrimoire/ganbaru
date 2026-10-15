import { parseDeviceVolumeState } from "$lib/music/device-volume";
import type { DeviceVolume } from "./device-volume-contracts";

const ANDROID_MEDIA_VOLUME_EVENT = "ganbaru:android-media-volume";

/**
 * Uses the phone's media volume as the Music volume, so the slider and the hardware buttons control the same level.
 * Devices with a fixed volume fall back to the app volume.
 */
export function createDeviceVolume(): DeviceVolume {
  function read(): number | null {
    const bridge = window.GanbaruAndroidMediaVolume;
    return bridge ? parseDeviceVolumeState(bridge.state()) : null;
  }

  function set(level: number): number | null {
    const bridge = window.GanbaruAndroidMediaVolume;
    return bridge ? parseDeviceVolumeState(bridge.setLevel(level)) : null;
  }

  function subscribe(listener: () => void): () => void {
    window.addEventListener(ANDROID_MEDIA_VOLUME_EVENT, listener);
    return () => window.removeEventListener(ANDROID_MEDIA_VOLUME_EVENT, listener);
  }

  return { read, set, subscribe };
}
