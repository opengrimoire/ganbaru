import { describe, expect, it } from "vitest";
import {
  localMediaHasVideo,
  localMediaSrc,
  localVideoReady,
  NO_WEBVIEW_LOCAL_MEDIA,
  transitionWebviewLocalMedia,
  type WebviewLocalMedia,
} from "./local-media-state";

const pending: WebviewLocalMedia = { phase: "loaded", src: "http://media/a", video: "pending" };

describe("WebView local media state", () => {
  it("starts with nothing loaded", () => {
    expect(localMediaSrc(NO_WEBVIEW_LOCAL_MEDIA)).toBeNull();
    expect(localMediaHasVideo(NO_WEBVIEW_LOCAL_MEDIA)).toBe(false);
    expect(localVideoReady(NO_WEBVIEW_LOCAL_MEDIA)).toBe(false);
  });

  it("treats a loaded file as video until its first frame decodes", () => {
    const loaded = transitionWebviewLocalMedia(NO_WEBVIEW_LOCAL_MEDIA, { type: "loaded", src: "http://media/a" });
    expect(loaded).toEqual(pending);
    expect(localMediaSrc(loaded)).toBe("http://media/a");
    expect(localMediaHasVideo(loaded)).toBe(true);
    expect(localVideoReady(loaded)).toBe(false);

    const ready = transitionWebviewLocalMedia(loaded, { type: "videoReady" });
    expect(localVideoReady(ready)).toBe(true);
    expect(transitionWebviewLocalMedia(ready, { type: "videoReady" })).toBe(ready);
  });

  it("replaces an earlier file and its readiness when another one loads", () => {
    const ready = transitionWebviewLocalMedia(pending, { type: "videoReady" });
    const next = transitionWebviewLocalMedia(ready, { type: "loaded", src: "http://media/b" });
    expect(next).toEqual({ phase: "loaded", src: "http://media/b", video: "pending" });
  });

  it("drops the video for artwork while keeping the loaded file", () => {
    const audio = transitionWebviewLocalMedia(pending, { type: "artworkShown" });
    expect(localMediaSrc(audio)).toBe("http://media/a");
    expect(localMediaHasVideo(audio)).toBe(false);
    expect(transitionWebviewLocalMedia(audio, { type: "artworkShown" })).toBe(audio);
  });

  it("ignores a decoded frame once the file plays as audio, so the video never reappears", () => {
    const audio = transitionWebviewLocalMedia(pending, { type: "artworkShown" });
    const after = transitionWebviewLocalMedia(audio, { type: "videoReady" });
    expect(after).toBe(audio);
    expect(localMediaHasVideo(after)).toBe(false);
  });

  it("ignores readiness and artwork while nothing is loaded", () => {
    expect(transitionWebviewLocalMedia(NO_WEBVIEW_LOCAL_MEDIA, { type: "videoReady" })).toBe(NO_WEBVIEW_LOCAL_MEDIA);
    expect(transitionWebviewLocalMedia(NO_WEBVIEW_LOCAL_MEDIA, { type: "artworkShown" })).toBe(NO_WEBVIEW_LOCAL_MEDIA);
  });

  it("clears every loaded variant on reset", () => {
    for (const state of [
      pending,
      { ...pending, video: "ready" },
      { ...pending, video: "none" },
    ] satisfies WebviewLocalMedia[]) {
      expect(transitionWebviewLocalMedia(state, { type: "reset" })).toBe(NO_WEBVIEW_LOCAL_MEDIA);
    }
    expect(transitionWebviewLocalMedia(NO_WEBVIEW_LOCAL_MEDIA, { type: "reset" })).toBe(NO_WEBVIEW_LOCAL_MEDIA);
  });
});
