import { decodePairingQr } from "$lib/api/vault-handoff";

const IDEAL_CAMERA_WIDTH = 1920;
const IDEAL_CAMERA_HEIGHT = 1080;
const MAX_SCAN_SIDE = 1080;

export interface PairingFrameCrop {
  sourceX: number;
  sourceY: number;
  sourceSize: number;
  outputSize: number;
}

interface PairingCameraCapabilities extends MediaTrackCapabilities {
  focusMode?: string[];
}

type PairingCameraConstraintSet = MediaTrackConstraintSet & {
  focusMode?: "continuous";
};

/** Selects the centered square shown by the scanner without inventing extra pixels. */
export function pairingFrameCrop(sourceWidth: number, sourceHeight: number): PairingFrameCrop {
  if (!Number.isFinite(sourceWidth) || !Number.isFinite(sourceHeight)) {
    throw new Error("Invalid pairing camera dimensions");
  }
  const sourceSize = Math.floor(Math.min(sourceWidth, sourceHeight));
  if (sourceSize <= 0) throw new Error("Invalid pairing camera dimensions");
  return {
    sourceX: Math.floor((sourceWidth - sourceSize) / 2),
    sourceY: Math.floor((sourceHeight - sourceSize) / 2),
    sourceSize,
    outputSize: Math.min(sourceSize, MAX_SCAN_SIDE),
  };
}

/** Converts an RGBA camera frame into the bounded grayscale input used by the native decoder. */
export function pairingFrameToLuma(rgba: Uint8ClampedArray): Uint8Array {
  if (rgba.length % 4 !== 0) throw new Error("Invalid pairing camera frame");
  const luma = new Uint8Array(rgba.length / 4);
  for (let source = 0, target = 0; source < rgba.length; source += 4, target += 1) {
    luma[target] = Math.round(
      rgba[source] * 0.299 + rgba[source + 1] * 0.587 + rgba[source + 2] * 0.114,
    );
  }
  return luma;
}

/** Turns one bounded grayscale frame into the validated payload of the QR code it contains. */
export type QrFrameDecoder = (width: number, height: number, luma: Uint8Array) => Promise<string>;

/** Owns the Android WebView camera stream used to scan a desktop pairing invitation or a contact card. */
export class PairingCameraScanner {
  readonly #video: HTMLVideoElement;
  readonly #canvas = document.createElement("canvas");
  readonly #decode: QrFrameDecoder;
  #stream: MediaStream | null = null;

  constructor(video: HTMLVideoElement, decode: QrFrameDecoder = decodePairingQr) {
    this.#video = video;
    this.#decode = decode;
  }

  /** Requests the rear camera and attaches its preview to the supplied video element. */
  async start(): Promise<void> {
    this.stop();
    if (!navigator.mediaDevices?.getUserMedia) {
      throw new Error("Camera scanning is unavailable on this device");
    }
    this.#stream = await navigator.mediaDevices.getUserMedia({
      audio: false,
      video: {
        facingMode: { ideal: "environment" },
        width: { ideal: IDEAL_CAMERA_WIDTH },
        height: { ideal: IDEAL_CAMERA_HEIGHT },
      },
    });
    const videoTrack = this.#stream.getVideoTracks()[0];
    if (videoTrack) await enableContinuousFocus(videoTrack);
    this.#video.srcObject = this.#stream;
    this.#video.playsInline = true;
    await this.#video.play();
  }

  /** Decodes the current preview frame, returning the validated payload string. */
  async scanFrame(): Promise<string> {
    const sourceWidth = this.#video.videoWidth;
    const sourceHeight = this.#video.videoHeight;
    if (sourceWidth <= 0 || sourceHeight <= 0) {
      throw new Error("Pairing camera is not ready");
    }
    const crop = pairingFrameCrop(sourceWidth, sourceHeight);
    this.#canvas.width = crop.outputSize;
    this.#canvas.height = crop.outputSize;
    const context = this.#canvas.getContext("2d", { willReadFrequently: true });
    if (!context) throw new Error("Pairing camera frame is unavailable");
    context.drawImage(
      this.#video,
      crop.sourceX,
      crop.sourceY,
      crop.sourceSize,
      crop.sourceSize,
      0,
      0,
      crop.outputSize,
      crop.outputSize,
    );
    const rgba = context.getImageData(0, 0, crop.outputSize, crop.outputSize).data;
    return this.#decode(
      crop.outputSize,
      crop.outputSize,
      pairingFrameToLuma(rgba),
    );
  }

  /** Releases the camera immediately. */
  stop(): void {
    for (const track of this.#stream?.getTracks() ?? []) track.stop();
    this.#stream = null;
    this.#video.srcObject = null;
  }
}

async function enableContinuousFocus(track: MediaStreamTrack): Promise<void> {
  const capabilities = track.getCapabilities() as PairingCameraCapabilities;
  if (!capabilities.focusMode?.includes("continuous")) return;
  const advanced: PairingCameraConstraintSet[] = [{ focusMode: "continuous" }];
  try {
    await track.applyConstraints({ advanced });
  } catch {
    // Some Android WebViews advertise this constraint but reject it for the selected camera.
  }
}
