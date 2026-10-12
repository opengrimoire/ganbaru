//! Desktop playback of local media files independent of Tauri: file inspection, decoding on a
//! dedicated worker thread, and execution of committed Music session effects under the
//! delivery authority supplied by the caller.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use ganbaru_music_library::session::models::SessionEffect;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source};
use serde::{Deserialize, Serialize};

mod worker;
use worker::{DeliveryAuthority, PlaybackController};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    None,
    Rodio,
    Webview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MediaKind {
    Audio,
    Video,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PlayerStatus {
    Idle,
    Ready,
    Playing,
    Paused,
    Ended,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalMediaSource {
    pub kind: String,
    pub path: String,
    pub identity: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadRequest {
    pub source: LocalMediaSource,
    pub start_ms: Option<u64>,
    pub volume: Option<f64>,
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaProbe {
    pub path: String,
    pub title: String,
    pub file_size_bytes: u64,
    pub extension: Option<String>,
    pub media_kind: MediaKind,
    pub playable_start_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub status: PlayerStatus,
    pub source_identity: Option<String>,
    pub title: Option<String>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub has_video: bool,
    pub backend_kind: BackendKind,
    pub playable_start_ms: Option<u64>,
    pub error: Option<String>,
}

impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            status: PlayerStatus::Idle,
            source_identity: None,
            title: None,
            position_ms: 0,
            duration_ms: None,
            volume: 0.8,
            muted: false,
            rate: 1.0,
            has_video: false,
            backend_kind: BackendKind::None,
            playable_start_ms: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaPlayerError {
    pub code: String,
    pub message: String,
}

impl MediaPlayerError {
    fn invalid_source(message: impl Into<String>) -> Self {
        Self {
            code: "invalidSource".to_string(),
            message: message.into(),
        }
    }

    fn backend_unavailable() -> Self {
        Self {
            code: "backendUnavailable".to_string(),
            message: "The requested local media backend is not available.".to_string(),
        }
    }

    fn audio_device(message: impl Into<String>) -> Self {
        Self {
            code: "audioDevice".to_string(),
            message: message.into(),
        }
    }

    fn decode_failed(message: impl Into<String>) -> Self {
        Self {
            code: "decodeFailed".to_string(),
            message: message.into(),
        }
    }

    fn seek_failed(message: impl Into<String>) -> Self {
        Self {
            code: "seekFailed".to_string(),
            message: message.into(),
        }
    }

    fn backend_thread() -> Self {
        Self {
            code: "backendThread".to_string(),
            message: "The Rust media player backend is temporarily unavailable.".to_string(),
        }
    }

    fn delivery_revoked() -> Self {
        Self {
            code: "deliveryRevoked".into(),
            message: "Native Music delivery expired or its authority changed.".into(),
        }
    }

    /// Source failures can advance the queue; transport and device failures cannot.
    pub fn is_source_failure(&self) -> bool {
        matches!(self.code.as_str(), "invalidSource" | "decodeFailed")
    }
}

trait LocalAudioBackend: Send {
    fn play(&mut self);
    fn pause(&mut self);
    fn stop(&mut self);
    fn seek(&mut self, position_ms: u64) -> Result<(), MediaPlayerError>;
    fn set_volume(&mut self, volume: f64, muted: bool);
    fn set_rate(&mut self, rate: f64);
    fn position_ms(&self) -> u64;
    fn duration_ms(&self) -> Option<u64>;
    fn is_empty(&self) -> bool;
}

trait LocalAudioFactory: Send {
    fn load(
        &mut self,
        request: &LoadRequest,
        volume: f64,
        muted: bool,
        rate: f64,
    ) -> Result<Box<dyn LocalAudioBackend>, MediaPlayerError>;
}

struct RodioAudioFactory;

impl LocalAudioFactory for RodioAudioFactory {
    fn load(
        &mut self,
        request: &LoadRequest,
        volume: f64,
        muted: bool,
        rate: f64,
    ) -> Result<Box<dyn LocalAudioBackend>, MediaPlayerError> {
        let file = File::open(&request.source.path).map_err(|e| {
            MediaPlayerError::invalid_source(format!("Failed to open local audio file: {e}"))
        })?;
        let decoder = Decoder::try_from(file).map_err(|e| {
            MediaPlayerError::decode_failed(format!("Failed to decode local audio file: {e}"))
        })?;
        let duration_ms = duration_to_ms(decoder.total_duration());
        let mut sink = open_playback_sink(decoder.sample_rate())?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        player.pause();
        player.set_volume(effective_native_volume(volume, muted));
        player.set_speed(clamp_rate(rate) as f32);
        player.append(decoder);
        if let Some(start_ms) = request.start_ms.filter(|value| *value > 0) {
            player
                .try_seek(Duration::from_millis(start_ms))
                .map_err(|e| {
                    MediaPlayerError::seek_failed(format!("Failed to seek local audio file: {e}"))
                })?;
        }
        Ok(Box::new(RodioAudioBackend {
            _sink: sink,
            player,
            duration_ms,
        }))
    }
}

struct RodioAudioBackend {
    _sink: MixerDeviceSink,
    player: Player,
    duration_ms: Option<u64>,
}

impl LocalAudioBackend for RodioAudioBackend {
    fn play(&mut self) {
        self.player.play();
    }

    fn pause(&mut self) {
        self.player.set_volume(0.0);
        self.player.pause();
    }

    fn stop(&mut self) {
        self.player.set_volume(0.0);
        self.player.stop();
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), MediaPlayerError> {
        self.player
            .try_seek(Duration::from_millis(position_ms))
            .map_err(|e| MediaPlayerError::seek_failed(format!("Failed to seek local audio: {e}")))
    }

    fn set_volume(&mut self, volume: f64, muted: bool) {
        self.player
            .set_volume(effective_native_volume(volume, muted));
    }

    fn set_rate(&mut self, rate: f64) {
        self.player.set_speed(clamp_rate(rate) as f32);
    }

    fn position_ms(&self) -> u64 {
        duration_to_ms(Some(self.player.get_pos())).unwrap_or(0)
    }

    fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    fn is_empty(&self) -> bool {
        self.player.empty()
    }
}

struct PlayerCore {
    audio_factory: Box<dyn LocalAudioFactory>,
    audio: Option<Box<dyn LocalAudioBackend>>,
    loaded: bool,
    snapshot: PlayerSnapshot,
}

impl Default for PlayerCore {
    fn default() -> Self {
        Self::with_audio_factory(Box::new(RodioAudioFactory))
    }
}

impl PlayerCore {
    fn with_audio_factory(audio_factory: Box<dyn LocalAudioFactory>) -> Self {
        Self {
            audio_factory,
            audio: None,
            loaded: false,
            snapshot: PlayerSnapshot::default(),
        }
    }

    #[cfg(test)]
    fn handle(&mut self, command: BackendCommand) -> Result<PlayerSnapshot, MediaPlayerError> {
        self.handle_authorized(command, &DeliveryAuthority::unrestricted())
    }

    fn handle_authorized(
        &mut self,
        command: BackendCommand,
        authority: &DeliveryAuthority,
    ) -> Result<PlayerSnapshot, MediaPlayerError> {
        match command {
            BackendCommand::Session(effect) => self.apply_session_effect(*effect, authority),
            #[cfg(test)]
            BackendCommand::Load { request, probe } => self.load(*request, *probe),
            #[cfg(test)]
            BackendCommand::Play => self.play(),
            #[cfg(test)]
            BackendCommand::Pause => Ok(self.pause()),
            #[cfg(test)]
            BackendCommand::Stop => Ok(self.stop()),
            #[cfg(test)]
            BackendCommand::Seek(position_ms) => self.seek(position_ms),
            #[cfg(test)]
            BackendCommand::SetMuted(muted) => Ok(self.set_muted(muted)),
            BackendCommand::Snapshot => Ok(self.current_snapshot()),
        }
    }

    fn load(
        &mut self,
        request: LoadRequest,
        probe: MediaProbe,
    ) -> Result<PlayerSnapshot, MediaPlayerError> {
        self.stop_active_backend();
        let volume = request
            .volume
            .map(clamp_volume)
            .unwrap_or(self.snapshot.volume);
        let rate = request.rate.map(clamp_rate).unwrap_or(self.snapshot.rate);
        let title = request
            .source
            .title
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| probe.title.clone());
        let has_video = probe.media_kind == MediaKind::Video;
        let playable_start_ms = if has_video {
            probe.playable_start_ms.filter(|value| *value > 0)
        } else {
            None
        };
        let (audio, backend_kind) = if has_video {
            (None, BackendKind::Webview)
        } else {
            (
                Some(
                    self.audio_factory
                        .load(&request, volume, self.snapshot.muted, rate)?,
                ),
                BackendKind::Rodio,
            )
        };
        let duration_ms = audio.as_ref().and_then(|backend| backend.duration_ms());
        self.loaded = true;
        self.audio = audio;
        self.snapshot = PlayerSnapshot {
            status: PlayerStatus::Ready,
            source_identity: Some(request.source.identity),
            title: Some(title),
            position_ms: request.start_ms.unwrap_or(0),
            duration_ms,
            volume,
            muted: self.snapshot.muted,
            rate,
            has_video,
            backend_kind,
            playable_start_ms,
            error: None,
        };
        Ok(self.snapshot.clone())
    }

    #[cfg(test)]
    fn play(&mut self) -> Result<PlayerSnapshot, MediaPlayerError> {
        self.play_authorized(&DeliveryAuthority::unrestricted())
    }

    fn play_authorized(
        &mut self,
        authority: &DeliveryAuthority,
    ) -> Result<PlayerSnapshot, MediaPlayerError> {
        if !self.loaded {
            self.snapshot.status = PlayerStatus::Error;
            self.snapshot.error = Some("Load a local media file before playing.".to_string());
            return Err(MediaPlayerError {
                code: "noPreparedSource".into(),
                message: "Load a local media file before playing.".into(),
            });
        }
        if self.snapshot.status == PlayerStatus::Ended {
            self.seek(0)?;
        }
        authority.require_current()?;
        if self.audio.is_none() {
            let error = MediaPlayerError::backend_unavailable();
            self.snapshot.status = PlayerStatus::Error;
            self.snapshot.error = Some(error.message.clone());
            return Err(error);
        }
        if let Some(audio) = self.audio.as_mut() {
            audio.set_volume(self.snapshot.volume, self.snapshot.muted);
            audio.play();
        }
        self.snapshot.status = PlayerStatus::Playing;
        self.snapshot.error = None;
        Ok(self.current_snapshot())
    }

    fn pause(&mut self) -> PlayerSnapshot {
        if self.loaded {
            if let Some(audio) = self.audio.as_mut() {
                audio.pause();
            }
            self.snapshot.status = PlayerStatus::Paused;
            self.snapshot.error = None;
        }
        self.current_snapshot()
    }

    fn stop(&mut self) -> PlayerSnapshot {
        let volume = self.snapshot.volume;
        let rate = self.snapshot.rate;
        let muted = self.snapshot.muted;
        self.stop_active_backend();
        self.loaded = false;
        self.snapshot = PlayerSnapshot {
            volume,
            muted,
            rate,
            ..PlayerSnapshot::default()
        };
        self.snapshot.clone()
    }

    fn seek(&mut self, position_ms: u64) -> Result<PlayerSnapshot, MediaPlayerError> {
        if let Some(audio) = self.audio.as_mut() {
            audio.seek(position_ms)?;
        }
        self.snapshot.position_ms = position_ms;
        Ok(self.current_snapshot())
    }

    fn set_volume(&mut self, volume: f64) -> PlayerSnapshot {
        self.snapshot.volume = clamp_volume(volume);
        if let Some(audio) = self.audio.as_mut() {
            audio.set_volume(self.snapshot.volume, self.snapshot.muted);
        }
        self.current_snapshot()
    }

    fn set_muted(&mut self, muted: bool) -> PlayerSnapshot {
        self.snapshot.muted = muted;
        if let Some(audio) = self.audio.as_mut() {
            audio.set_volume(self.snapshot.volume, self.snapshot.muted);
        }
        self.current_snapshot()
    }

    fn set_rate(&mut self, rate: f64) -> PlayerSnapshot {
        self.snapshot.rate = clamp_rate(rate);
        if let Some(audio) = self.audio.as_mut() {
            audio.set_rate(self.snapshot.rate);
        }
        self.current_snapshot()
    }

    fn current_snapshot(&mut self) -> PlayerSnapshot {
        if let Some(audio) = self.audio.as_ref() {
            self.snapshot.position_ms = audio.position_ms();
            self.snapshot.duration_ms = audio.duration_ms();
            if self.snapshot.status == PlayerStatus::Playing && audio.is_empty() {
                self.snapshot.status = PlayerStatus::Ended;
                if let Some(duration_ms) = self.snapshot.duration_ms {
                    self.snapshot.position_ms = duration_ms;
                }
            }
        }
        self.snapshot.clone()
    }

    fn stop_active_backend(&mut self) {
        if let Some(mut audio) = self.audio.take() {
            audio.stop();
        }
    }
}

#[derive(Debug)]
enum BackendCommand {
    Session(Box<SessionEffect>),
    #[cfg(test)]
    Load {
        request: Box<LoadRequest>,
        probe: Box<MediaProbe>,
    },
    #[cfg(test)]
    Play,
    #[cfg(test)]
    Pause,
    #[cfg(test)]
    Stop,
    #[cfg(test)]
    Seek(u64),
    #[cfg(test)]
    SetMuted(bool),
    Snapshot,
}

#[cfg(test)]
impl BackendCommand {
    fn load(request: LoadRequest, probe: MediaProbe) -> Self {
        Self::Load {
            request: Box::new(request),
            probe: Box::new(probe),
        }
    }
}

#[derive(Debug, Default)]
pub struct MediaPlayerState {
    controller: PlaybackController,
}

/// Executes a committed application-session effect on the decoder worker.
pub fn apply_session_effect(
    state: &MediaPlayerState,
    effect: &SessionEffect,
    authority: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<PlayerSnapshot, MediaPlayerError> {
    state
        .controller
        .dispatch_authorized(BackendCommand::Session(Box::new(effect.clone())), authority)
}

impl PlayerCore {
    /// Applies one effect on the existing decoder worker, including blocking preparation.
    fn apply_session_effect(
        &mut self,
        effect: SessionEffect,
        authority: &DeliveryAuthority,
    ) -> Result<PlayerSnapshot, MediaPlayerError> {
        match effect {
            SessionEffect::Load {
                source,
                position_ms,
                volume,
                muted,
                rate,
                autoplay,
                ..
            } => {
                let request = LoadRequest {
                    source: LocalMediaSource {
                        kind: "local-file".into(),
                        path: source.path.ok_or_else(|| {
                            MediaPlayerError::invalid_source(
                                "Local session source has no resolved path",
                            )
                        })?,
                        identity: source.identity,
                        title: Some(source.title),
                    },
                    start_ms: Some(position_ms),
                    volume: Some(volume),
                    rate: Some(rate),
                };
                validate_load_request(&request)?;
                self.stop();
                let probe = probe_local_file(&request.source.path)?;
                authority.require_current()?;
                self.load(request, probe)?;
                authority.require_current()?;
                self.set_muted(muted);
                if autoplay {
                    self.play_authorized(authority)
                } else {
                    Ok(self.current_snapshot())
                }
            }
            SessionEffect::Play { .. } => self.play_authorized(authority),
            SessionEffect::Pause { .. } => Ok(self.pause()),
            SessionEffect::Stop { .. } => Ok(self.stop()),
            SessionEffect::Seek { position_ms, .. } => self.seek(position_ms),
            SessionEffect::Settings {
                volume,
                muted,
                rate,
                ..
            } => {
                self.set_volume(volume);
                self.set_muted(muted);
                Ok(self.set_rate(rate))
            }
        }
    }
}

/// Samples the decoder from the native session scheduler, independently of the WebView.
pub fn session_snapshot(state: &MediaPlayerState) -> Result<PlayerSnapshot, MediaPlayerError> {
    state.controller.dispatch(BackendCommand::Snapshot)
}

fn validate_load_request(request: &LoadRequest) -> Result<(), MediaPlayerError> {
    if request.source.kind != "local-file" {
        return Err(MediaPlayerError::invalid_source(
            "The Rust media player module only accepts local-file sources.",
        ));
    }
    if request.source.identity.trim().is_empty() {
        return Err(MediaPlayerError::invalid_source(
            "Local media sources require a stable identity.",
        ));
    }
    Ok(())
}

pub fn probe_local_file(path: &str) -> Result<MediaProbe, MediaPlayerError> {
    let path = PathBuf::from(path);
    validate_local_file_path(&path)?;
    let metadata = std::fs::metadata(&path).map_err(|e| {
        MediaPlayerError::invalid_source(format!("Failed to inspect local media file: {e}"))
    })?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    let media_kind = extension
        .as_deref()
        .map(media_kind_from_extension)
        .unwrap_or(MediaKind::Unknown);
    let playable_start_ms = probe_playable_start_ms(&path, extension.as_deref(), &media_kind)
        .filter(|value| *value > 0);
    Ok(MediaProbe {
        path: path.to_string_lossy().to_string(),
        title: media_title_from_path(&path),
        file_size_bytes: metadata.len(),
        media_kind,
        playable_start_ms,
        extension,
    })
}

fn probe_playable_start_ms(
    path: &Path,
    extension: Option<&str>,
    media_kind: &MediaKind,
) -> Option<u64> {
    if *media_kind != MediaKind::Video || !is_mp4_edit_list_probe_candidate(extension) {
        return None;
    }

    probe_mp4_leading_empty_edit_ms(path)
}

fn is_mp4_edit_list_probe_candidate(extension: Option<&str>) -> bool {
    matches!(extension, Some("m4v" | "mov" | "mp4"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mp4BoxHeader {
    name: [u8; 4],
    payload_start: u64,
    end: u64,
}

fn probe_mp4_leading_empty_edit_ms(path: &Path) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let moov = find_mp4_child_box(&mut file, 0, file_len, *b"moov").ok()??;
    let movie_timescale = read_mp4_movie_timescale(&mut file, moov).ok()??;
    if movie_timescale == 0 {
        return None;
    }

    let mut starts = Vec::new();
    file.seek(SeekFrom::Start(moov.payload_start)).ok()?;
    while file.stream_position().ok()? < moov.end {
        let Some(child) = read_mp4_box_header(&mut file, moov.end).ok()? else {
            break;
        };
        if child.name == *b"trak" {
            if let Some(start_ms) =
                read_mp4_track_leading_empty_edit_ms(&mut file, child, movie_timescale)
                    .ok()
                    .flatten()
            {
                starts.push(start_ms);
            }
        }
        file.seek(SeekFrom::Start(child.end)).ok()?;
    }

    starts.into_iter().filter(|start_ms| *start_ms > 0).min()
}

fn read_mp4_movie_timescale(file: &mut File, moov: Mp4BoxHeader) -> std::io::Result<Option<u32>> {
    let Some(mvhd) = find_mp4_child_box(file, moov.payload_start, moov.end, *b"mvhd")? else {
        return Ok(None);
    };
    file.seek(SeekFrom::Start(mvhd.payload_start))?;
    let version = read_u8(file)?;
    skip_bytes(file, 3)?;
    let timescale_offset = if version == 1 { 16 } else { 8 };
    skip_bytes(file, timescale_offset)?;
    Ok(Some(read_be_u32(file)?))
}

fn read_mp4_track_leading_empty_edit_ms(
    file: &mut File,
    track: Mp4BoxHeader,
    movie_timescale: u32,
) -> std::io::Result<Option<u64>> {
    let Some(edts) = find_mp4_child_box(file, track.payload_start, track.end, *b"edts")? else {
        return Ok(None);
    };
    let Some(elst) = find_mp4_child_box(file, edts.payload_start, edts.end, *b"elst")? else {
        return Ok(None);
    };
    read_mp4_elst_leading_empty_edit_ms(file, elst, movie_timescale)
}

fn read_mp4_elst_leading_empty_edit_ms(
    file: &mut File,
    elst: Mp4BoxHeader,
    movie_timescale: u32,
) -> std::io::Result<Option<u64>> {
    file.seek(SeekFrom::Start(elst.payload_start))?;
    let version = read_u8(file)?;
    skip_bytes(file, 3)?;
    let entry_count = read_be_u32(file)?;
    let mut leading_empty_duration = 0_u64;

    for _ in 0..entry_count {
        let (segment_duration, media_time) = if version == 1 {
            (read_be_u64(file)?, read_be_i64(file)?)
        } else {
            (u64::from(read_be_u32(file)?), i64::from(read_be_i32(file)?))
        };
        skip_bytes(file, 4)?;
        if media_time == -1 {
            leading_empty_duration = leading_empty_duration.saturating_add(segment_duration);
        } else {
            break;
        }
    }

    Ok((leading_empty_duration > 0)
        .then(|| duration_units_to_ms(leading_empty_duration, movie_timescale)))
}

fn find_mp4_child_box(
    file: &mut File,
    start: u64,
    end: u64,
    name: [u8; 4],
) -> std::io::Result<Option<Mp4BoxHeader>> {
    file.seek(SeekFrom::Start(start))?;
    while file.stream_position()? < end {
        let Some(child) = read_mp4_box_header(file, end)? else {
            return Ok(None);
        };
        if child.name == name {
            return Ok(Some(child));
        }
        file.seek(SeekFrom::Start(child.end))?;
    }
    Ok(None)
}

fn read_mp4_box_header(file: &mut File, parent_end: u64) -> std::io::Result<Option<Mp4BoxHeader>> {
    let start = file.stream_position()?;
    if start.saturating_add(8) > parent_end {
        return Ok(None);
    }

    let size32 = read_be_u32(file)?;
    let name = read_box_name(file)?;
    let (size, header_len) = match size32 {
        0 => (parent_end.saturating_sub(start), 8),
        1 => (read_be_u64(file)?, 16),
        size => (u64::from(size), 8),
    };
    if size < header_len || start.saturating_add(size) > parent_end {
        return Ok(None);
    }

    Ok(Some(Mp4BoxHeader {
        name,
        payload_start: start + header_len,
        end: start + size,
    }))
}

fn duration_units_to_ms(duration: u64, timescale: u32) -> u64 {
    if timescale == 0 {
        return 0;
    }
    let value = (u128::from(duration) * 1_000) + (u128::from(timescale) / 2);
    let ms = value / u128::from(timescale);
    ms.min(u128::from(u64::MAX)) as u64
}

fn read_box_name(file: &mut File) -> std::io::Result<[u8; 4]> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_u8(file: &mut File) -> std::io::Result<u8> {
    let mut bytes = [0_u8; 1];
    file.read_exact(&mut bytes)?;
    Ok(bytes[0])
}

fn read_be_u32(file: &mut File) -> std::io::Result<u32> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_be_i32(file: &mut File) -> std::io::Result<i32> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes)?;
    Ok(i32::from_be_bytes(bytes))
}

fn read_be_u64(file: &mut File) -> std::io::Result<u64> {
    let mut bytes = [0_u8; 8];
    file.read_exact(&mut bytes)?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_be_i64(file: &mut File) -> std::io::Result<i64> {
    let mut bytes = [0_u8; 8];
    file.read_exact(&mut bytes)?;
    Ok(i64::from_be_bytes(bytes))
}

fn skip_bytes(file: &mut File, len: u64) -> std::io::Result<()> {
    let offset = i64::try_from(len).unwrap_or(i64::MAX);
    file.seek(SeekFrom::Current(offset))?;
    Ok(())
}

fn validate_local_file_path(path: &Path) -> Result<(), MediaPlayerError> {
    if !path.is_absolute() {
        return Err(MediaPlayerError::invalid_source(
            "Local media paths must be absolute.",
        ));
    }
    let metadata = std::fs::metadata(path).map_err(|e| {
        MediaPlayerError::invalid_source(format!("Local media file is not readable: {e}"))
    })?;
    if !metadata.is_file() {
        return Err(MediaPlayerError::invalid_source(
            "Local media path must point to a file.",
        ));
    }
    Ok(())
}

fn media_title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Untitled media")
        .to_string()
}

fn media_kind_from_extension(extension: &str) -> MediaKind {
    match extension {
        "aac" | "aif" | "aiff" | "alac" | "ape" | "flac" | "m4a" | "mp3" | "ogg" | "opus"
        | "wav" | "wma" => MediaKind::Audio,
        "avi" | "flv" | "m4v" | "mkv" | "mov" | "mp4" | "mpeg" | "mpg" | "ogv" | "webm" | "wmv" => {
            MediaKind::Video
        }
        _ => MediaKind::Unknown,
    }
}

fn open_playback_sink(sample_rate: SampleRate) -> Result<MixerDeviceSink, MediaPlayerError> {
    DeviceSinkBuilder::from_default_device()
        .map(|builder| builder.with_sample_rate(sample_rate))
        .and_then(|builder| builder.open_sink_or_fallback())
        .map_err(|e| {
            MediaPlayerError::audio_device(format!("Failed to open local audio output: {e}"))
        })
}

fn effective_native_volume(volume: f64, muted: bool) -> f32 {
    if muted {
        0.0
    } else {
        clamp_volume(volume) as f32
    }
}

fn duration_to_ms(duration: Option<Duration>) -> Option<u64> {
    duration.map(|value| {
        let millis = value.as_millis();
        if millis > u128::from(u64::MAX) {
            u64::MAX
        } else {
            millis as u64
        }
    })
}

fn clamp_volume(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        PlayerSnapshot::default().volume
    }
}

fn clamp_rate(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.25, 2.0)
    } else {
        PlayerSnapshot::default().rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn media_kind_uses_extension_groups() {
        assert_eq!(media_kind_from_extension("flac"), MediaKind::Audio);
        assert_eq!(media_kind_from_extension("mkv"), MediaKind::Video);
        assert_eq!(media_kind_from_extension("txt"), MediaKind::Unknown);
    }

    #[test]
    fn clamp_helpers_bound_user_values() {
        assert_eq!(clamp_volume(-1.0), 0.0);
        assert_eq!(clamp_volume(0.75), 0.75);
        assert_eq!(clamp_volume(1.25), 1.0);
        assert_eq!(clamp_volume(2.0), 1.0);
        assert_eq!(clamp_rate(0.1), 0.25);
        assert_eq!(clamp_rate(4.0), 2.0);
    }

    #[test]
    fn local_file_validation_rejects_relative_paths() {
        let error = validate_local_file_path(Path::new("song.mp3")).unwrap_err();
        assert_eq!(error.code, "invalidSource");
    }

    #[test]
    fn media_title_omits_file_extension() {
        assert_eq!(
            media_title_from_path(Path::new("/music/01 - Made in Abyss.mp3")),
            "01 - Made in Abyss"
        );
    }

    #[test]
    fn mp4_duration_conversion_rounds_to_milliseconds() {
        assert_eq!(duration_units_to_ms(62_561, 1_000), 62_561);
        assert_eq!(duration_units_to_ms(1, 3), 333);
    }

    #[test]
    fn mp4_probe_reads_leading_empty_edit_start() {
        let path =
            std::env::temp_dir().join(format!("ganbaru-ai-media-probe-{}.mp4", std::process::id()));
        std::fs::write(&path, minimal_mp4_with_leading_empty_edit(62_561)).unwrap();

        let start_ms = probe_mp4_leading_empty_edit_ms(&path);

        let _ = std::fs::remove_file(&path);
        assert_eq!(start_ms, Some(62_561));
    }

    #[test]
    fn player_core_loads_and_updates_snapshot() {
        let mut core = test_core();
        let snapshot = core
            .handle(BackendCommand::load(
                local_load_request(Some(1_500)),
                audio_probe(),
            ))
            .unwrap();

        assert_eq!(snapshot.status, PlayerStatus::Ready);
        assert_eq!(snapshot.position_ms, 1_500);
        assert_eq!(snapshot.volume, 0.75);
        assert!(!snapshot.muted);
        assert_eq!(snapshot.rate, 1.5);
        assert_eq!(snapshot.title.as_deref(), Some("Song title"));
        assert!(!snapshot.has_video);
        assert_eq!(snapshot.backend_kind, BackendKind::Rodio);
        assert_eq!(snapshot.playable_start_ms, None);
        assert_eq!(snapshot.duration_ms, Some(120_000));
    }

    #[test]
    fn player_core_pause_is_state_only_and_clears_errors() {
        let mut core = loaded_core();
        let playing = core.handle(BackendCommand::Play).unwrap();
        assert_eq!(playing.status, PlayerStatus::Playing);

        let snapshot = core.handle(BackendCommand::Pause).unwrap();
        assert_eq!(snapshot.status, PlayerStatus::Paused);
        assert_eq!(snapshot.error, None);
    }

    #[test]
    fn player_core_stop_releases_source_state_and_keeps_settings() {
        let mut core = loaded_core();
        core.handle(BackendCommand::SetMuted(true)).unwrap();
        let snapshot = core.handle(BackendCommand::Stop).unwrap();

        assert_eq!(snapshot.status, PlayerStatus::Idle);
        assert_eq!(snapshot.source_identity, None);
        assert_eq!(snapshot.position_ms, 0);
        assert_eq!(snapshot.volume, 0.75);
        assert!(snapshot.muted);
        assert_eq!(snapshot.rate, 1.5);
    }

    #[test]
    fn player_core_reloading_stops_previous_audio_backend() {
        let loaded_backends = Arc::new(Mutex::new(Vec::new()));
        let mut core = PlayerCore::with_audio_factory(Box::new(RecordingAudioFactory {
            loaded_backends: Arc::clone(&loaded_backends),
        }));
        core.handle(BackendCommand::load(
            local_load_request(None),
            audio_probe(),
        ))
        .unwrap();
        core.handle(BackendCommand::load(
            local_load_request(Some(5_000)),
            audio_probe(),
        ))
        .unwrap();

        let backends = loaded_backends.lock().unwrap();
        assert_eq!(backends.len(), 2);
        assert!(backends[0].lock().unwrap().stopped);
        assert!(!backends[1].lock().unwrap().stopped);
    }

    #[test]
    fn player_core_uses_webview_for_video() {
        let mut core = test_core();
        let loaded = core
            .handle(BackendCommand::load(
                local_load_request(None),
                video_probe(),
            ))
            .unwrap();
        assert_eq!(loaded.status, PlayerStatus::Ready);
        assert!(loaded.has_video);
        assert_eq!(loaded.playable_start_ms, Some(62_561));
        assert_eq!(loaded.backend_kind, BackendKind::Webview);
        assert_eq!(loaded.error, None);
    }

    #[test]
    fn player_core_muting_does_not_change_volume() {
        let mut core = loaded_core();
        let muted = core.handle(BackendCommand::SetMuted(true)).unwrap();
        assert!(muted.muted);
        assert_eq!(muted.volume, 0.75);

        let unmuted = core.handle(BackendCommand::SetMuted(false)).unwrap();
        assert!(!unmuted.muted);
        assert_eq!(unmuted.volume, 0.75);
    }

    #[test]
    fn playback_controller_dispatches_on_worker_thread() {
        let controller = PlaybackController::with_core(test_core());
        let loaded = controller
            .dispatch(BackendCommand::load(
                local_load_request(None),
                audio_probe(),
            ))
            .unwrap();
        assert_eq!(loaded.status, PlayerStatus::Ready);

        let seeked = controller.dispatch(BackendCommand::Seek(42_000)).unwrap();
        assert_eq!(seeked.position_ms, 42_000);

        let snapshot = controller.dispatch(BackendCommand::Snapshot).unwrap();
        assert_eq!(snapshot.position_ms, 42_000);
    }

    #[test]
    fn revoked_delivery_cannot_start_after_blocking_decode() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let directory = TestMediaDirectory::new();
        let path = directory.path().join("song.mp3");
        std::fs::write(&path, b"fake decoder input").unwrap();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (started, admission) = mpsc::sync_channel(1);
        let (release, blocked) = mpsc::sync_channel(1);
        let mut core = PlayerCore::with_audio_factory(Box::new(RecordingAudioFactory {
            loaded_backends: recorded.clone(),
        }));
        core.handle(BackendCommand::load(
            local_load_request(None),
            audio_probe(),
        ))
        .unwrap();
        core.handle(BackendCommand::Play).unwrap();
        core.audio_factory = Box::new(BlockingAudioFactory {
            started,
            blocked,
            recorded: recorded.clone(),
        });
        let controller = Arc::new(PlaybackController::with_core(core));
        let current = Arc::new(AtomicBool::new(true));
        let guard = current.clone();
        let task_controller = controller.clone();
        let effect = native_load_effect(&path);
        let task = std::thread::spawn(move || {
            task_controller.dispatch_authorized(
                BackendCommand::Session(Box::new(effect)),
                Arc::new(move || guard.load(Ordering::Acquire)),
            )
        });
        admission.recv_timeout(Duration::from_secs(2)).unwrap();
        let prior = recorded.lock().unwrap()[0].clone();
        assert!(prior.lock().unwrap().stopped);
        current.store(false, Ordering::Release);
        release.send(()).unwrap();
        assert_eq!(task.join().unwrap().unwrap_err().code, "deliveryRevoked");
        let backend = recorded.lock().unwrap()[1].clone();
        let backend = backend.lock().unwrap();
        assert_eq!(backend.play_count, 0);
        assert!(backend.stopped);
        drop(backend);
        assert_eq!(
            controller
                .dispatch(BackendCommand::Snapshot)
                .unwrap()
                .status,
            PlayerStatus::Idle
        );
    }

    #[test]
    fn decoder_timeout_retains_admission_until_actual_completion() {
        use std::sync::mpsc;
        let directory = TestMediaDirectory::new();
        let path = directory.path().join("song.mp3");
        std::fs::write(&path, b"fake decoder input").unwrap();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (started, admission) = mpsc::sync_channel(1);
        let (release, blocked) = mpsc::sync_channel(1);
        let controller = Arc::new(PlaybackController::with_core(
            PlayerCore::with_audio_factory(Box::new(BlockingAudioFactory {
                started,
                blocked,
                recorded: recorded.clone(),
            })),
        ));
        let task_controller = controller.clone();
        let effect = native_load_effect(&path);
        let task = std::thread::spawn(move || {
            task_controller.dispatch_with_timeout(
                BackendCommand::Session(Box::new(effect)),
                Arc::new(|| true),
                Duration::from_millis(100),
            )
        });
        admission.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(task.join().unwrap().unwrap_err().code, "backendTimeout");
        for _ in 0..100 {
            assert_eq!(
                controller.dispatch(BackendCommand::Stop).unwrap_err().code,
                "backendBusy"
            );
        }
        assert!(recorded.lock().unwrap().is_empty());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match controller.dispatch(BackendCommand::Snapshot) {
                Ok(snapshot) => {
                    assert_eq!(snapshot.status, PlayerStatus::Idle);
                    break;
                }
                Err(error)
                    if error.code == "backendBusy" && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                result => panic!("Decoder did not finish cancellation: {result:?}"),
            }
        }
        let backend = recorded.lock().unwrap()[0].clone();
        let backend = backend.lock().unwrap();
        assert_eq!(backend.play_count, 0);
        assert!(backend.stopped);
        drop(backend);
        assert_eq!(
            controller.dispatch(BackendCommand::Stop).unwrap().status,
            PlayerStatus::Idle
        );
    }

    #[test]
    fn dropping_controller_does_not_wait_for_a_stalled_source() {
        use std::sync::mpsc;
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (started, admission) = mpsc::sync_channel(1);
        let (release, blocked) = mpsc::sync_channel(1);
        let controller = Arc::new(PlaybackController::with_core(
            PlayerCore::with_audio_factory(Box::new(BlockingAudioFactory {
                started,
                blocked,
                recorded: recorded.clone(),
            })),
        ));
        let task_controller = controller.clone();
        let task = std::thread::spawn(move || {
            task_controller.dispatch_with_timeout(
                BackendCommand::load(local_load_request(None), audio_probe()),
                Arc::new(|| true),
                Duration::from_millis(100),
            )
        });
        admission.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(task.join().unwrap().unwrap_err().code, "backendTimeout");
        let (dropped, completion) = mpsc::sync_channel(1);
        let task = std::thread::spawn(move || {
            drop(controller);
            dropped.send(()).unwrap();
        });
        let result = completion.recv_timeout(Duration::from_secs(2));
        release.send(()).unwrap();
        result.unwrap();
        task.join().unwrap();
    }

    #[test]
    fn committed_settings_apply_together_without_starting_a_paused_decoder() {
        let mut core = loaded_core();
        let result = core
            .handle(BackendCommand::Session(Box::new(SessionEffect::Settings {
                generation: 1,
                volume: 0.5,
                muted: true,
                rate: 0.75,
            })))
            .unwrap();
        assert_eq!(result.status, PlayerStatus::Ready);
        assert_eq!(result.volume, 0.5);
        assert!(result.muted);
        assert_eq!(result.rate, 0.75);
    }

    fn native_load_effect(path: &Path) -> SessionEffect {
        use ganbaru_music_library::session::models::{SessionBackend, SessionSource, SourceKind};
        SessionEffect::Load {
            session_id: "session".into(),
            generation: 1,
            source: Box::new(SessionSource {
                kind: SourceKind::LocalFile,
                identity: "local:song".into(),
                original_input: "song".into(),
                title: "Song".into(),
                path: Some(path.to_string_lossy().into_owned()),
                artwork_path: None,
                video_id: None,
                playlist_id: None,
                start_ms: None,
                end_ms: None,
            }),
            backend: SessionBackend::NativeAudio,
            position_ms: 0,
            autoplay: true,
            volume: 0.75,
            muted: false,
            rate: 1.0,
        }
    }

    struct BlockingAudioFactory {
        started: std::sync::mpsc::SyncSender<()>,
        blocked: std::sync::mpsc::Receiver<()>,
        recorded: Arc<Mutex<Vec<Arc<Mutex<FakeAudioState>>>>>,
    }

    struct TestMediaDirectory(PathBuf);

    impl TestMediaDirectory {
        fn new() -> Self {
            static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ganbaru-music-worker-{}-{timestamp}-{sequence}",
                std::process::id(),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestMediaDirectory {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.0) {
                eprintln!("Remove Music test directory {}: {error}", self.0.display());
            }
        }
    }

    impl LocalAudioFactory for BlockingAudioFactory {
        fn load(
            &mut self,
            request: &LoadRequest,
            volume: f64,
            muted: bool,
            rate: f64,
        ) -> Result<Box<dyn LocalAudioBackend>, MediaPlayerError> {
            self.started
                .send(())
                .map_err(|_| MediaPlayerError::backend_thread())?;
            self.blocked
                .recv()
                .map_err(|_| MediaPlayerError::backend_thread())?;
            RecordingAudioFactory {
                loaded_backends: self.recorded.clone(),
            }
            .load(request, volume, muted, rate)
        }
    }

    fn test_core() -> PlayerCore {
        PlayerCore::with_audio_factory(Box::new(FakeAudioFactory))
    }

    fn loaded_core() -> PlayerCore {
        let mut core = test_core();
        core.handle(BackendCommand::load(
            local_load_request(None),
            audio_probe(),
        ))
        .unwrap();
        core
    }

    fn local_load_request(start_ms: Option<u64>) -> LoadRequest {
        LoadRequest {
            source: LocalMediaSource {
                kind: "local-file".to_string(),
                path: "/music/song.mp3".to_string(),
                identity: "local:/music/song.mp3".to_string(),
                title: Some("Song title".to_string()),
            },
            start_ms,
            volume: Some(0.75),
            rate: Some(1.5),
        }
    }

    fn audio_probe() -> MediaProbe {
        MediaProbe {
            path: "/music/song.mp3".to_string(),
            title: "song".to_string(),
            file_size_bytes: 1024,
            extension: Some("mp3".to_string()),
            media_kind: MediaKind::Audio,
            playable_start_ms: None,
        }
    }

    fn video_probe() -> MediaProbe {
        MediaProbe {
            path: "/music/video.mp4".to_string(),
            title: "video".to_string(),
            file_size_bytes: 1024,
            extension: Some("mp4".to_string()),
            media_kind: MediaKind::Video,
            playable_start_ms: Some(62_561),
        }
    }

    fn minimal_mp4_with_leading_empty_edit(empty_duration: u32) -> Vec<u8> {
        let mut mvhd = vec![0, 0, 0, 0];
        mvhd.extend_from_slice(&0_u32.to_be_bytes());
        mvhd.extend_from_slice(&0_u32.to_be_bytes());
        mvhd.extend_from_slice(&1_000_u32.to_be_bytes());
        mvhd.extend_from_slice(&128_000_u32.to_be_bytes());

        let mut elst = vec![0, 0, 0, 0];
        elst.extend_from_slice(&2_u32.to_be_bytes());
        elst.extend_from_slice(&empty_duration.to_be_bytes());
        elst.extend_from_slice(&(-1_i32).to_be_bytes());
        elst.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
        elst.extend_from_slice(&65_000_u32.to_be_bytes());
        elst.extend_from_slice(&0_i32.to_be_bytes());
        elst.extend_from_slice(&0x0001_0000_u32.to_be_bytes());

        let elst = mp4_box(*b"elst", elst);
        let edts = mp4_box(*b"edts", elst);
        let trak = mp4_box(*b"trak", edts);
        mp4_box(*b"moov", [mp4_box(*b"mvhd", mvhd), trak].concat())
    }

    fn mp4_box(name: [u8; 4], payload: Vec<u8>) -> Vec<u8> {
        let size = u32::try_from(payload.len() + 8).unwrap();
        let mut bytes = Vec::with_capacity(size as usize);
        bytes.extend_from_slice(&size.to_be_bytes());
        bytes.extend_from_slice(&name);
        bytes.extend_from_slice(&payload);
        bytes
    }

    struct FakeAudioFactory;

    impl LocalAudioFactory for FakeAudioFactory {
        fn load(
            &mut self,
            request: &LoadRequest,
            volume: f64,
            muted: bool,
            rate: f64,
        ) -> Result<Box<dyn LocalAudioBackend>, MediaPlayerError> {
            Ok(Box::new(FakeAudioBackend {
                state: Arc::new(Mutex::new(FakeAudioState {
                    position_ms: request.start_ms.unwrap_or(0),
                    duration_ms: Some(120_000),
                    volume,
                    muted,
                    rate,
                    playing: false,
                    play_count: 0,
                    stopped: false,
                    empty: false,
                })),
            }))
        }
    }

    struct RecordingAudioFactory {
        loaded_backends: Arc<Mutex<Vec<Arc<Mutex<FakeAudioState>>>>>,
    }

    impl LocalAudioFactory for RecordingAudioFactory {
        fn load(
            &mut self,
            request: &LoadRequest,
            volume: f64,
            muted: bool,
            rate: f64,
        ) -> Result<Box<dyn LocalAudioBackend>, MediaPlayerError> {
            let state = Arc::new(Mutex::new(FakeAudioState {
                position_ms: request.start_ms.unwrap_or(0),
                duration_ms: Some(120_000),
                volume,
                muted,
                rate,
                playing: false,
                play_count: 0,
                stopped: false,
                empty: false,
            }));
            self.loaded_backends
                .lock()
                .unwrap()
                .push(Arc::clone(&state));
            Ok(Box::new(FakeAudioBackend { state }))
        }
    }

    struct FakeAudioBackend {
        state: Arc<Mutex<FakeAudioState>>,
    }

    struct FakeAudioState {
        position_ms: u64,
        duration_ms: Option<u64>,
        volume: f64,
        muted: bool,
        rate: f64,
        playing: bool,
        play_count: usize,
        stopped: bool,
        empty: bool,
    }

    impl LocalAudioBackend for FakeAudioBackend {
        fn play(&mut self) {
            let mut state = self.state.lock().unwrap();
            state.playing = true;
            state.play_count += 1;
        }

        fn pause(&mut self) {
            let mut state = self.state.lock().unwrap();
            state.playing = false;
        }

        fn stop(&mut self) {
            let mut state = self.state.lock().unwrap();
            state.stopped = true;
            state.playing = false;
        }

        fn seek(&mut self, position_ms: u64) -> Result<(), MediaPlayerError> {
            let mut state = self.state.lock().unwrap();
            state.position_ms = position_ms;
            Ok(())
        }

        fn set_volume(&mut self, volume: f64, muted: bool) {
            let mut state = self.state.lock().unwrap();
            state.volume = volume;
            state.muted = muted;
        }

        fn set_rate(&mut self, rate: f64) {
            let mut state = self.state.lock().unwrap();
            state.rate = rate;
        }

        fn position_ms(&self) -> u64 {
            self.state.lock().unwrap().position_ms
        }

        fn duration_ms(&self) -> Option<u64> {
            self.state.lock().unwrap().duration_ms
        }

        fn is_empty(&self) -> bool {
            self.state.lock().unwrap().empty
        }
    }
}
