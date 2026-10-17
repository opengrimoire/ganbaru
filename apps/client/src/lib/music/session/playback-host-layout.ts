export interface PlaybackHostRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface PixelAlignedPlaybackHostRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Expands a CSS rectangle to physical-pixel boundaries.
 *
 * This prevents the persistent media layer from leaving a partially covered
 * pixel around the placeholder when the WebView composites local video.
 */
export function coverPlaybackHostRect(
  rect: PlaybackHostRect,
  devicePixelRatio: number,
): PixelAlignedPlaybackHostRect {
  const scale = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0
    ? devicePixelRatio
    : 1;
  const left = Math.floor(rect.left * scale) / scale;
  const top = Math.floor(rect.top * scale) / scale;
  const right = Math.ceil(rect.right * scale) / scale;
  const bottom = Math.ceil(rect.bottom * scale) / scale;
  return {
    left,
    top,
    width: Math.max(0, right - left),
    height: Math.max(0, bottom - top),
  };
}
