pub(crate) fn youtube_host_content_security_policy() -> &'static str {
    "default-src 'none'; script-src 'unsafe-inline' https://www.youtube.com; style-src 'unsafe-inline'; frame-src https://www.youtube.com https://www.youtube-nocookie.com; connect-src https://www.youtube.com https://*.youtube.com https://*.googlevideo.com; img-src https://i.ytimg.com https://*.ytimg.com data:; media-src https://*.googlevideo.com; object-src 'none'; base-uri 'none'; form-action 'none'"
}

pub(crate) fn youtube_host_html() -> &'static str {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="referrer" content="strict-origin-when-cross-origin">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Ganbaru AI YouTube player</title>
  <style>
    html, body, #player-root, #player { width: 100%; height: 100%; margin: 0; overflow: hidden; background: #000; }
  </style>
</head>
<body>
  <div id="player-root"><div id="player"></div></div>
  <script>
    const params = new URLSearchParams(location.search);
    const token = params.get("token") || "";
    const loadId = params.get("load") || "";
    let player = null;
    let apiReady = false;
    let activeSource = null;
    let playbackActive = false;
    let playlistSnapshotTimer = null;
    let playlistErrorSent = false;

    function numberParam(name, fallback) {
      const raw = params.get(name);
      if (raw === null || raw === "") return fallback;
      const value = Number(raw);
      return Number.isFinite(value) ? value : fallback;
    }

    function booleanParam(name) {
      return params.get(name) === "true";
    }

    function initialPayloadFromParams() {
      const kind = params.get("sourceKind");
      const videoId = params.get("videoId");
      const playlistId = params.get("playlistId");
      if (kind !== "youtube-video" && kind !== "youtube-playlist") return null;
      if (kind === "youtube-video" && !videoId) return null;
      if (kind === "youtube-playlist" && !playlistId) return null;

      const source = {
        kind,
        videoId,
        playlistId: playlistId || null,
        startMs: numberParam("startMs", null),
        endMs: numberParam("endMs", null)
      };
      return {
        source,
        resumeMs: numberParam("resumeMs", 0),
        volume: numberParam("volume", 0.8),
        rate: numberParam("rate", 1),
        autoplay: booleanParam("autoplay")
      };
    }

    function send(message) {
      parent.postMessage({ token, load: loadId, ...message }, "*");
    }

    function resetPlayerElement() {
      const root = document.getElementById("player-root");
      if (!root) return "player";
      root.replaceChildren();
      const element = document.createElement("div");
      element.id = "player";
      root.appendChild(element);
      return element;
    }

    function playbackStatus(state) {
      if (state === 0) return "ended";
      if (state === 1) return "playing";
      if (state === 2) return "paused";
      if (state === 3) return "loading";
      if (state === 5) return "ready";
      return "ready";
    }

    function snapshot(status) {
      if (!player) return;
      const duration = player.getDuration();
      const metadata = videoMetadata();
      send({
        type: "ganbaru-ai-youtube-state",
        status: status || playbackStatus(player.getPlayerState()),
        positionMs: Math.max(0, Math.round(player.getCurrentTime() * 1000)),
        durationMs: Number.isFinite(duration) && duration > 0 ? Math.round(duration * 1000) : null,
        videoId: metadata.videoId,
        title: metadata.title,
        channel: metadata.channel
      });
    }

    function currentPlaylistIds() {
      if (!player || typeof player.getPlaylist !== "function") return [];
      const playlist = player.getPlaylist();
      if (!Array.isArray(playlist)) return [];
      return playlist.filter((videoId) => typeof videoId === "string" && videoId.length > 0);
    }

    function currentPlaylistIndex() {
      if (!player || typeof player.getPlaylistIndex !== "function") return null;
      const index = player.getPlaylistIndex();
      return Number.isFinite(index) && index >= 0 ? index : null;
    }

    function sendPlaylistSnapshot(retries) {
      if (!activeSource || activeSource.kind !== "youtube-playlist" || !activeSource.playlistId) return;
      const videoIds = currentPlaylistIds();
      if (videoIds.length > 0) {
        send({
          type: "ganbaru-ai-youtube-playlist",
          playlistId: activeSource.playlistId,
          videoIds,
          index: currentPlaylistIndex()
        });
        return;
      }
      if (retries <= 0) {
        sendPlaylistResolutionError();
        return;
      }
      if (playlistSnapshotTimer) clearTimeout(playlistSnapshotTimer);
      playlistSnapshotTimer = setTimeout(() => {
        playlistSnapshotTimer = null;
        sendPlaylistSnapshot(retries - 1);
      }, 500);
    }

    function sendPlaylistResolutionError() {
      if (!activeSource || !activeSource.playlistId || playlistErrorSent) return;
      playlistErrorSent = true;
      send({
        type: "ganbaru-ai-youtube-playlist-error",
        playlistId: activeSource.playlistId
      });
    }

    function playlistRequest(source, payload) {
      const request = {
        listType: "playlist",
        list: source.playlistId,
        index: 0
      };
      const startSeconds = payload.resumeMs > 0
        ? payload.resumeMs / 1000
        : (source.startMs !== null ? Math.floor(source.startMs / 1000) : null);
      if (startSeconds !== null) request.startSeconds = startSeconds;
      return request;
    }

    function videoMetadata() {
      if (!player || typeof player.getVideoData !== "function") {
        return { videoId: null, title: null, channel: null };
      }
      const data = player.getVideoData();
      if (!data || typeof data !== "object") {
        return { videoId: null, title: null, channel: null };
      }
      const videoId = typeof data.video_id === "string" && data.video_id.trim()
        ? data.video_id.trim()
        : null;
      const title = typeof data.title === "string" && data.title.trim()
        ? data.title.trim()
        : null;
      const channel = typeof data.author === "string" && data.author.trim()
        ? data.author.trim()
        : null;
      return { videoId, title, channel };
    }

    function applyVolume(value) {
      if (!player || typeof value !== "number" || !Number.isFinite(value)) return;
      const volume = Math.max(0, Math.min(100, Math.round(value * 100)));
      player.setVolume(volume);
      if (volume > 0 && typeof player.unMute === "function") {
        player.unMute();
      } else if (volume === 0 && typeof player.mute === "function") {
        player.mute();
      }
    }

    function playerVars(source) {
      const vars = {
        autoplay: 0,
        controls: 0,
        disablekb: 1,
        fs: 0,
        iv_load_policy: 3,
        playsinline: 1,
        rel: 0,
        origin: location.origin,
        widget_referrer: location.href
      };
      if (source.kind === "youtube-playlist") {
        vars.listType = "playlist";
        vars.list = source.playlistId;
      }
      if (source.startMs !== null) vars.start = Math.floor(source.startMs / 1000);
      if (source.endMs !== null) vars.end = Math.floor(source.endMs / 1000);
      return vars;
    }

    function loadSource(payload) {
      if (!apiReady) return;
      if (player) {
        player.destroy();
        player = null;
      }
      playbackActive = false;
      const playerElement = resetPlayerElement();
      const source = payload.source;
      activeSource = source;
      playlistErrorSent = false;
      if (playlistSnapshotTimer) {
        clearTimeout(playlistSnapshotTimer);
        playlistSnapshotTimer = null;
      }
      const options = {
        host: "https://www.youtube.com",
        width: "100%",
        height: "100%",
        playerVars: playerVars(source),
        events: {
          onReady(event) {
            event.target.getIframe().setAttribute("referrerpolicy", "strict-origin-when-cross-origin");
            applyVolume(payload.volume);
            event.target.setPlaybackRate(payload.rate);
            if (source.kind === "youtube-playlist" && !source.videoId) {
              const request = playlistRequest(source, payload);
              if (payload.autoplay === true) {
                event.target.loadPlaylist(request);
              } else {
                event.target.cuePlaylist(request);
              }
            } else if (payload.resumeMs > 0) {
              event.target.seekTo(payload.resumeMs / 1000, true);
            }
            if (payload.autoplay === true) {
              event.target.playVideo();
            }
            snapshot("ready");
            sendPlaylistSnapshot(30);
          },
          onStateChange(event) {
            playbackActive = event.data === 1;
            snapshot(playbackStatus(event.data));
            sendPlaylistSnapshot(30);
          },
          onError(event) {
            playbackActive = false;
            send({ type: "ganbaru-ai-youtube-error", code: event.data });
          }
        }
      };
      if (source.kind === "youtube-video" || source.videoId) {
        options.videoId = source.kind === "youtube-video" ? source.videoId : source.videoId;
      }
      player = new YT.Player(playerElement, options);
    }

    function handleCommand(data) {
      if (!player) return;
      if (data.action === "snapshot") {
        snapshot();
        return;
      }
      if (data.action === "play") {
        const volume = typeof data.volume === "number" ? data.volume : null;
        applyVolume(volume);
        player.playVideo();
        playbackActive = true;
        if (volume !== null) {
          setTimeout(() => applyVolume(volume), 0);
          setTimeout(() => applyVolume(volume), 150);
        }
        snapshot("playing");
        return;
      }
      if (data.action === "pause") {
        applyVolume(data.volume);
        player.pauseVideo();
        playbackActive = false;
        snapshot("paused");
        return;
      }
      if (data.action === "stop") {
        playbackActive = false;
        player.stopVideo();
      }
      if (data.action === "seek") player.seekTo(data.positionMs / 1000, true);
      if (data.action === "volume") applyVolume(data.volume);
      if (data.action === "rate") player.setPlaybackRate(data.rate);
      snapshot();
    }

    window.onYouTubeIframeAPIReady = () => {
      apiReady = true;
      send({ type: "ganbaru-ai-youtube-ready" });
      const initialLoad = initialPayloadFromParams();
      if (initialLoad) {
        loadSource(initialLoad);
      }
    };

    window.addEventListener("message", (event) => {
      if (event.source !== parent) return;
      const data = event.data;
      if (!data || data.token !== token || data.type !== "ganbaru-ai-youtube-command") return;
      handleCommand(data);
    });

    window.setInterval(() => {
      if (player && playbackActive) snapshot();
    }, 1000);
  </script>
  <script src="https://www.youtube.com/iframe_api"></script>
</body>
</html>"#
}
