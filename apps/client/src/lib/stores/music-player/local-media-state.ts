/**
 * What the WebView local media element shows for the loaded file: the video still waiting for its first frame, the
 * decoded video, or no video because the file plays as audio behind its artwork.
 */
export type LocalVideoDisplay = "pending" | "ready" | "none";

/** The local file the WebView media element plays, if any. */
export type WebviewLocalMedia =
  | { phase: "none" }
  | { phase: "loaded"; src: string; video: LocalVideoDisplay };

export type WebviewLocalMediaEvent =
  | { type: "loaded"; src: string }
  | { type: "videoReady" }
  | { type: "artworkShown" }
  | { type: "reset" };

export const NO_WEBVIEW_LOCAL_MEDIA: WebviewLocalMedia = { phase: "none" };

/** Applies one local media event; an event that changes nothing returns the same state. */
export function transitionWebviewLocalMedia(
  state: WebviewLocalMedia,
  event: WebviewLocalMediaEvent,
): WebviewLocalMedia {
  switch (event.type) {
    case "loaded":
      return { phase: "loaded", src: event.src, video: "pending" };
    case "videoReady":
      return state.phase === "loaded" && state.video === "pending" ? { ...state, video: "ready" } : state;
    case "artworkShown":
      return state.phase === "loaded" && state.video !== "none" ? { ...state, video: "none" } : state;
    case "reset":
      return state.phase === "none" ? state : NO_WEBVIEW_LOCAL_MEDIA;
  }
}

/** The media URL the element plays, or `null` when no local file is loaded. */
export function localMediaSrc(state: WebviewLocalMedia): string | null {
  return state.phase === "loaded" ? state.src : null;
}

/** Whether the loaded file shows video rather than artwork. */
export function localMediaHasVideo(state: WebviewLocalMedia): boolean {
  return state.phase === "loaded" && state.video !== "none";
}

/** Whether the loaded video has decoded its first frame. */
export function localVideoReady(state: WebviewLocalMedia): boolean {
  return state.phase === "loaded" && state.video === "ready";
}
