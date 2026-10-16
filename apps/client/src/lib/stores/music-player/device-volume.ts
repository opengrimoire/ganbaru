import type { DeviceVolume } from "./device-volume-contracts";

/** Desktop keeps the Music volume inside the app, apart from the system volume. */
export function createDeviceVolume(): DeviceVolume {
  return { read: () => null, set: () => null, subscribe: () => () => {} };
}
