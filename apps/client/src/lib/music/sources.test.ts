import { describe, expect, it } from "vitest";
import {
  parseMusicSourceInput,
  parseTimestampMs,
  youtubeVideoIdFromIdentity,
  youtubeVideoSourceFromId,
} from "./sources";

describe("parseMusicSourceInput", () => {
  it("parses YouTube watch links with timestamps", () => {
    const result = parseMusicSourceInput("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1m20s");

    expect(result.error).toBeNull();
    expect(result.source).toMatchObject({
      kind: "youtube-video",
      videoId: "dQw4w9WgXcQ",
      identity: "youtube:video:dQw4w9WgXcQ",
      startMs: 80_000,
    });
  });

  it("parses YouTube Music playlist links", () => {
    const result = parseMusicSourceInput("https://music.youtube.com/playlist?list=PL1234567890");

    expect(result.error).toBeNull();
    expect(result.source).toMatchObject({
      kind: "youtube-playlist",
      playlistId: "PL1234567890",
      identity: "youtube:playlist:PL1234567890",
    });
  });

  it("prefers playlist identity when watch links include a list", () => {
    const result = parseMusicSourceInput("https://music.youtube.com/watch?v=abcDEF_1234&list=PLabcdef");

    expect(result.error).toBeNull();
    expect(result.source).toMatchObject({
      kind: "youtube-playlist",
      videoId: "abcDEF_1234",
      playlistId: "PLabcdef",
      identity: "youtube:playlist:PLabcdef:video:abcDEF_1234",
    });
  });

  it("parses YouTube playlist links without a starting video", () => {
    const result = parseMusicSourceInput("https://www.youtube.com/playlist?list=PLabcdef12345");

    expect(result.error).toBeNull();
    expect(result.source).toMatchObject({
      kind: "youtube-playlist",
      videoId: null,
      playlistId: "PLabcdef12345",
      identity: "youtube:playlist:PLabcdef12345",
    });
  });

  it("parses short youtu.be links", () => {
    const result = parseMusicSourceInput("https://youtu.be/dQw4w9WgXcQ?start=42");

    expect(result.error).toBeNull();
    expect(result.source).toMatchObject({
      kind: "youtube-video",
      videoId: "dQw4w9WgXcQ",
      startMs: 42_000,
    });
  });

  it("parses mobile YouTube links", () => {
    const result = parseMusicSourceInput("https://m.youtube.com/watch?v=dQw4w9WgXcQ");
    expect(result.error).toBeNull();
    expect(result.source).toMatchObject({ kind: "youtube-video", videoId: "dQw4w9WgXcQ" });
  });

  it("rejects malformed YouTube identities", () => {
    const result = parseMusicSourceInput("https://www.youtube.com/watch?v=bad!");
    expect(result.source).toBeNull();
    expect(result.error).toBe("invalid-youtube-video");
  });

  it("parses local absolute paths", () => {
    const result = parseMusicSourceInput("/home/person/Music/focus.flac");

    expect(result.error).toBeNull();
    expect(result.source).toMatchObject({
      kind: "local-file",
      path: "/home/person/Music/focus.flac",
      identity: "local:/home/person/Music/focus.flac",
      title: "focus",
    });
  });

  it("rejects unsupported remote URLs", () => {
    const result = parseMusicSourceInput("https://example.com/file.mp3");

    expect(result.source).toBeNull();
    expect(result.error).toBe("unsupported");
  });
});

describe("youtubeVideoIdFromIdentity", () => {
  it("extracts standalone video IDs without treating playlists or malformed identities as videos", () => {
    expect(youtubeVideoIdFromIdentity("youtube:video:dQw4w9WgXcQ")).toBe("dQw4w9WgXcQ");
    expect(youtubeVideoIdFromIdentity("youtube:playlist:PLabcdef:item:0:video:dQw4w9WgXcQ")).toBeNull();
    expect(youtubeVideoIdFromIdentity("youtube:video:bad!")).toBeNull();
  });
});

describe("youtubeVideoSourceFromId", () => {
  it("creates stable playlist queue entries", () => {
    expect(youtubeVideoSourceFromId("abcDEF_1234", {
      playlistId: "PLabcdef",
      playlistIndex: 2,
      startMs: 10_000,
    })).toMatchObject({
      kind: "youtube-video",
      videoId: "abcDEF_1234",
      playlistId: "PLabcdef",
      identity: "youtube:playlist:PLabcdef:item:2:video:abcDEF_1234",
      startMs: 10_000,
    });
  });
});

describe("parseTimestampMs", () => {
  it("parses seconds, token, and colon forms", () => {
    expect(parseTimestampMs("90")).toBe(90_000);
    expect(parseTimestampMs("1h2m3s")).toBe(3_723_000);
    expect(parseTimestampMs("02:03")).toBe(123_000);
    expect(parseTimestampMs("01:02:03")).toBe(3_723_000);
  });

  it("returns null for invalid timestamp text", () => {
    expect(parseTimestampMs("soon")).toBeNull();
    expect(parseTimestampMs("1x2m")).toBeNull();
  });
});
